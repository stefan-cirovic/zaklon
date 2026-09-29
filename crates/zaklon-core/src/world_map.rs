//! The whole world in one pack for the Zaklon map: a Protomaps basemap build
//! (OpenStreetMap data, ODbL), zoom 0-15, one PMTiles file of about 138 GB,
//! which the hub reads itself (see the hub's tiles.rs).
//!
//! Protomaps builds the basemap every day. It keeps the daily builds for a
//! week, and the last build of every schema version (4.15.1, 4.15.2, ...)
//! for good; all of them are listed in builds.json with their size, schema
//! version and BLAKE3 hash. The hub offers the newest build that is at least
//! a week old ([`choose`]): such a build survived the weekly cleanup, so its
//! address keeps working for as long as a household needs to download it.
//! The hub fetches the list itself (its world.rs); without one it offers the
//! build pinned here ([`pinned`]), which it also always recognizes on disk.
//!
//! A build's file is `maps/protomaps-world-<date>.pmtiles` in the library,
//! so the name says which build it is.

use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::catalog::{Category, Localized, Offer, Pack, PackFile};

/// The world map's pack id.
pub const WORLD_MAP_ID: &str = "world-map";
/// Protomaps' list of the builds it keeps.
pub const BUILDS_URL: &str = "https://build-metadata.protomaps.dev/builds.json";
/// Where a listed build is downloaded from: `<DOWNLOADS>/<key>`.
pub const DOWNLOADS: &str = "https://build.protomaps.com";
/// A build this old survived Protomaps' weekly cleanup of daily builds.
pub const MIN_AGE: time::Duration = time::Duration::days(7);
/// A whole-world build is far larger than this...
pub const MIN_SIZE: u64 = 50_000_000_000;
/// ...and far smaller than this (they are about 138 GB in 2026).
pub const MAX_SIZE: u64 = 400_000_000_000;
/// The basemap schema the map's style draws: `@protomaps/basemaps` 5.x (in
/// ui/package.json) is made for version 4 tiles, as every 4.x build is.
pub const SCHEMA_MAJOR: u64 = 4;

/// The build pinned in the app: offered when the hub has no list of builds,
/// and always recognized on disk (checked with its SHA-256).
const PINNED_DATE: &str = "20260928";
const PINNED_SIZE: u64 = 138_415_942_566;
/// The SHA-256 of the pinned build's file (lower-case hex).
pub const PINNED_SHA256: &str = "7561013a401aa44db88c80fad1fc3f1f9e41fff7bf0ae5986fa9c7b87725f8ec";

/// One build in Protomaps' list, as it names it. Fields the hub does not use
/// (the MD5 sum) are left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Build {
    /// Its file name, "20260811.pmtiles".
    pub key: String,
    pub size: u64,
    /// BLAKE3 of the file (hex). Older builds have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub b3sum: Option<String>,
    /// When it was published (RFC 3339).
    #[serde(default)]
    pub uploaded: String,
    /// The basemap schema version, "4.15.2".
    #[serde(default)]
    pub version: String,
}

impl Build {
    /// "20260811" of "20260811.pmtiles"; None for a name of any other form.
    pub fn date(&self) -> Option<&str> {
        let d = self.key.strip_suffix(".pmtiles")?;
        is_date(d).then_some(d)
    }

    pub fn uploaded_at(&self) -> Option<OffsetDateTime> {
        OffsetDateTime::parse(&self.uploaded, &Rfc3339).ok()
    }

    /// Everything but its age: a sane name and size, a BLAKE3 hash, and a
    /// schema version the map's style draws.
    pub fn usable(&self) -> bool {
        let b3 = self.b3sum.as_deref().is_some_and(|h| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()));
        let mut parts = self.version.split('.');
        let schema = parts.next().and_then(|m| m.parse::<u64>().ok()) == Some(SCHEMA_MAJOR) && parts.all(|p| p.parse::<u64>().is_ok());
        self.date().is_some() && (MIN_SIZE..=MAX_SIZE).contains(&self.size) && b3 && schema
    }

    /// Published at least [`MIN_AGE`] before `now`.
    pub fn old_enough(&self, now: OffsetDateTime) -> bool {
        self.uploaded_at().is_some_and(|t| now - t >= MIN_AGE)
    }
}

