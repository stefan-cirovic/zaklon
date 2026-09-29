//! End-to-end test of a real hub: it opens a fresh data folder, listens on
//! free ports, and is driven over HTTP exactly like the desktop window
//! (loopback) and a paired phone (TLS pinned to the hub's certificate) would.
//!
//! Run with: `cargo test -p zaklon-hub --test e2e -- --nocapture`

mod common;

use std::net::SocketAddr;
use std::path::Path;
use std::time::{Duration, Instant};

use common::*;
use serde_json::{json, Value};

// ---- helpers ------------------------------------------------------------------

/// Serve `dir` over HTTP (with Range support) on a free port; returns the base URL.
async fn file_server(dir: &Path) -> String {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = axum::Router::new().fallback_service(tower_http::services::ServeDir::new(dir));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

/// A pairing by QR code from the phone `client`: the status and the reply.
async fn pair_by_qr(client: &reqwest::Client, hub: &Hub, body: Value) -> (u16, Value) {
    let r = client.post(format!("{}/api/pair/complete", hub.tls)).json(&body).send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
}

async fn wait_pack(hub: &Hub, id: &str, until: &[&str]) -> Value {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let (_, cat) = hub.get("/api/catalog").await;
        let pack = cat["packs"].as_array().unwrap().iter().find(|p| p["id"] == id).unwrap().clone();
        let status = pack["state"]["status"].as_str().unwrap().to_string();
        if until.contains(&status.as_str()) {
            return pack;
        }
        assert!(Instant::now() < deadline, "pack {id} stuck in {status}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn start_hub() -> Hub {
    // Windows accounts are often named like this, and the data folder lives under them.
    let root = temp_dir("hub-Đorđe-Ћирић");

    // A small test catalog served by a local file server.
    let files = temp_dir("files");
    let payload: Vec<u8> = (0..3_000_000u32).map(|i| (i.wrapping_mul(2654435761) >> 24) as u8).collect();
    std::fs::write(files.join("tiny_test_2026-01.zim"), &payload).unwrap();
    let server = file_server(&files).await;
    let catalog = json!({
        "version": 1,
        "generated": "2999-01-01",
        "packs": [{
            "id": "test-pack",
            "title": { "en": "Test pack", "sr": "Test paket" },
            "category": "knowledge",
            "version": "2026-01",
            "size": payload.len(),
            "files": [{
                "path": "zim/tiny_test_2026-01.zim",
                "urls": [format!("{server}/tiny_test_2026-01.zim")],
                "sha256": sha256_hex(&payload),
                "size": payload.len()
            }]
        }, {
            "id": "test-model",
            "title": { "en": "Test model" },
            "category": "model",
            "version": "1",
            "size": payload.len(),
            "files": [{
                "path": "models/test-model.gguf",
                "urls": [format!("{server}/tiny_test_2026-01.zim")],
                "sha256": sha256_hex(&payload),
                "size": payload.len()
            }]
        }, {
            "id": "broken-pack",
            "title": { "en": "Broken pack" },
            "category": "knowledge",
            "version": "1",
            "size": payload.len(),
            "files": [{
                "path": "zim/broken.zim",
                "urls": [format!("{server}/tiny_test_2026-01.zim")],
                "sha256": "0".repeat(64),
                "size": payload.len()
            }]
        }]
    });
    std::fs::create_dir_all(root.join("catalog")).unwrap();
    std::fs::write(root.join("catalog/catalog.json"), catalog.to_string()).unwrap();
    run_hub(root).await
}

// ---- the test -------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn full_hub_flow() {
    let hub = start_hub().await;

    // 1. Fresh hub: not set up; the laptop sees details.
    let (st, status) = hub.get("/api/status").await;
    assert_eq!(st, 200);
    assert_eq!(status["set_up"], false);
    assert!(status["devices"].is_number(), "laptop sees details");
    let fingerprint = status["fingerprint"].as_str().unwrap().to_string();
    assert_eq!(fingerprint.len(), 64);

    // 2. A phone that does not know the fingerprint yet can read only the public status.
    let phone = phone_client(&fingerprint);
    let (st, public) = {
        let r = phone.get(format!("{}/api/status", hub.tls)).send().await.unwrap();
        (r.status().as_u16(), r.json::<Value>().await.unwrap())
    };
    assert_eq!(st, 200);
    assert!(public.get("devices").is_none(), "no details for strangers");
    assert!(public.get("root").is_none());

    // 3. A client pinned to a different fingerprint must refuse the hub.
    let impostor = phone_client(&"ab".repeat(32));
    assert!(impostor.get(format!("{}/api/status", hub.tls)).send().await.is_err(), "pinning rejects other certs");

    // 4. Setup: short password refused, then accepted, then not repeatable.
    let (st, _) = hub.post("/api/setup", json!({ "password": "short" })).await;
    assert_eq!(st, 400);
    let (st, _) = hub.post("/api/setup", json!({ "password": "correct horse", "hub_name": "E2E hub", "language": "sr" })).await;
    assert_eq!(st, 204);
    let (st, _) = hub.post("/api/setup", json!({ "password": "another one" })).await;
    assert_eq!(st, 400);
    assert_eq!(hub.get("/api/status").await.1["hub_name"], "E2E hub");

    // 5. Other websites in a browser on the laptop cannot drive the hub.
    let evil = hub
        .http
        .post(format!("{}/api/pair/start", hub.local))
        .header("origin", "http://evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(evil.status().as_u16(), 403, "cross-site request refused");
    let rebind = hub
        .http
        .get(format!("{}/api/devices", hub.local))
        .header("host", "evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(rebind.status().as_u16(), 403, "DNS rebinding refused");

    // 6. Nothing private is reachable over the network without a token.
    let r = phone.get(format!("{}/api/devices", hub.tls)).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);
    let r = phone.get(format!("{}/api/devices", hub.tls)).bearer_auth("not-a-token").send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);

    // 7. Pairing by QR code. The QR code carries a long secret, not the
    //    6-digit code, and the phone pairs with that secret and the household
    //    password. A wrong secret (the 6-digit code too) gets the same answer
    //    as no open code at all, and every failed request takes one of the
    //    code's three attempts.
    let (_, pair) = hub.post("/api/pair/start", json!({})).await;
    let code = pair["code"].as_str().unwrap().to_string();
    let secret = pair["payload"]["secret"].as_str().unwrap().to_string();
    assert_eq!(pair["payload"]["v"], 2);
    assert_eq!(pair["payload"]["fp"], fingerprint.as_str());
    assert_eq!(secret.len(), 32);
    assert!(pair["payload"].get("code").is_none(), "the QR code does not carry the 6-digit code");
    let other = phone_client_from(&fingerprint, other_device(2));
    let (st, body) = pair_by_qr(&phone, &hub, json!({ "secret": code, "password": "correct horse", "device_name": "x" })).await;
    assert_eq!((st, body["code"].as_str()), (403, Some("code_expired")), "the 6-digit code is no secret");
    // Only who has the QR code learns that the password was wrong.
    let (st, body) = pair_by_qr(&phone, &hub, json!({ "secret": secret, "password": "wrong password", "device_name": "x" })).await;
    assert_eq!((st, body["code"].as_str()), (403, Some("wrong_password")));
    let (st, body) = pair_by_qr(&other, &hub, json!({ "secret": "0".repeat(32), "password": "correct horse", "device_name": "x" })).await;
    assert_eq!((st, body["code"].as_str()), (403, Some("code_expired")));
    // Three attempts are used: now even the right secret and password are refused.
    let (st, body) = pair_by_qr(&other, &hub, json!({ "secret": secret, "password": "correct horse", "device_name": "x" })).await;
    assert_eq!((st, body["code"].as_str()), (403, Some("too_many_attempts")), "burned code stays invalid");
    let (st, body) = pair_by_qr(&other, &hub, json!({ "secret": secret, "password": "correct horse", "device_name": "x" })).await;
    assert_eq!((st, body["code"].as_str()), (403, Some("code_expired")));

    // 8. Pairing with a fresh code and the right password. Starting a new code
    //    cancels the previous one. One address may take only two of a code's
    //    three attempts.
    let (_, old) = hub.post("/api/pair/start", json!({})).await;
    let (_, pair) = hub.post("/api/pair/start", json!({})).await;
    let (st, _) = pair_by_qr(&phone, &hub, json!({ "secret": old["payload"]["secret"], "password": "correct horse", "device_name": "x" })).await;
    assert_eq!(st, 403, "an older code is no longer valid");
    let (st, _) = pair_by_qr(&phone, &hub, json!({ "secret": pair["payload"]["secret"], "password": "wrong password", "device_name": "x" })).await;
    assert_eq!(st, 403);
    let (st, body) = pair_by_qr(&phone, &hub, json!({ "secret": pair["payload"]["secret"], "password": "correct horse", "device_name": "x" })).await;
    assert_eq!((st, body["code"].as_str()), (403, Some("too_many_attempts")), "this address has had its two");
    let (_, pair) = hub.post("/api/pair/start", json!({})).await;
    let request = json!({ "secret": pair["payload"]["secret"], "password": "correct horse", "device_name": "Ana's phone", "nonce": "0123456789abcdef-e2e" });
    let r = phone.post(format!("{}/api/pair/complete", hub.tls)).json(&request).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let paired: Value = r.json().await.unwrap();
    // The reply "got lost": the phone repeats the request and gets the same device, not a second one.
    let again: Value = phone.post(format!("{}/api/pair/complete", hub.tls)).json(&request).send().await.unwrap().json().await.unwrap();
    assert_eq!(again["device_id"], paired["device_id"]);
    let token = paired["device_token"].as_str().unwrap().to_string();
    assert_eq!(paired["fingerprint"], fingerprint.as_str());

    let as_phone = |method: reqwest::Method, path: &str| {
        phone.request(method, format!("{}{path}", hub.tls)).bearer_auth(&token)
    };

    let devices: Value = as_phone(reqwest::Method::GET, "/api/devices").send().await.unwrap().json().await.unwrap();
    assert_eq!(devices.as_array().unwrap().len(), 1);
    assert_eq!(devices[0]["name"], "Ana's phone");

    // 9. A phone cannot do laptop-only things.
    let r = as_phone(reqwest::Method::POST, "/api/pair/start").send().await.unwrap();
    assert_eq!(r.status().as_u16(), 403);
    let r = as_phone(reqwest::Method::POST, "/api/password").json(&json!({ "new_password": "hijacked!!" })).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 403);
    // Nor anything that touches this computer's data, network or drives.
    let laptop_only: &[(reqwest::Method, &str, Value)] = &[
        (reqwest::Method::GET, "/api/backups", Value::Null),
        (reqwest::Method::POST, "/api/backups", json!({ "dir": "" })),
        (reqwest::Method::POST, "/api/backups/restore", json!({ "path": "C:\\nothing.zip" })),
        (reqwest::Method::POST, "/api/updates/settings", json!({ "enabled": false })),
        (reqwest::Method::POST, "/api/pinned-tool", json!({ "tool": "maps" })),
        (reqwest::Method::GET, "/api/hotspot", Value::Null),
        (reqwest::Method::POST, "/api/hotspot/start", json!({})),
        (reqwest::Method::POST, "/api/hotspot/stop", json!({})),
        (reqwest::Method::GET, "/api/firewall", Value::Null),
        (reqwest::Method::POST, "/api/firewall/allow", json!({})),
        (reqwest::Method::POST, "/api/firewall/private", json!({})),
        (reqwest::Method::POST, "/api/export/cancel", json!({})),
        (reqwest::Method::POST, "/api/packs/import", json!({ "dir": hub.root.display().to_string() })),
        (reqwest::Method::DELETE, "/api/packs/kiwix-tools", Value::Null),
        (reqwest::Method::DELETE, "/api/maps/rs", Value::Null),
    ];
    for (method, path, body) in laptop_only {
        let mut req = as_phone(method.clone(), path);
        if !body.is_null() {
            req = req.json(body);
        }
        let r = req.send().await.unwrap();
        assert_eq!(r.status().as_u16(), 403, "a phone must not reach {method} {path}");
    }

    // The water calculator's inputs are the household's, and any paired
    // phone may change them. Nothing is saved at first.
    let (status, empty) = hub.get("/api/water").await;
    assert_eq!((status, empty["plan"].clone()), (200, Value::Null));
    let plan = json!({ "v": 1, "drink": { "people": 4, "children": 1, "smallPets": 0, "largePets": 1, "days": 7 }, "garden": { "beds": [{ "id": "a", "by": "size", "length": 3, "width": 1.2, "crop": "tomatoes" }], "lat": 44.8, "month": 7 } });
    let r = as_phone(reqwest::Method::PUT, "/api/water").json(&json!({ "plan": plan })).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200, "a phone saves the water plan");
    let (_, on_laptop) = hub.get("/api/water").await;
    assert_eq!(on_laptop["plan"], plan);
    assert_eq!(on_laptop["updated_by"], "Ana's phone");
    assert!(on_laptop["updated_at"].as_str().is_some_and(|t| t.len() >= 20), "{on_laptop}");
    // The laptop changes it; the phone reads the laptop's plan.
    let plan2 = json!({ "v": 1, "drink": { "people": 2, "days": 3 }, "garden": { "beds": [] } });
    assert_eq!(hub.send(reqwest::Method::PUT, "/api/water", Some(json!({ "plan": plan2 }))).await.0, 200);
    let on_phone: Value = as_phone(reqwest::Method::GET, "/api/water").send().await.unwrap().json().await.unwrap();
    assert_eq!((on_phone["plan"].clone(), on_phone["updated_by"].clone()), (plan2.clone(), json!("laptop")));
    // Every save is a new revision, also within the same second.
    assert!(on_phone["rev"].is_string() && on_phone["rev"] != on_laptop["rev"], "{on_phone} {on_laptop}");
    // Not a JSON object, or far too large: refused, and the plan stays.
    for refused in [json!({ "plan": [1, 2, 3] }), json!({ "plan": "4 people" }), json!({ "plan": { "garden": "x".repeat(40_000) } })] {
        let r = as_phone(reqwest::Method::PUT, "/api/water").json(&refused).send().await.unwrap();
        assert_eq!(r.status().as_u16(), 400);
    }
    assert_eq!(hub.get("/api/water").await.1["plan"], plan2);
    // Cleared: no plan again.
    assert_eq!(hub.send(reqwest::Method::PUT, "/api/water", Some(json!({ "plan": null }))).await.0, 200);
    assert_eq!(hub.get("/api/water").await.1["plan"], Value::Null);
    let stranger = phone.get(format!("{}/api/water", hub.tls)).send().await.unwrap();
    assert_eq!(stranger.status().as_u16(), 401, "only the household reads the water plan");

    // The tool pinned to the bar is the household's: the laptop pins it,
    // every phone reads the same one. Nothing is pinned at first.
    let phone_pinned = || {
        let req = as_phone(reqwest::Method::GET, "/api/pinned-tool");
        async move {
            let r = req.send().await.unwrap();
            assert_eq!(r.status().as_u16(), 200);
            r.json::<Value>().await.unwrap()["tool"].clone()
        }
    };
    assert_eq!(phone_pinned().await, Value::Null, "nothing pinned by default (the refused phone changed nothing)");
    assert_eq!(hub.post("/api/pinned-tool", json!({ "tool": "supplies" })).await, (200, json!({ "tool": "supplies" })));
    assert_eq!(phone_pinned().await, "supplies");
    // Pinning another replaces it; a name that is no tool id is refused.
    assert_eq!(hub.post("/api/pinned-tool", json!({ "tool": "library" })).await.0, 200);
    assert_eq!(hub.post("/api/pinned-tool", json!({ "tool": "../hub.json" })).await.0, 400);
    assert_eq!(hub.get("/api/pinned-tool").await, (200, json!({ "tool": "library" })));
    assert_eq!(phone_pinned().await, "library");
    // Unpinned: nothing again.
    assert_eq!(hub.post("/api/pinned-tool", json!({ "tool": null })).await.0, 200);
    assert_eq!(phone_pinned().await, Value::Null);
    let stranger = phone.get(format!("{}/api/pinned-tool", hub.tls)).send().await.unwrap();
    assert_eq!(stranger.status().as_u16(), 401, "only the household reads it");

    // The power calculator's list is the household's too, but not only the
    // laptop's: any paired phone may change it. Nothing is saved at first.
    let (status, empty) = hub.get("/api/power").await;
    assert_eq!((status, empty["plan"].clone()), (200, Value::Null));
    let plan = json!({ "v": 1, "lines": [{ "k": "a", "id": "fridge", "qty": 1, "watts": 200, "hours": 24, "whDay": 1200 }], "days": 3 });
    let r = as_phone(reqwest::Method::PUT, "/api/power").json(&json!({ "plan": plan })).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200, "a phone saves the list");
    let (_, on_laptop) = hub.get("/api/power").await;
    assert_eq!(on_laptop["plan"], plan);
    assert_eq!(on_laptop["updated_by"], "Ana's phone");
    assert!(on_laptop["updated_at"].as_str().is_some_and(|t| t.len() >= 20), "{on_laptop}");
    // The laptop changes it; the phone reads the laptop's list.
    let plan2 = json!({ "v": 1, "lines": [], "days": 7 });
    assert_eq!(hub.send(reqwest::Method::PUT, "/api/power", Some(json!({ "plan": plan2 }))).await.0, 200);
    let on_phone: Value = as_phone(reqwest::Method::GET, "/api/power").send().await.unwrap().json().await.unwrap();
    assert_eq!((on_phone["plan"].clone(), on_phone["updated_by"].clone()), (plan2.clone(), json!("laptop")));
    // Every save is a new revision, also within the same second.
    assert!(on_phone["rev"].is_string() && on_phone["rev"] != on_laptop["rev"], "{on_phone} {on_laptop}");
    // Not a JSON object, or far too large: refused, and the list stays.
    for refused in [json!({ "plan": [1, 2, 3] }), json!({ "plan": { "lines": "x".repeat(40_000) } })] {
        let r = as_phone(reqwest::Method::PUT, "/api/power").json(&refused).send().await.unwrap();
        assert_eq!(r.status().as_u16(), 400);
    }
    assert_eq!(hub.get("/api/power").await.1["plan"], plan2);
    // Cleared: no list again.
    assert_eq!(hub.send(reqwest::Method::PUT, "/api/power", Some(json!({ "plan": null }))).await.0, 200);
    assert_eq!(hub.get("/api/power").await.1["plan"], Value::Null);
    let stranger = phone.get(format!("{}/api/power", hub.tls)).send().await.unwrap();
    assert_eq!(stranger.status().as_u16(), 401, "only the household reads the list");
    // Other websites in the laptop's browser are refused on the local port too:
    // a foreign origin, the opaque "null" origin (sandboxed pages, file://) and
    // a request the browser marks as cross-site.
    for (name, value) in [("origin", "http://evil.example"), ("origin", "null"), ("sec-fetch-site", "cross-site")] {
        for (method, path) in [(reqwest::Method::POST, "/api/backups"), (reqwest::Method::POST, "/api/export/cancel"), (reqwest::Method::GET, "/api/backups")] {
            let mut req = hub.http.request(method.clone(), format!("{}{path}", hub.local)).header(name, value);
            if method == reqwest::Method::POST {
                req = req.json(&json!({ "dir": "" }));
            }
            let r = req.send().await.unwrap();
            assert_eq!(r.status().as_u16(), 403, "{name}: {value} must not reach {method} {path}");
        }
    }
    // A lookalike of the hub's own origin on another port is foreign as well.
    let port = hub.local.rsplit(':').next().unwrap().parse::<u16>().unwrap();
    let r = hub
        .http
        .post(format!("{}/api/export/cancel", hub.local))
        .header("origin", format!("http://127.0.0.1:{}", port.wrapping_add(1)))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 403, "another local port is another site");
    // The hub's own page is still let in, and no backup was made by the refused requests.
    let r = hub.http.get(format!("{}/api/backups", hub.local)).header("origin", hub.local.as_str()).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let backups: Value = r.json().await.unwrap();
    // (The hub's own daily backup may be there; a hand-made one must not.)
    assert!(
        backups["backups"].as_array().unwrap().iter().all(|b| b["automatic"] == true),
        "refused requests made no backup: {backups}"
    );

    // 10. Supplies from the phone, with history naming the phone.
    let r = as_phone(reqwest::Method::POST, "/api/items")
        .json(&json!({ "name": "Brašno", "quantity": 2, "unit": "kg", "category": "food", "place": "pantry", "min_quantity": 3, "barcode": "8600000000017" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 201);
    let item: Value = r.json().await.unwrap();
    let id = item["id"].as_str().unwrap().to_string();
    let adjusted: Value = as_phone(reqwest::Method::POST, &format!("/api/items/{id}/adjust"))
        .json(&json!({ "delta": -0.5 }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(adjusted["quantity"], 1.5);
    let r = as_phone(reqwest::Method::POST, "/api/items").json(&json!({ "name": "x", "category": "weapons" })).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 400, "unknown category refused");
    let r = as_phone(reqwest::Method::POST, &format!("/api/items/{id}/adjust")).json(&json!({ "delta": 0 })).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 400, "zero delta refused");

    let (_, summary) = hub.get("/api/supplies/summary").await;
    assert_eq!(summary["running_low"][0]["name"], "Brašno");
    let (_, shopping) = hub.get("/api/shopping").await;
    assert_eq!(shopping[0]["source"], "running_low");
    // Bought in the shop (phone), put away at home with a date (laptop).
    let low_id = shopping[0]["id"].as_str().unwrap().to_string();
    let r = as_phone(reqwest::Method::POST, &format!("/api/shopping/{low_id}/bought")).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 204);
    let (_, away) = hub.get("/api/put-away").await;
    let away_id = away[0]["id"].as_str().unwrap().to_string();
    let (st, put) = hub.post(&format!("/api/put-away/{away_id}"), json!({ "quantity": 2, "expiry": "2027-05-31" })).await;
    assert_eq!(st, 200, "{put}");
    assert_eq!(put["quantity"], 3.5);
    assert_eq!(put["batches"].as_array().unwrap().len(), 2);
    assert_eq!(put["expiry"], "2027-05-31");
    let (_, away) = hub.get("/api/put-away").await;
    assert_eq!(away, json!([]));
    let (_, code) = hub.get("/api/barcodes/8600000000017").await;
    assert_eq!(code["item"]["name"], "Brašno");
    let (_, history) = hub.get("/api/history?limit=5").await;
    assert_eq!(history[0]["action"], "add", "put away is recorded as restocking");
    assert_eq!(history[0]["actor"], "laptop");
    let consume = history.as_array().unwrap().iter().find(|h| h["action"] == "consume").expect("consume in history");
    assert_eq!(consume["actor"], "Ana's phone");

    // 11. Add-ons: download, verify and install a pack.
    let (st, _) = hub.post("/api/packs/test-pack/download", json!({})).await;
    assert_eq!(st, 202);
    let pack = wait_pack(&hub, "test-pack", &["installed", "failed"]).await;
    assert_eq!(pack["state"]["status"], "installed", "{pack}");
    let installed = hub.root.join("library/zim/tiny_test_2026-01.zim");
    assert_eq!(std::fs::metadata(&installed).unwrap().len(), 3_000_000);

    // 12. A pack whose checksum does not match is rejected and nothing is kept.
    hub.post("/api/packs/broken-pack/download", json!({})).await;
    let pack = wait_pack(&hub, "broken-pack", &["installed", "failed"]).await;
    assert_eq!(pack["state"]["status"], "failed");
    assert!(pack["state"]["error"].as_str().unwrap().contains("checksum"));
    assert!(!hub.root.join("library/zim/broken.zim").exists());

    // 13. Export to a "USB stick", remove, import back.
    let usb = temp_dir("usb");
    let (st, err) = hub.post("/api/export", json!({ "dir": usb.display().to_string(), "ids": ["broken-pack"] })).await;
    assert_eq!(st, 400, "only installed packs can be copied: {err}");
    let (st, _) = hub.post("/api/export", json!({ "dir": usb.join("missing").display().to_string(), "ids": ["test-pack"] })).await;
    assert_eq!(st, 400, "the folder must exist");
    let (st, _) = hub.post("/api/export", json!({ "dir": usb.display().to_string(), "ids": ["test-pack"] })).await;
    assert_eq!(st, 202);
    let deadline = Instant::now() + Duration::from_secs(20);
    let ex = loop {
        let (_, ex) = hub.get("/api/export").await;
        if ex["running"] == false {
            break ex;
        }
        assert!(Instant::now() < deadline, "copy did not finish: {ex}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert_eq!(ex["finished"], true, "{ex}");
    assert_eq!(ex["bytes_done"], 3_000_000);
    assert!(usb.join("zaklon-packs/tiny_test_2026-01.zim").is_file());
    assert!(usb.join("zaklon-packs/README.txt").is_file());
    assert!(!usb.join("zaklon-packs/tiny_test_2026-01.zim.part").exists());
    // With the apps: the installer the setup kept and the phone app go to the drive's root.
    std::fs::create_dir_all(hub.root.join("library/installer")).unwrap();
    std::fs::write(hub.root.join("library/installer/Zaklon-setup.exe"), b"MZ installer").unwrap();
    let (_, ex) = hub.get("/api/export").await;
    assert_eq!(ex["apps"][0][0], "Zaklon-setup.exe");
    let (st, _) = hub.post("/api/export", json!({ "dir": usb.display().to_string(), "with_apps": true })).await;
    assert_eq!(st, 202);
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let (_, ex) = hub.get("/api/export").await;
        if ex["running"] == false {
            assert_eq!(ex["finished"], true, "{ex}");
            break;
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(std::fs::read(usb.join("Zaklon-setup.exe")).unwrap(), b"MZ installer");
    assert!(usb.join("ZAKLON-README.txt").is_file());
    // A phone cannot write to the laptop's drives or list them.
    let r = phone.post(format!("{}/api/export", hub.tls)).bearer_auth(&token).json(&json!({ "dir": usb.display().to_string(), "ids": ["test-pack"] })).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 403);
    let r = phone.get(format!("{}/api/drives", hub.tls)).bearer_auth(&token).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 403);
    let r = phone.get(format!("{}/api/hardware", hub.tls)).bearer_auth(&token).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let (_, hw) = hub.get("/api/hardware").await;
    assert!(hw["ram_total"].as_u64().unwrap() > 0);

    // A phone's outbox may send the same shopping list add twice; it counts once.
    let (st, a1) = hub.post("/api/shopping", json!({ "text": "Candles", "client_id": "offline-1-123" })).await;
    assert_eq!(st, 201);
    let (st, a2) = hub.post("/api/shopping", json!({ "text": "Candles", "client_id": "offline-1-123" })).await;
    assert_eq!(st, 201);
    assert_eq!(a1["id"], a2["id"]);
    let (_, list) = hub.get("/api/shopping").await;
    assert_eq!(list.as_array().unwrap().iter().filter(|e| e["text"] == "Candles").count(), 1);

    // The assistant: without the AI engine it says so, and a question
    // finishes with a clear error instead of hanging.
    let (st, ai) = hub.get("/api/assistant").await;
    assert_eq!(st, 200);
    assert_eq!(ai["engine"], "missing", "{ai}");
    assert!(ai["models"].is_array(), "the test catalog has no models: {ai}");
    // Only a model the catalog has is recommended, and only one that fits.
    assert!(ai["recommended"].is_null(), "{ai}");
    let (st, _) = hub.post("/api/assistant/ask", json!({ "question": "   " })).await;
    assert_eq!(st, 400, "an empty question is refused");
    let (st, asked) = hub.post("/api/assistant/ask", json!({ "question": "Koliko traje pasulj?", "language": "sr" })).await;
    assert_eq!(st, 200);
    let answer_id = asked["id"].as_str().unwrap().to_string();
    let deadline = Instant::now() + Duration::from_secs(20);
    let answer = loop {
        let (_, a) = hub.get(&format!("/api/assistant/answers/{answer_id}")).await;
        if a["status"] == "done" || a["status"] == "failed" {
            break a;
        }
        assert!(Instant::now() < deadline, "answer did not finish: {a}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert_eq!(answer["status"], "failed");
    assert_eq!(answer["language"], "sr");
    assert!(answer["error"].as_str().unwrap().contains("not installed") || answer["error"].as_str().unwrap().contains("no AI model"), "{answer}");
    let (st, _) = hub.post("/api/assistant/model", json!({ "id": "not-a-model" })).await;
    assert_eq!(st, 400);
    let r = phone.get(format!("{}/api/assistant", hub.tls)).bearer_auth(&token).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200, "phones use the hub's assistant");
    let (st, _) = hub.send(reqwest::Method::DELETE, "/api/packs/test-pack", None).await;
    assert_eq!(st, 204);
    assert!(!installed.exists());
    // The import is queued and copies in the background, with progress.
    let (_, importing) = hub.post("/api/packs/import", json!({ "dir": usb.display().to_string() })).await;
    assert_eq!(importing["importing"], json!(["test-pack"]));
    let pack = wait_pack(&hub, "test-pack", &["installed", "failed"]).await;
    assert_eq!(pack["state"]["status"], "installed", "{pack}");
    assert!(installed.exists());
    assert!(!hub.root.join("library/zim/tiny_test_2026-01.zim.import").exists());

    // 14. Resume: a partial download continues from where it stopped.
    hub.send(reqwest::Method::DELETE, "/api/packs/test-pack", None).await;
    let payload = std::fs::read(usb.join("zaklon-packs/tiny_test_2026-01.zim")).unwrap();
    std::fs::write(hub.root.join("library/zim/tiny_test_2026-01.zim.part"), &payload[..1_234_567]).unwrap();
    hub.post("/api/packs/test-pack/download", json!({})).await;
    let pack = wait_pack(&hub, "test-pack", &["installed", "failed"]).await;
    assert_eq!(pack["state"]["status"], "installed", "{pack}");
    assert_eq!(sha256_hex(&std::fs::read(&installed).unwrap()), sha256_hex(&payload));

    // 14b. AI models: a phone lists installed models and copies one, resuming midway.
    hub.post("/api/packs/test-model/download", json!({})).await;
    let pack = wait_pack(&hub, "test-model", &["installed", "failed"]).await;
    assert_eq!(pack["state"]["status"], "installed", "{pack}");
    let models: Value = as_phone(reqwest::Method::GET, "/api/models").send().await.unwrap().json().await.unwrap();
    assert_eq!(models[0]["id"], "test-model");
    assert_eq!(models[0]["size"], 3_000_000);
    let whole = as_phone(reqwest::Method::GET, "/api/models/test-model/file").send().await.unwrap();
    assert_eq!(whole.status().as_u16(), 200);
    let first = whole.bytes().await.unwrap();
    let tail = as_phone(reqwest::Method::GET, "/api/models/test-model/file")
        .header("range", "bytes=2000000-")
        .send()
        .await
        .unwrap();
    assert_eq!(tail.status().as_u16(), 206);
    let tail = tail.bytes().await.unwrap();
    assert_eq!(&first[2_000_000..], &tail[..], "resumed part matches");
    assert_eq!(sha256_hex(&first), models[0]["sha256"].as_str().unwrap());
    // A range with an end gets only up to that byte; a suffix range the last bytes.
    for (range, from, to) in [("bytes=10-19", 10, 19), ("bytes=-100", 2_999_900, 2_999_999), ("bytes=2999990-5000000", 2_999_990, 2_999_999)] {
        let r = as_phone(reqwest::Method::GET, "/api/models/test-model/file").header("range", range).send().await.unwrap();
        assert_eq!(r.status().as_u16(), 206, "{range}");
        assert_eq!(r.headers()["content-range"], format!("bytes {from}-{to}/3000000").as_str(), "{range}");
        assert_eq!(r.headers()["content-length"], (to - from + 1).to_string().as_str(), "{range}");
        assert_eq!(&r.bytes().await.unwrap()[..], &first[from..=to], "{range}");
    }
    for range in ["bytes=3000000-", "bytes=20-10", "bytes=-0", "bytes=x-"] {
        let r = as_phone(reqwest::Method::GET, "/api/models/test-model/file").header("range", range).send().await.unwrap();
        assert_eq!(r.status().as_u16(), 416, "{range}");
        assert_eq!(r.headers()["content-range"], "bytes */3000000", "{range}");
    }
    let r = as_phone(reqwest::Method::GET, "/api/models/test-pack/file").send().await.unwrap();
    assert_eq!(r.status().as_u16(), 404, "only models are served this way");

    // 15. Library pages may never run scripts (sandbox), even opened directly.
    let r = hub.http.get(format!("{}/kiwix/content/x/y", hub.local)).send().await.unwrap();
    if r.status().is_success() {
        assert_eq!(r.headers()["content-security-policy"], "sandbox allow-popups");
    }
    // ...but the page's own styles and images (marked cross-site because the
    // page is sandboxed) are still served; there is no engine here, so 503.
    for prefix in ["/kiwix/", "/kiwix-lat/"] {
        let r = hub
            .http
            .get(format!("{}{prefix}content/x/_mw_/style.css", hub.local))
            .header("sec-fetch-site", "cross-site")
            .send()
            .await
            .unwrap();
        assert_ne!(r.status().as_u16(), 403, "{prefix} resources must load inside the sandboxed page");
    }
    // Library pages are not readable from the network without a token.
    let r = phone.get(format!("{}/kiwix/content/x/y", hub.tls)).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);
    // A cross-site navigation is never trusted as the laptop.
    let r = hub.http.get(format!("{}/api/devices", hub.local)).header("sec-fetch-site", "cross-site").send().await.unwrap();
    assert_eq!(r.status().as_u16(), 403);

    // 15b. Without the library engine add-on the library says so.
    let (_, lib) = hub.get("/api/library").await;
    assert_eq!(lib["engine"], "missing");
    let (_, results) = hub.get("/api/library/search?q=voda").await;
    assert_eq!(results, json!([]));

    // 16. The install page is public, the APK folder serves only files that exist.
    let page = reqwest::get(format!("{}/get", hub.install)).await.unwrap();
    assert_eq!(page.status().as_u16(), 200);
    assert!(page.text().await.unwrap().contains("Instaliraj Zaklon"), "in the household's language (Serbian here)");
    let apk = reqwest::get(format!("{}/apk/zaklon.apk", hub.install)).await.unwrap();
    assert_eq!(apk.status().as_u16(), 404);
    // With both apps on the hub, Zaklon comes first and CoMaps is explained separately.
    let apk_dir = hub.root.join("library/apk");
    std::fs::create_dir_all(&apk_dir).unwrap();
    std::fs::write(apk_dir.join("comaps.apk"), b"x").unwrap();
    std::fs::write(apk_dir.join("zaklon.apk"), b"x").unwrap();
    let page = reqwest::get(format!("{}/get", hub.install)).await.unwrap().text().await.unwrap();
    let (zaklon, comaps) = (page.find("Preuzmi Zaklon").unwrap(), page.find("Preuzmi CoMaps").unwrap());
    assert!(zaklon < comaps, "Zaklon first");
    assert!(page.contains("Aplikacija za mape (po želji)"));
    std::fs::remove_file(apk_dir.join("comaps.apk")).unwrap();
    std::fs::remove_file(apk_dir.join("zaklon.apk")).unwrap();
    let sneaky = reqwest::get(format!("{}/apk/..%2F..%2Fhousehold%2Fhub.json", hub.install)).await.unwrap();
    assert_ne!(sneaky.status().as_u16(), 200, "no path traversal out of the APK folder");

    // 16b. Maps: the list of the world's pieces; a map file placed in the
    // library is served at the path CoMaps asks for, with Range.
    let (_, maps) = hub.get("/api/maps").await;
    assert_eq!(maps["version"], 260830);
    assert!(maps["countries"].as_array().unwrap().len() > 200);
    assert!(maps["countries"].as_array().unwrap().iter().all(|c| !c["id"].as_str().unwrap().starts_with("World")), "the world overview is not a country");
    assert!(maps["server_urls"][0].as_str().unwrap().starts_with("http://"));
    let map_dir = hub.root.join("library/maps/260830");
    std::fs::create_dir_all(&map_dir).unwrap();
    std::fs::write(map_dir.join("Test Land.mwm"), b"0123456789").unwrap();
    for path in ["/maps/2026.06.28/260830/Test%20Land.mwm", "/maps/260830/Test%20Land.mwm"] {
        let r = hub.http.get(format!("{}{path}", hub.install)).header("range", "bytes=4-").send().await.unwrap();
        assert_eq!(r.status().as_u16(), 206, "{path}");
        assert_eq!(&r.bytes().await.unwrap()[..], b"456789");
    }
    let r = reqwest::get(format!("{}/maps/260830/..%2F..%2Fhousehold%2Fhub.json", hub.install)).await.unwrap();
    assert_eq!(r.status().as_u16(), 404, "no escaping the maps folder");
    let (_, catalog) = hub.get("/api/catalog").await;
    assert!(catalog["packs"].as_array().unwrap().iter().all(|p| p["category"] != "maps"), "maps are not in the add-ons list");
    // A paired phone gets the map app over TLS (with its checksum) once the hub has it.
    let r = as_phone(reqwest::Method::GET, "/api/maps-app").send().await.unwrap();
    assert_eq!(r.status().as_u16(), 404, "not on the hub yet");

    // 17. Discovery beacon answers with the same fingerprint.
    let sock = tokio::net::UdpSocket::bind(("127.0.0.1", 0)).await.unwrap();
    // A short request gets no answer (no amplification)...
    sock.send_to(b"ZAKLON?", SocketAddr::from(([127, 0, 0, 1], hub.beacon_port))).await.unwrap();
    let mut probe = [0u8; 512];
    assert!(tokio::time::timeout(Duration::from_millis(500), sock.recv_from(&mut probe)).await.is_err(), "short request ignored");
    // ...a padded one does.
    let mut request = b"ZAKLON?".to_vec();
    request.resize(zaklon_hub::discovery::BEACON_MIN_REQUEST, 0);
    sock.send_to(&request, SocketAddr::from(([127, 0, 0, 1], hub.beacon_port))).await.unwrap();
    let mut buf = [0u8; 512];
    let (n, _) = tokio::time::timeout(Duration::from_secs(3), sock.recv_from(&mut buf)).await.expect("beacon reply").unwrap();
    let beacon: Value = serde_json::from_slice(&buf[..n]).unwrap();
    assert_eq!(beacon["fp"], fingerprint.as_str());

    // 18. Removing the device revokes its token at once.
    let device_id = paired["device_id"].as_str().unwrap();
    let (st, _) = hub.send(reqwest::Method::DELETE, &format!("/api/devices/{device_id}"), None).await;
    assert_eq!(st, 204);
    let r = as_phone(reqwest::Method::GET, "/api/items").send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);
    // Even the public status says so, so the phone app notices it was removed.
    let r = as_phone(reqwest::Method::GET, "/api/status").send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);
    let r = phone.get(format!("{}/api/status", hub.tls)).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200, "without a token the public status still works");

    // 19. The password can be changed from the laptop; the old one no longer pairs.
    let (st, _) = hub.post("/api/password", json!({ "new_password": "new household pw" })).await;
    assert_eq!(st, 204);
    let (_, pair) = hub.post("/api/pair/start", json!({})).await;
    let (st, body) = pair_by_qr(&phone, &hub, json!({ "secret": pair["payload"]["secret"], "password": "correct horse", "device_name": "x" })).await;
    assert_eq!((st, body["code"].as_str()), (403, Some("wrong_password")));

    // 20. Env port overrides were not written to disk.
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(hub.root.join("household/hub.json")).unwrap()).unwrap();
    assert_eq!(saved["port"], 8484);
    assert_eq!(saved["local_port"], 8481);
}
