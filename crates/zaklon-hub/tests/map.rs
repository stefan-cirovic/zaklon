//! The Zaklon map against a real hub: tiles from the map archives (a map
//! pack downloaded in Maps and the world overview that comes with the app, the
//! most detailed first, tile by tile), the fonts and icons of its style, a
//! map pack put in place by hand, who may read all this, and the
//! household's home location with "find a place".
//!
//! Run with: `cargo test -p zaklon-hub --test map -- --nocapture`

mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use common::*;
use reqwest::Method;
use serde_json::{json, Value};

const PASSWORD: &str = "correct horse";

/// Write a tiny map archive: a tile for each of `tiles`, whose data is the
/// given text, gzip-compressed as Protomaps does it.
fn write_archive(path: &Path, min_zoom: u8, max_zoom: u8, tiles: &[((u8, u32, u32), &str)]) {
    let mut tiles = tiles.to_vec();
    tiles.sort_by_key(|((z, x, y), _)| pmtiles::TileId::from(pmtiles::TileCoord::new(*z, *x, *y).unwrap()).value());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut w = pmtiles::PmTilesWriter::new(pmtiles::TileType::Mvt)
        .tile_compression(pmtiles::Compression::Gzip)
        .min_zoom(min_zoom)
        .max_zoom(max_zoom)
        .create(std::fs::File::create(path).unwrap())
        .unwrap();
    for ((z, x, y), data) in tiles {
        w.add_tile(pmtiles::TileCoord::new(z, x, y).unwrap(), data.as_bytes()).unwrap();
    }
    w.finalize().unwrap();
}

fn gunzip(data: &[u8]) -> String {
    use std::io::Read;
    let mut out = String::new();
    flate2::read::GzDecoder::new(data).read_to_string(&mut out).unwrap();
    out
}

/// A raw GET on the laptop's address: the status, the headers and the body as it came.
async fn raw(hub: &Hub, path: &str, headers: &[(&str, &str)]) -> (u16, reqwest::header::HeaderMap, Vec<u8>) {
    let mut req = hub.http.get(format!("{}{path}", hub.local));
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let r = req.send().await.unwrap();
    let (st, h) = (r.status().as_u16(), r.headers().clone());
    (st, h, r.bytes().await.unwrap().to_vec())
}

async fn tile_text(hub: &Hub, z: u8, x: u32, y: u32) -> (u16, String) {
    let (st, h, body) = raw(hub, &format!("/tiles/{z}/{x}/{y}.mvt"), &[]).await;
    if st != 200 {
        return (st, String::new());
    }
    assert_eq!(h["content-encoding"], "gzip", "stored compressed, sent as it is");
    assert_eq!(h["content-type"], "application/vnd.mapbox-vector-tile");
    (st, gunzip(&body))
}

/// The status of a pack, as the app shows it.
async fn pack_status(hub: &Hub, id: &str) -> String {
    let (_, cat) = hub.get("/api/catalog").await;
    let pack = cat["packs"].as_array().unwrap().iter().find(|p| p["id"] == id).unwrap_or_else(|| panic!("{id} is listed in the catalog"));
    pack["state"]["status"].as_str().unwrap().to_string()
}