/// Eight digits, as in a build's name ("20260811").
fn is_date(d: &str) -> bool {
    d.len() == 8 && d.bytes().all(|b| b.is_ascii_digit())
}

/// The builds in a builds.json. An entry that cannot be read is left out;
/// anything but a list is an error.
pub fn parse_builds(text: &str) -> Result<Vec<Build>, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("not a list of builds: {e}"))?;
    let entries = v.as_array().ok_or("not a list of builds")?;
    Ok(entries.iter().filter_map(|e| serde_json::from_value(e.clone()).ok()).collect())
}

/// The build to offer: the newest one (by when it was published) that is at
/// least a week old and [usable](Build::usable).
pub fn choose(builds: &[Build], now: OffsetDateTime) -> Option<&Build> {
    builds.iter().filter(|b| b.usable() && b.old_enough(now)).max_by(|a, b| (a.uploaded_at(), &a.key).cmp(&(b.uploaded_at(), &b.key)))
}

/// Where a build lives in the library.
pub fn path_of(date: &str) -> String {
    format!("maps/protomaps-world-{date}.pmtiles")
}

/// The build (date) a library path of the world map names, if it is one.
pub fn date_of_path(path: &str) -> Option<&str> {
    let d = path.strip_prefix("maps/protomaps-world-")?.strip_suffix(".pmtiles")?;
    is_date(d).then_some(d)
}

/// A listed build as the world map pack, checked with its BLAKE3; None for
/// one that is not [usable](Build::usable).
pub fn pack_of(build: &Build) -> Option<Pack> {
    if !build.usable() {
        return None;
    }
    let date = build.date()?;
    Some(world_pack(
        date,
        PackFile {
            path: path_of(date),
            urls: vec![format!("{DOWNLOADS}/{}", build.key)],
            sha256: String::new(),
            sha1_base64: None,
            blake3: build.b3sum.as_deref().map(str::to_ascii_lowercase),
            size: build.size,
            unpack: None,
            unpack_to: None,
        },
    ))
}

/// The build pinned in the app, checked with its SHA-256.
pub fn pinned() -> Pack {
    world_pack(
        PINNED_DATE,
        PackFile {
            path: path_of(PINNED_DATE),
            urls: vec![format!("{DOWNLOADS}/{PINNED_DATE}.pmtiles")],
            sha256: PINNED_SHA256.into(),
            sha1_base64: None,
            blake3: None,
            size: PINNED_SIZE,
            unpack: None,
            unpack_to: None,
        },
    )
}

fn world_pack(date: &str, file: PackFile) -> Pack {
    Pack {
        id: WORLD_MAP_ID.into(),
        title: Localized { en: "World map (towns, streets and buildings)".into(), sr: "Mapa sveta (mesta, ulice i zgrade)".into() },
        description: Localized {
            en: "The whole world in detail for the Zaklon map, on the laptop and on phones at home. Very large: it needs a big disk and a long download, which continues after interruptions.".into(),
            sr: "Ceo svet do detalja za Zaklon mapu, na laptopu i na telefonima kod kuće. Veoma velika: treba joj veliki disk i dugo preuzimanje, koje se nastavlja posle prekida.".into(),
        },
        category: Category::Maps,
        topics: vec!["maps".into()],
        version: date.into(),
        size: file.size,
        files: vec![file],
        license: "ODbL-1.0".into(),
        attribution: "© OpenStreetMap contributors; map tiles built by Protomaps (protomaps.com)".into(),
        source: "https://protomaps.com".into(),
        languages: vec![],
        recommended_for: vec![],
        offer: Offer::Auto,
        offer_reason: String::new(),
    }
}

