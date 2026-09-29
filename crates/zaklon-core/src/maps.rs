//! Offline maps from CoMaps (OpenStreetMap data, ODbL). The whole world is
//! offered in pieces (countries, and regions of large countries) exactly as
//! CoMaps publishes them; people pick what they need. The hub downloads the
//! map files and serves them on the local network, and serves the CoMaps app
//! itself, so phones get maps without internet.
//!
//! The list of pieces must match the CoMaps app version the hub hands out:
//! a map file is only usable by an app built for the same data version.
//!
//! Also here: the whole world as one pack for the Zaklon map (Protomaps,
//! PMTiles), which the hub reads itself (see the hub's tiles.rs).

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::catalog::{Category, Localized, Offer, Pack, PackFile};

/// Region list from the CoMaps release whose app we serve (v2026.08.31-14).
const COUNTRIES: &str = include_str!("../catalog/comaps-countries-260830.json");
/// Region names in English and Serbian (Latin), from CoMaps' own translations.
const NAMES: &str = include_str!("../catalog/comaps-names-260830.json");
/// Where CoMaps publishes map files: <base>/<series>/<version>/<Region>.mwm
pub const MAPS_BASE: &str = "https://mapgen-fi-1.comaps.app/maps";

/// The CoMaps Android app the hub serves to phones.
pub const COMAPS_APK_ID: &str = "comaps-app";
const COMAPS_APK_VERSION: &str = "2026.08.31-14";
const COMAPS_APK_URL: &str =
    "https://codeberg.org/comaps/comaps/releases/download/v2026.08.31-14/CoMaps-26083114-main-release.apk";
const COMAPS_APK_SHA256: &str = "6fa705e67b464ef4c3daa7781c53ae6e5061858687e4c46b7836b4a99bab4ed3";
const COMAPS_APK_SIZE: u64 = 61_716_469;

/// Pack ids of map pieces start with this.
pub const MAP_ID_PREFIX: &str = "map:";

/// The whole world in one pack for the Zaklon map: the Protomaps basemap
/// build of 2026-09-28 (OpenStreetMap data, ODbL), zoom 0-15, as one
/// PMTiles file. A newer catalog can offer a newer build under this id.
pub const WORLD_MAP_ID: &str = "world-map";
const WORLD_MAP_VERSION: &str = "20260928";
const WORLD_MAP_URL: &str = "https://build.protomaps.com/20260928.pmtiles";
/// Where it goes in the library. A copy put there by hand (with this name and
/// size) is shown at once and checked in the background.
pub const WORLD_MAP_PATH: &str = "maps/protomaps-world-20260928.pmtiles";
const WORLD_MAP_SIZE: u64 = 138_415_942_566;
/// The SHA-256 of the world map file (lower-case hex). Without one the world
/// map would not be offered: a pack is never downloaded or trusted without
/// its checksum.
pub const WORLD_MAP_SHA256: &str = "7561013a401aa44db88c80fad1fc3f1f9e41fff7bf0ae5986fa9c7b87725f8ec";