/// Wait until the pack `id` is installed.
async fn installed(hub: &Hub, id: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let st = pack_status(hub, id).await;
        if st == "installed" {
            return;
        }
        assert!(Instant::now() < deadline, "{id} stuck in {st}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// The map's summary once `until` holds for it.
async fn map_until(hub: &Hub, what: &str, until: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let (st, map) = hub.get("/api/map").await;
        assert_eq!(st, 200, "{map}");
        if until(&map) {
            return map;
        }
        assert!(Instant::now() < deadline, "{what}: {map}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_map_comes_from_the_hub_most_detailed_first() {
    let root = temp_dir("map-Đorđe");
    let assets = temp_dir("map-assets");

    // The app's own assets: the overview (zoom 0-1), a font range and the icons.
    write_archive(&assets.join("overview.pmtiles"), 0, 1, &[((0, 0, 0), "overview 0"), ((1, 0, 0), "overview 1/0/0"), ((1, 1, 1), "overview 1/1/1")]);
    std::fs::create_dir_all(assets.join("fonts/Noto Sans Regular")).unwrap();
    std::fs::write(assets.join("fonts/Noto Sans Regular/0-255.pbf"), b"glyphs 0-255").unwrap();
    std::fs::create_dir_all(assets.join("sprites")).unwrap();
    std::fs::write(assets.join("sprites/dark.json"), br#"{"icon":{}}"#).unwrap();
    std::fs::write(assets.join("sprites/dark@2x.png"), b"\x89PNG").unwrap();
    std::fs::write(
        assets.join("cities.tsv"),
        "Belgrade\tBelgrade\tBeograd\tCentral Serbia\tRS\t44.8040\t20.4651\t1273651\n\
         Novi Sad\tNovi Sad\t\tVojvodina\tRS\t45.2517\t19.8369\t215400\n\
         Belgrade\tBelgrade\t\tMontana\tUS\t45.7760\t-111.1769\t8029\n",
    )
    .unwrap();

    // The world map pack (zoom 0-3, without tile 1/1/1), put in the
    // hub's maps folder by hand before the hub starts; and one of the wrong
    // size, which is never used.
    let world = temp_dir("map-world").join("world.pmtiles");
    write_archive(&world, 0, 3, &[((0, 0, 0), "world 0"), ((1, 0, 0), "world 1/0/0"), ((3, 4, 2), "world 3/4/2")]);
    let world_bytes = std::fs::read(&world).unwrap();
    let pack = |id: &str, path: &str, size: usize| {
        json!({
            "id": id,
            "title": { "en": id },
            "category": "maps",
            "version": "20260928",
            "size": size,
            "files": [{ "path": path, "urls": ["https://127.0.0.1:9/world.pmtiles"], "sha256": sha256_hex(&world_bytes), "size": size }]
        })
    };
    let catalog = json!({
        "version": 1,
        "generated": "2999-01-01",
        "packs": [pack("test-world", "maps/test-world.pmtiles", world_bytes.len()), pack("test-short", "maps/test-short.pmtiles", world_bytes.len() - 10)]
    });
    std::fs::create_dir_all(root.join("catalog")).unwrap();
    std::fs::write(root.join("catalog/catalog.json"), catalog.to_string()).unwrap();
    std::fs::create_dir_all(root.join("library/maps")).unwrap();
    std::fs::write(root.join("library/maps/test-world.pmtiles"), &world_bytes).unwrap();
    std::fs::write(root.join("library/maps/test-short.pmtiles"), &world_bytes[..world_bytes.len() - 1]).unwrap();

    std::env::set_var("ZAKLON_MAP_ASSETS", &assets);
    let hub = run_hub(root.clone()).await;

    // 1. The pack found in place is used at once, while the hub checks it.
    let map = map_until(&hub, "the world map is used", |m| m["tiles"]["detailed"] == true).await;
    assert_eq!(map["tiles"]["max_zoom"], 3);
    assert_eq!(map["tiles"]["min_zoom"], 0);
    assert_eq!(map["tiles"]["overview"], true);
    assert_eq!((map["glyphs"].as_bool(), map["sprites"].as_bool()), (Some(true), Some(true)));
    // The largest map pack (the real world map, once the catalog offers it).
    assert!(["test-world", "world-map"].contains(&map["world"]["id"].as_str().unwrap_or_default()), "{map}");
    let key = map["tiles"]["key"].as_str().unwrap().to_string();
    // It is installed once checked; the one of the wrong size fails.
    installed(&hub, "test-world").await;
    assert_eq!(pack_status(&hub, "test-short").await, "failed");

    // 2. Tiles: the most detailed archive that has one, tile by tile.
    assert_eq!(tile_text(&hub, 0, 0, 0).await, (200, "world 0".into()));
    assert_eq!(tile_text(&hub, 1, 1, 1).await, (200, "overview 1/1/1".into()), "falls back to the overview");
    assert_eq!(tile_text(&hub, 3, 4, 2).await, (200, "world 3/4/2".into()));
    assert_eq!(tile_text(&hub, 3, 0, 0).await.0, 204, "inside the zoom levels, no tile: empty");
    assert_eq!(tile_text(&hub, 4, 0, 0).await.0, 404, "beyond the zoom levels");
    assert_eq!(tile_text(&hub, 1, 5, 0).await.0, 404, "no such tile");
    assert_eq!(raw(&hub, "/tiles/0/0/0.png", &[]).await.0, 404);
    // Cached by the browser, and asked again only if changed.
    let (_, h, _) = raw(&hub, "/tiles/0/0/0.mvt", &[]).await;
    let etag = h["etag"].to_str().unwrap().to_string();
    assert!(h["cache-control"].to_str().unwrap().contains("max-age"));
    assert_eq!(raw(&hub, "/tiles/0/0/0.mvt", &[("if-none-match", &etag)]).await.0, 304);

    // 3. The style's fonts and icons, and nothing else of the assets.
    let (st, h, body) = raw(&hub, "/map/fonts/Noto%20Sans%20Regular/0-255.pbf", &[]).await;
    assert_eq!((st, body.as_slice()), (200, b"glyphs 0-255".as_slice()));
    assert_eq!(h["content-type"], "application/x-protobuf");
    assert_eq!(raw(&hub, "/map/fonts/Noto%20Sans%20Regular/256-511.pbf", &[]).await.0, 404);
    // A font without glyphs of its own in a range is drawn with the regular one there.
    let (st, _, body) = raw(&hub, "/map/fonts/Noto%20Sans%20Devanagari%20Regular%20v1/0-255.pbf", &[]).await;
    assert_eq!((st, body.as_slice()), (200, b"glyphs 0-255".as_slice()));
    let (st, h, _) = raw(&hub, "/map/sprites/dark.json", &[]).await;
    assert_eq!((st, h["content-type"].to_str().unwrap()), (200, "application/json"));
    assert_eq!(raw(&hub, "/map/sprites/dark@2x.png", &[]).await.0, 200);
    for escape in ["/map/sprites/..%2Fcities.tsv", "/map/fonts/..%2F..%2Fcities.tsv/0-255.pbf", "/map/fonts/Noto%20Sans%20Regular/..%2F..%2Fcities.tsv"] {
        assert_eq!(raw(&hub, escape, &[]).await.0, 404, "{escape}");
    }

    // 4. Only the household: not another website, not a stranger on the network.
    let (st, _, _) = raw(&hub, "/tiles/0/0/0.mvt", &[("origin", "http://evil.example"), ("sec-fetch-site", "cross-site")]).await;
    assert_eq!(st, 403);
    assert_eq!(hub.post("/api/setup", json!({ "password": PASSWORD, "hub_name": "Map hub", "language": "en" })).await.0, 204);
    let fingerprint = hub.get("/api/status").await.1["fingerprint"].as_str().unwrap().to_string();
    let phone = phone_client(&fingerprint);
    let r = phone.get(format!("{}/tiles/0/0/0.mvt", hub.tls)).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401, "a device that is not paired");
    let (_, start) = hub.post("/api/pair/start", json!({})).await;
    let r = phone
        .post(format!("{}/api/pair/complete", hub.tls))
        .json(&json!({ "secret": start["payload"]["secret"], "password": PASSWORD, "device_name": "Ana's phone" }))
        .send()
        .await
        .unwrap();
    let token = r.json::<Value>().await.unwrap()["device_token"].as_str().unwrap().to_string();
    let r = phone.get(format!("{}/tiles/3/4/2.mvt", hub.tls)).bearer_auth(&token).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200, "a paired phone");
    assert_eq!(gunzip(&r.bytes().await.unwrap()), "world 3/4/2");
    let r = phone.get(format!("{}/map/sprites/dark.json", hub.tls)).bearer_auth(&token).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);

    // 5. Removed on the laptop (while the map has it open): only the overview is left.
    assert_eq!(hub.send(Method::DELETE, "/api/packs/test-world", None).await.0, 204);
    assert!(!root.join("library/maps/test-world.pmtiles").exists());
    let map = map_until(&hub, "the world map is gone", |m| m["tiles"]["detailed"] == false).await;
    assert_eq!(map["tiles"]["max_zoom"], 1);
    assert_ne!(map["tiles"]["key"], key.as_str(), "another set of tiles, another address");
    assert_eq!(tile_text(&hub, 0, 0, 0).await, (200, "overview 0".into()));
    assert_eq!(tile_text(&hub, 3, 4, 2).await.0, 404);

    // 6. Put back by hand while the hub runs: noticed, used, checked.
    std::fs::write(root.join("library/maps/test-world.pmtiles"), &world_bytes).unwrap();
    map_until(&hub, "the world map is noticed", |m| m["tiles"]["detailed"] == true).await;
    installed(&hub, "test-world").await;
    assert_eq!(tile_text(&hub, 3, 4, 2).await, (200, "world 3/4/2".into()));

    // 7. The home location: one for the household, set on the laptop or a phone.
    let as_phone = |method: Method, path: &str| phone.request(method, format!("{}{path}", hub.tls)).bearer_auth(&token);
    assert_eq!(hub.get("/api/home-location").await, (200, json!({ "home": null })));
    let (st, set) = hub
        .send(Method::PUT, "/api/home-location", Some(json!({ "lat": 44.8040123456, "lon": 20.4651, "label": "Belgrade, Central Serbia\n", "source": "place" })))
        .await;
    assert_eq!(st, 200, "{set}");
    assert_eq!((set["home"]["lat"].as_f64(), set["home"]["lon"].as_f64()), (Some(44.804012), Some(20.4651)));
    assert_eq!(set["home"]["label"], "Belgrade, Central Serbia");
    assert_eq!(set["home"]["set_by"], "laptop");
    let seen: Value = as_phone(Method::GET, "/api/home-location").send().await.unwrap().json().await.unwrap();
    assert_eq!(seen["home"]["label"], "Belgrade, Central Serbia", "every device sees it");
    assert_eq!(hub.get("/api/map").await.1["home"]["lat"], 44.804012, "the map knows it too");
    let r = as_phone(Method::PUT, "/api/home-location").json(&json!({ "lat": 45.2517, "lon": 19.8369, "source": "gps" })).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200, "a phone may set it");
    let (_, home) = hub.get("/api/home-location").await;
    assert_eq!((home["home"]["source"].as_str(), home["home"]["set_by"].as_str()), (Some("gps"), Some("Ana's phone")));
    assert!(home["home"].get("label").is_none(), "the old name does not stay with the new place");
    for (lat, lon) in [(91.0, 20.0), (-90.5, 0.0), (45.0, 180.5), (45.0, -181.0)] {
        let (st, e) = hub.send(Method::PUT, "/api/home-location", Some(json!({ "lat": lat, "lon": lon }))).await;
        assert_eq!((st, e["code"].as_str()), (400, Some("bad_location")), "{lat} {lon}");
    }
    assert_eq!(hub.get("/api/home-location").await.1["home"]["source"], "gps", "a refused place changes nothing");
    let r = phone.get(format!("{}/api/home-location", hub.tls)).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401, "not for a device that is not paired");
    assert_eq!(as_phone(Method::DELETE, "/api/home-location").send().await.unwrap().status().as_u16(), 204);
    assert_eq!(hub.get("/api/home-location").await.1, json!({ "home": null }));

    // 8. "Find a place", by any of its names, the larger first.
    let (st, found) = hub.get("/api/map/places?q=beograd").await;
    assert_eq!(st, 200, "{found}");
    assert_eq!(found[0]["name"], "Belgrade");
    assert_eq!(found[0]["name_sr"], "Beograd");
    assert_eq!(found[0]["country"], "RS");
    let (_, found) = hub.get("/api/map/places?q=Belgrade").await;
    assert_eq!(found.as_array().unwrap().iter().map(|p| p["country"].as_str().unwrap()).collect::<Vec<_>>(), ["RS", "US"]);
    assert_eq!(hub.get("/api/map/places?q=n").await.1, json!([]), "too short");
    let r = as_phone(Method::GET, "/api/map/places?q=novi%20sad").send().await.unwrap();
    assert_eq!(r.json::<Value>().await.unwrap()[0]["name"], "Novi Sad");
    assert_eq!(hub.get("/api/map").await.1["places"], true);
}