/// What the hub offers of the world map, and which builds it knows.
#[derive(Debug, Clone)]
pub struct WorldOffer {
    /// The build offered for download.
    pub offer: Pack,
    /// Every build whose file the hub recognizes on disk (by name and size)
    /// and can check: the pinned one first, which for its date wins over the
    /// list, then the listed ones.
    pub known: Vec<Pack>,
    /// The builds (their dates) that can still be downloaded, as far as the
    /// list says: an unfinished download of one of them goes on.
    pub downloadable: Vec<String>,
    /// The offer comes from Protomaps' list, not from the build pinned in the app.
    pub listed: bool,
}

impl WorldOffer {
    /// The same offer and the same builds as `other`.
    pub fn same(&self, other: &WorldOffer) -> bool {
        let versions = |w: &WorldOffer| w.known.iter().map(|p| p.version.clone()).collect::<Vec<_>>();
        self.offer.version == other.offer.version
            && self.offer.files.first().map(|f| &f.path) == other.offer.files.first().map(|f| &f.path)
            && versions(self) == versions(other)
            && self.downloadable == other.downloadable
            && self.listed == other.listed
    }
}

/// What to offer, from Protomaps' list (None when the hub has none, not
/// even from an earlier day): the chosen build, or the pinned one when there
/// is no list or nothing in it qualifies.
pub fn offer(list: Option<&[Build]>, now: OffsetDateTime) -> WorldOffer {
    let pinned = pinned();
    let mut listed: Vec<Pack> = Vec::new();
    for p in list.unwrap_or_default().iter().filter_map(pack_of) {
        if !listed.iter().any(|q| q.version == p.version) {
            listed.push(p);
        }
    }
    let mut downloadable: Vec<String> = listed.iter().map(|p| p.version.clone()).collect();
    let known: Vec<Pack> = std::iter::once(pinned.clone()).chain(listed.iter().filter(|p| p.version != pinned.version).cloned()).collect();
    match list.and_then(|l| choose(l, now)).and_then(pack_of) {
        Some(chosen) => WorldOffer { offer: chosen, known, downloadable, listed: true },
        None => {
            // Nothing better: the pinned build, which is then tried as it is.
            if !downloadable.contains(&pinned.version) {
                downloadable.push(pinned.version.clone());
            }
            WorldOffer { offer: pinned, known, downloadable, listed: false }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trimmed copy of Protomaps' builds.json of 2026-09-29.
    const FIXTURE: &str = include_str!("../testdata/protomaps-builds-20260929.json");

    fn at(ts: &str) -> OffsetDateTime {
        OffsetDateTime::parse(ts, &Rfc3339).unwrap()
    }

    fn today() -> OffsetDateTime {
        at("2026-09-29T09:00:00Z")
    }

    fn builds() -> Vec<Build> {
        parse_builds(FIXTURE).unwrap()
    }

    fn with(key: &str, change: impl FnOnce(&mut Build)) -> Vec<Build> {
        let mut list = builds();
        change(list.iter_mut().find(|b| b.key == key).unwrap());
        list
    }

    fn chosen(list: &[Build]) -> Option<String> {
        choose(list, today()).map(|b| b.key.clone())
    }

    #[test]
    fn the_list_is_read_as_protomaps_writes_it() {
        let list = builds();
        assert!(list.len() > 10);
        let b = list.iter().find(|b| b.key == "20260928.pmtiles").unwrap();
        assert_eq!(b.size, 138_415_942_566);
        assert_eq!(b.b3sum.as_deref(), Some("8d2d48dfc2524c5f0071acb5a7bc5e7eb5cd481915bc5b1cfc80fe2e7398fdf7"));
        assert_eq!(b.version, "4.15.2");
        assert_eq!(b.date(), Some("20260928"));
        assert!(b.usable());
        // The oldest entries have no hash at all.
        assert!(list.iter().any(|b| b.b3sum.is_none()));
        // An entry that cannot be read is left out; anything but a list is an error.
        let odd = r#"[{"key":"20260811.pmtiles","size":"big"},{"key":"20260811.pmtiles","size":137295889397}]"#;
        assert_eq!(parse_builds(odd).unwrap().len(), 1);
        assert!(parse_builds("<html>Not found</html>").is_err());
        assert!(parse_builds(r#"{"builds":[]}"#).is_err());
    }

    #[test]
    fn the_newest_build_a_week_old_is_chosen() {
        // The daily builds of the last week are too young: they may still go.
        assert_eq!(chosen(&builds()).as_deref(), Some("20260811.pmtiles"));
        // A week later the newest of them has stayed a week.
        assert_eq!(choose(&builds(), at("2026-10-05T09:00:00Z")).map(|b| b.key.as_str()), Some("20260927.pmtiles"));
        assert_eq!(choose(&builds(), at("2026-10-06T00:00:00Z")).map(|b| b.key.as_str()), Some("20260928.pmtiles"));
        // Exactly seven days is old enough; a second less is not.
        assert_eq!(choose(&builds(), at("2026-09-30T09:09:59.501Z")).map(|b| b.key.as_str()), Some("20260923.pmtiles"));
        assert_eq!(choose(&builds(), at("2026-09-30T09:09:58Z")).map(|b| b.key.as_str()), Some("20260811.pmtiles"));
    }

    #[test]
    fn a_build_without_a_blake3_hash_is_never_chosen() {
        assert_eq!(chosen(&with("20260811.pmtiles", |b| b.b3sum = None)).as_deref(), Some("20260722.pmtiles"));
        assert_eq!(chosen(&with("20260811.pmtiles", |b| b.b3sum = Some("not a hash".into()))).as_deref(), Some("20260722.pmtiles"));
        // Before 2025 Protomaps published no BLAKE3: nothing to choose then.
        assert_eq!(choose(&builds(), at("2025-01-10T00:00:00Z")), None);
    }

    #[test]
    fn only_a_schema_the_style_draws() {
        for v in ["5.0.0", "3.7.1", "", "4", "4.x.1", "v4.15.1"] {
            let list = with("20260811.pmtiles", |b| b.version = v.into());
            let expect = if v == "4" { "20260811.pmtiles" } else { "20260722.pmtiles" };
            assert_eq!(chosen(&list).as_deref(), Some(expect), "{v:?}");
        }
        // The old schemas in the list are never chosen, however new the list.
        assert!(builds().iter().filter(|b| !b.version.starts_with("4.")).all(|b| !b.usable()));
    }

    #[test]
    fn a_strange_name_or_size_is_never_chosen() {
        for key in ["20260811.pmtiles.bak", "../20260811.pmtiles", "2026081.pmtiles", "2026-08-1.pmtiles", "20260811.mbtiles", "world.pmtiles"] {
            assert_eq!(chosen(&with("20260811.pmtiles", |b| b.key = key.into())).as_deref(), Some("20260722.pmtiles"), "{key}");
        }
        for size in [0, 1_000_000, MIN_SIZE - 1, MAX_SIZE + 1, u64::MAX] {
            assert_eq!(chosen(&with("20260811.pmtiles", |b| b.size = size)).as_deref(), Some("20260722.pmtiles"), "{size}");
        }
        assert_eq!(chosen(&with("20260811.pmtiles", |b| b.uploaded = "yesterday".into())).as_deref(), Some("20260722.pmtiles"));
    }

    #[test]
    fn nothing_to_choose_from_an_empty_list() {
        assert_eq!(choose(&[], today()), None);
        assert_eq!(chosen(&parse_builds("[]").unwrap()), None);
    }

    #[test]
    fn a_listed_build_is_checked_with_its_blake3() {
        let list = builds();
        let w = offer(Some(&list), today());
        assert!(w.listed);
        let p = &w.offer;
        assert!(p.is_safe());
        assert_eq!((p.id.as_str(), p.version.as_str(), p.size), (WORLD_MAP_ID, "20260811", 137_295_889_397));
        assert_eq!((p.category.clone(), p.topics.clone(), p.offer), (Category::Maps, vec!["maps".to_string()], Offer::Auto));
        let f = &p.files[0];
        assert_eq!(f.path, "maps/protomaps-world-20260811.pmtiles");
        assert_eq!(f.urls, ["https://build.protomaps.com/20260811.pmtiles"]);
        assert_eq!(f.blake3.as_deref(), Some("b2aa7f4b1858ec873bd2fb6aff1393ce330ad4d236f2b4f9ad1875e910c1eb8e"));
        assert!(f.sha256.is_empty() && f.sha1_base64.is_none());
        assert_eq!(date_of_path(&f.path), Some("20260811"));
        // Every usable build can be recognized and downloaded; the daily ones too.
        assert!(w.downloadable.contains(&"20260926".to_string()) && w.downloadable.contains(&"20260811".to_string()));
        assert!(!w.downloadable.contains(&"20240812".to_string()), "an old schema is never downloaded");
    }

    #[test]
    fn without_a_list_the_pinned_build_is_offered() {
        for w in [offer(None, today()), offer(Some(&[]), today())] {
            assert!(!w.listed);
            assert_eq!(w.offer.version, "20260928");
            assert_eq!(w.downloadable, ["20260928"], "tried as it is: nothing better");
        }
        // A list with nothing that qualifies yet: the pinned build too.
        let w = offer(Some(&builds()), at("2025-01-10T00:00:00Z"));
        assert!(!w.listed);
        assert_eq!(w.offer.files[0].sha256, PINNED_SHA256);
        assert!(w.downloadable.contains(&"20260928".to_string()));
        let p = pinned();
        assert!(p.is_safe());
        assert_eq!(p.size, 138_415_942_566);
        assert_eq!(p.files[0].path, "maps/protomaps-world-20260928.pmtiles");
        assert_eq!(p.files[0].urls, ["https://build.protomaps.com/20260928.pmtiles"]);
        assert_eq!(p.files[0].sha256, PINNED_SHA256);
        assert!(p.files[0].blake3.is_none(), "checked with its SHA-256");
    }

    #[test]
    fn the_pinned_build_is_always_known_and_wins_for_its_date() {
        let list = builds();
        let w = offer(Some(&list), today());
        assert_eq!(w.known[0].files[0].sha256, PINNED_SHA256, "the pinned build first");
        let same_date: Vec<&Pack> = w.known.iter().filter(|p| p.version == "20260928").collect();
        assert_eq!(same_date.len(), 1, "the list's 20260928 does not replace it");
        assert!(same_date[0].files[0].blake3.is_none());
        assert!(w.known.iter().any(|p| p.version == "20260811" && p.files[0].blake3.is_some()));
        // Without the list it is still known; so is every usable listed build.
        assert_eq!(offer(None, today()).known.len(), 1);
        assert_eq!(w.known.len(), list.iter().filter(|b| b.usable()).count());
        assert!(w.same(&offer(Some(&list), today())));
        assert!(!w.same(&offer(None, today())));
    }

    #[test]
    fn a_world_map_path_names_its_build() {
        assert_eq!(date_of_path("maps/protomaps-world-20260928.pmtiles"), Some("20260928"));
        assert_eq!(path_of("20261019"), "maps/protomaps-world-20261019.pmtiles");
        for p in ["maps/protomaps-world-2026092.pmtiles", "maps/world.pmtiles", "maps/protomaps-world-20260928.pmtiles.part", "protomaps-world-20260928.pmtiles"] {
            assert_eq!(date_of_path(p), None, "{p}");
        }
    }
}