/// The world map pack, once its checksum is known (see `WORLD_MAP_SHA256`).
pub fn world_map_pack() -> Option<Pack> {
    let known = WORLD_MAP_SHA256.len() == 64 && WORLD_MAP_SHA256.chars().all(|c| c.is_ascii_hexdigit());
    known.then(|| Pack {
        id: WORLD_MAP_ID.into(),
        title: Localized { en: "World map (towns, streets and buildings)".into(), sr: "Mapa sveta (mesta, ulice i zgrade)".into() },
        description: Localized {
            en: "The whole world in detail for the Zaklon map, on the laptop and on phones at home. Very large: it needs a big disk and a long download, which continues after interruptions.".into(),
            sr: "Ceo svet do detalja za Zaklon mapu, na laptopu i na telefonima kod kuće. Veoma velika: treba joj veliki disk i dugo preuzimanje, koje se nastavlja posle prekida.".into(),
        },
        category: Category::Maps,
        topics: vec!["maps".into()],
        version: WORLD_MAP_VERSION.into(),
        size: WORLD_MAP_SIZE,
        files: vec![PackFile {
            path: WORLD_MAP_PATH.into(),
            urls: vec![WORLD_MAP_URL.into()],
            sha256: WORLD_MAP_SHA256.to_ascii_lowercase(),
            sha1_base64: None,
            size: WORLD_MAP_SIZE,
            unpack: None,
            unpack_to: None,
        }],
        license: "ODbL-1.0".into(),
        attribution: "© OpenStreetMap contributors; map tiles built by Protomaps (protomaps.com)".into(),
        source: "https://protomaps.com".into(),
        languages: vec![],
        recommended_for: vec![],
        offer: Offer::Auto,
        offer_reason: String::new(),
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct MapRegion {
    pub id: String,
    pub size: u64,
    #[serde(skip)]
    pub sha1_base64: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MapCountry {
    pub id: String,
    pub size: u64,
    pub regions: Vec<MapRegion>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MapTree {
    pub version: u64,
    pub series: String,
    pub countries: Vec<MapCountry>,
    /// The world overview and coastlines. CoMaps needs them before any
    /// country, so the hub fetches them with the first map.
    pub base: Vec<MapRegion>,
}

/// Pack ids of the world overview maps.
pub const BASE_IDS: [&str; 2] = ["map:World", "map:WorldCoasts"];

#[derive(Deserialize)]
struct Node {
    id: String,
    #[serde(default)]
    g: Vec<Node>,
    #[serde(default)]
    s: u64,
    #[serde(default)]
    sha1_base64: String,
}

#[derive(Deserialize)]
struct Root {
    v: u64,
    map_series: String,
    g: Vec<Node>,
}

fn leaves(n: &Node, out: &mut Vec<MapRegion>) {
    if n.g.is_empty() {
        out.push(MapRegion { id: n.id.clone(), size: n.s, sha1_base64: n.sha1_base64.clone() });
    } else {
        for c in &n.g {
            leaves(c, out);
        }
    }
}

/// The world, as countries each made of one or more map pieces.
pub fn tree() -> &'static MapTree {
    static TREE: OnceLock<MapTree> = OnceLock::new();
    TREE.get_or_init(|| {
        let root: Root = serde_json::from_str(COUNTRIES).expect("bundled map list is valid");
        let is_base = |n: &Node| n.id == "World" || n.id == "WorldCoasts";
        let base = root.g.iter().filter(|n| is_base(n)).map(|n| MapRegion { id: n.id.clone(), size: n.s, sha1_base64: n.sha1_base64.clone() }).collect();
        let mut countries: Vec<MapCountry> = root
            .g
            .iter()
            .filter(|n| !is_base(n))
            .map(|n| {
                let mut regions = Vec::new();
                leaves(n, &mut regions);
                MapCountry { id: n.id.clone(), size: regions.iter().map(|r| r.size).sum(), regions }
            })
            .collect();
        countries.sort_by(|a, b| a.id.cmp(&b.id));
        MapTree { version: root.v, series: root.map_series, countries, base }
    })
}

/// Region ids read like "Germany_Free State of Bavaria_Upper Bavaria".
pub fn display_name(id: &str) -> String {
    id.replace('_', " – ")
}

#[derive(Deserialize, Default)]
struct Names {
    en: Option<String>,
    sr: Option<String>,
}

fn names() -> &'static HashMap<String, Names> {
    static NAMES_MAP: OnceLock<HashMap<String, Names>> = OnceLock::new();
    NAMES_MAP.get_or_init(|| serde_json::from_str(NAMES).expect("bundled map names are valid"))
}

/// The name people know a piece by, in English or Serbian ("en" / "sr").
pub fn local_name(id: &str, lang: &str) -> String {
    let n = names().get(id);
    let picked = match lang {
        "sr" => n.and_then(|n| n.sr.clone()),
        _ => None,
    };
    picked.or_else(|| n.and_then(|n| n.en.clone())).unwrap_or_else(|| display_name(id))
}

fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Library path of a map piece: maps/<version>/<Region>.mwm
pub fn region_path(version: u64, region: &str) -> String {
    format!("maps/{version}/{region}.mwm")
}

/// Every map piece as a downloadable pack, plus the CoMaps app.
pub fn packs() -> Vec<Pack> {
    let t = tree();
    let mut out: Vec<Pack> = t
        .countries
        .iter()
        .flat_map(|c| c.regions.iter())
        .chain(t.base.iter())
        .map(|r| Pack {
            id: format!("{MAP_ID_PREFIX}{}", r.id),
            title: Localized { en: local_name(&r.id, "en"), sr: local_name(&r.id, "sr") },
            description: Localized::default(),
            category: Category::Maps,
            topics: vec!["maps".into()],
            version: t.version.to_string(),
            size: r.size,
            files: vec![PackFile {
                path: region_path(t.version, &r.id),
                urls: vec![format!("{MAPS_BASE}/{}/{}/{}.mwm", t.series, t.version, url_encode(&r.id))],
                sha256: String::new(),
                sha1_base64: Some(r.sha1_base64.clone()),
                size: r.size,
                unpack: None,
                unpack_to: None,
            }],
            license: "ODbL-1.0".into(),
            attribution: "© OpenStreetMap contributors, CoMaps".into(),
            source: "https://www.comaps.app".into(),
            offer: Offer::Auto,
            offer_reason: String::new(),
            languages: vec![],
            recommended_for: vec![],
        })
        .collect();
    out.push(Pack {
        id: COMAPS_APK_ID.into(),
        title: Localized { en: "CoMaps map app for phones".into(), sr: "CoMaps aplikacija za mape (telefoni)".into() },
        description: Localized {
            en: "Offline maps and navigation for Android. The hub hands it to phones together with the maps.".into(),
            sr: "Offline mape i navigacija za Android. Hub je deli telefonima zajedno sa mapama.".into(),
        },
        category: Category::App,
        topics: Vec::new(),
        version: COMAPS_APK_VERSION.into(),
        size: COMAPS_APK_SIZE,
        files: vec![PackFile {
            path: "apk/comaps.apk".into(),
            urls: vec![COMAPS_APK_URL.into()],
            sha256: COMAPS_APK_SHA256.into(),
            sha1_base64: None,
            size: COMAPS_APK_SIZE,
            unpack: None,
            unpack_to: None,
        }],
        license: "Apache-2.0".into(),
        attribution: "CoMaps contributors".into(),
        source: "https://www.comaps.app".into(),
        offer: Offer::Auto,
        offer_reason: String::new(),
        languages: vec![],
        recommended_for: vec![],
    });
    out.extend(world_map_pack());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_list_is_complete_and_safe() {
        let t = tree();
        assert_eq!(t.version, 260830);
        assert!(t.countries.len() > 200);
        let serbia = t.countries.iter().find(|c| c.id == "Serbia").unwrap();
        assert_eq!(serbia.regions.len(), 1);
        assert!(serbia.size > 200_000_000);
        let germany = t.countries.iter().find(|c| c.id == "Germany").unwrap();
        assert!(germany.regions.len() > 5, "big countries come in regions");
        let p = packs();
        assert!(p.len() > 1000);
        for pack in &p {
            assert!(pack.is_safe(), "{}", pack.id);
        }
        let mne = p.iter().find(|x| x.id == "map:Montenegro").unwrap();
        assert_eq!(mne.files[0].path, "maps/260830/Montenegro.mwm");
        assert_eq!(mne.files[0].urls[0], "https://mapgen-fi-1.comaps.app/maps/2026.06.28/260830/Montenegro.mwm");
        assert_eq!(mne.files[0].sha1_base64.as_deref().map(str::len), Some(28));
        assert_eq!(t.base.len(), 2);
        for id in BASE_IDS {
            let b = p.iter().find(|x| x.id == id).unwrap();
            assert!(b.size > 1_000_000 && b.files[0].sha1_base64.is_some(), "{id}");
        }
        assert!(!t.countries.iter().any(|c| c.id.starts_with("World")));
        assert_eq!(local_name("Serbia", "sr"), "Srbija");
        assert_eq!(local_name("Serbia", "en"), "Serbia");
        assert_eq!(local_name("Macedonia", "en"), "North Macedonia");
        assert_eq!(local_name("Croatia_Central", "sr"), "Hrvatska — centar");
        // Every piece has a Serbian name, in Latin script.
        for c in &t.countries {
            for id in std::iter::once(&c.id).chain(c.regions.iter().map(|r| &r.id)) {
                let sr = local_name(id, "sr");
                assert!(!sr.chars().any(|ch| ('\u{0400}'..='\u{04FF}').contains(&ch)), "Cyrillic in {sr}");
            }
        }
    }

    #[test]
    fn the_world_map_is_offered_only_with_its_checksum() {
        let offered = packs().into_iter().find(|p| p.id == WORLD_MAP_ID);
        match world_map_pack() {
            None => assert!(offered.is_none(), "never offered without its checksum"),
            Some(w) => {
                let p = offered.expect("offered once its checksum is known");
                assert!(p.is_safe());
                assert_eq!((p.category, p.topics.clone()), (Category::Maps, vec!["maps".to_string()]));
                assert_eq!(p.size, 138_415_942_566);
                assert_eq!(p.files[0].path, "maps/protomaps-world-20260928.pmtiles");
                assert_eq!(p.files[0].urls, ["https://build.protomaps.com/20260928.pmtiles"]);
                assert_eq!(p.files[0].sha256.len(), 64);
                assert_eq!(w.files[0].sha256, p.files[0].sha256);
            }
        }
    }
}
