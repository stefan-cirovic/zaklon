//! Offline maps from CoMaps (OpenStreetMap data, ODbL). The whole world is
//! offered in pieces (countries, and regions of large countries) exactly as
//! CoMaps publishes them; people pick what they need. The hub downloads the
//! map files and serves them on the local network, and serves the CoMaps app
//! itself, so phones get maps without internet.
//!
//! The list of pieces must match the CoMaps app version the hub hands out:
//! a map file is only usable by an app built for the same data version.
//!
//! The whole world as one pack for the Zaklon map (Protomaps, PMTiles) is
//! in [`crate::world_map`]; [`packs`] lists it with the others.

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

/// Every map piece as a downloadable pack, plus the CoMaps app and the world
/// map for the Zaklon map (the build pinned in the app; the hub offers the
/// build it chooses from Protomaps' list in its place).
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
                blake3: None,
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
            blake3: None,
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
    out.push(crate::world_map::pinned());
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
    fn the_world_map_is_listed_with_its_checksum() {
        let p = packs().into_iter().find(|p| p.id == crate::world_map::WORLD_MAP_ID).expect("the world map is listed");
        assert!(p.is_safe());
        assert_eq!((p.category, p.topics.clone()), (Category::Maps, vec!["maps".to_string()]));
        assert_eq!(p.files[0].path, "maps/protomaps-world-20260928.pmtiles");
        assert_eq!(p.files[0].sha256, crate::world_map::PINNED_SHA256);
    }
}
