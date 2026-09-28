//! The add-on catalog: what can be downloaded, from where, and how to verify
//! it. A default catalog is bundled into the binary; a newer one can be placed
//! at `<root>/catalog/catalog.json` (fetched later from the project's signed
//! catalog endpoint or imported from USB).

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde::{Deserialize, Serialize};

const BUNDLED: &str = include_str!("../catalog/default-catalog.json");

/// The topics a pack can be about, in the order the Tools screen and the
/// Add-ons folders show them: health and first aid, water, food (recipes,
/// storing, canning), garden, power (small solar, batteries), build (setting
/// up solar, irrigation, rainwater and pumps, and repairs), knowledge
/// (encyclopedias, books, dictionaries) and maps.
pub const TOPICS: [&str; 8] = ["health", "water", "food", "garden", "power", "build", "knowledge", "maps"];

/// A pack's topics in a catalog from before a pack could have several,
/// named after the old Add-ons folders: "reference" (Wikipedia and books),
/// "health", "garden" (garden and food) and "skills" (repair and skills).
fn legacy_topics(topic: &str) -> Vec<&str> {
    match topic {
        "reference" => vec!["knowledge"],
        "garden" => vec!["garden", "food"],
        "skills" => vec!["build"],
        "" => vec![],
        other => vec![other],
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    pub version: u32,
    pub generated: String,
    /// What a household starts with, by app language (see [`StarterSet`]).
    #[serde(default)]
    pub starter_sets: Vec<StarterSet>,
    pub packs: Vec<Pack>,
    /// Packs the catalog offered once and no longer offers (the rights to
    /// their content turned out to be unclear, say), as they were listed.
    /// Nothing downloads them any more; a household that has one keeps it,
    /// sees it by name and can delete it. A pack that leaves `packs` moves
    /// here. Their files have no download addresses.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub withdrawn: Vec<Pack>,
}

/// The add-ons a household starts with, downloaded with one button together
/// with the AI model that fits the computer. Only packs offered as
/// [`Offer::Auto`] can be in one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StarterSet {
    /// The app language it is for ("sr", "en").
    pub lang: String,
    /// The packs, in the order they are listed and downloaded.
    pub packs: Vec<String>,
    /// The country whose map comes along, as the maps name it ("Serbia").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map: Option<String>,
}

/// How a pack may be offered, decided by its license.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Offer {
    /// Free for any use (public domain, CC0, CC BY, CC BY-SA, ODbL, OGL and
    /// the like): offered like any add-on, and may be in a starter set.
    #[default]
    Auto,
    /// Non-commercial or mixed licenses: listed apart, as a pack people
    /// download themselves once they confirmed its license. Never
    /// preselected and never in a starter set.
    User,
}

/// `offer` as a catalog names it. A kind this version does not know (from a
/// newer catalog) is treated as [`Offer::User`], the careful one.
fn offer_of<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Offer, D::Error> {
    let kind = String::deserialize(d)?;
    Ok(if kind == "auto" { Offer::Auto } else { Offer::User })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Knowledge,
    Maps,
    Model,
    App,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Localized {
    pub en: String,
    #[serde(default)]
    pub sr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackFile {
    /// Where the file lives, relative to `<root>/library/`.
    pub path: String,
    /// Download locations, tried in order. None for a withdrawn pack.
    #[serde(default)]
    pub urls: Vec<String>,
    /// Lower-case hex SHA-256 of the complete file (empty when only SHA-1 is published).
    #[serde(default)]
    pub sha256: String,
    /// Base64 SHA-1, as CoMaps publishes for map files. Used when there is no SHA-256.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1_base64: Option<String>,
    pub size: u64,
    /// "zip" to unpack after verification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unpack: Option<String>,
    /// Folder (relative to `library/`) to unpack into.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unpack_to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pack {
    pub id: String,
    pub title: Localized,
    #[serde(default)]
    pub description: Localized,
    pub category: Category,
    /// What a knowledge pack or a map is about: one or more of [`TOPICS`],
    /// the categories of the Tools screen. The Add-ons screen shows the pack
    /// in the folder of each (a knowledge pack with none it knows goes to
    /// "knowledge"). AI models and programs have none: they have folders of
    /// their own. Catalogs from before this was a list name one `topic`;
    /// [`Catalog::parse`] reads it as its list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub topics: Vec<String>,
    pub version: String,
    pub size: u64,
    pub files: Vec<PackFile>,
    /// The license, as short as its name ("CC BY-SA 4.0").
    #[serde(default)]
    pub license: String,
    /// The credit line its license asks for.
    #[serde(default)]
    pub attribution: String,
    /// Where it comes from: the publisher's site, or the library it is
    /// downloaded from.
    #[serde(default)]
    pub source: String,
    /// How it may be offered: like any add-on, or only as one people
    /// download themselves (see [`Offer`]).
    #[serde(default, deserialize_with = "offer_of")]
    pub offer: Offer,
    /// Why a pack is [`Offer::User`]: "noncommercial" (free for
    /// non-commercial use only) or "mixed_licenses" (each part keeps its own
    /// terms). The app explains it next to the license.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub offer_reason: String,
    #[serde(default)]
    pub languages: Vec<String>,
    /// UI languages for which this pack is recommended ("en", "sr").
    #[serde(default)]
    pub recommended_for: Vec<String>,
}

/// Where a pack stands on this hub. Persisted at `<root>/catalog/state.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PackStatus {
    NotInstalled,
    Queued,
    Downloading,
    Paused,
    Verifying,
    Installed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackState {
    pub status: PackStatus,
    pub bytes_done: u64,
    pub bytes_total: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_version: Option<String>,
    /// Bytes per second over the last few seconds while downloading.
    #[serde(default)]
    pub speed: u64,
    /// The pack's files on disk, as they were verified. The library, phones
    /// and USB copies use these, so an older version keeps working until a
    /// newer one has been verified.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<InstalledFile>,
    /// The catalog now has another version than the one on disk.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub update_available: bool,
    /// Old files and folders (relative to the library) still to be deleted;
    /// they were in use when they were replaced or removed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stale: Vec<String>,
    /// For each partial download (by library path): how many bytes are known
    /// to be on the disk. After a power cut the download continues from there.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub synced: BTreeMap<String, u64>,
    /// The folder a USB import copies from, so a paused import continues from it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import_from: Option<String>,
}

impl PackState {
    pub fn not_installed(total: u64) -> Self {
        Self {
            status: PackStatus::NotInstalled,
            bytes_done: 0,
            bytes_total: total,
            error: None,
            installed_version: None,
            speed: 0,
            files: Vec::new(),
            update_available: false,
            stale: Vec::new(),
            synced: BTreeMap::new(),
            import_from: None,
        }
    }

    /// Nothing worth remembering: not installed, nothing on disk, nothing pending.
    pub fn is_blank(&self) -> bool {
        self.status == PackStatus::NotInstalled
            && self.error.is_none()
            && self.files.is_empty()
            && self.stale.is_empty()
            && self.synced.is_empty()
            && self.import_from.is_none()
    }

    /// The files on disk are exactly what the catalog lists for `pack`.
    pub fn matches(&self, pack: &Pack) -> bool {
        self.installed_version.as_deref() == Some(pack.version.as_str())
            && self.files.len() == pack.files.len()
            && pack.files.iter().all(|f| self.files.iter().any(|i| i.is(f)))
    }
}

/// One verified file of an installed pack: where it is and what it is.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstalledFile {
    pub path: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1_base64: Option<String>,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unpack_to: Option<String>,
}

impl InstalledFile {
    /// Same place and same content as the catalog file `f`.
    pub fn is(&self, f: &PackFile) -> bool {
        self.path == f.path
            && self.size == f.size
            && self.sha256.eq_ignore_ascii_case(&f.sha256)
            && self.sha1_base64 == f.sha1_base64
            && self.unpack_to == f.unpack_to
    }
}

impl From<&PackFile> for InstalledFile {
    fn from(f: &PackFile) -> Self {
        Self { path: f.path.clone(), sha256: f.sha256.clone(), sha1_base64: f.sha1_base64.clone(), size: f.size, unpack_to: f.unpack_to.clone() }
    }
}

/// A path inside the library: relative, only plain names, no "..", no drive.
pub fn is_safe_relative(path: &str) -> bool {
    let p = Path::new(path);
    !path.is_empty()
        && !path.contains('\\')
        && p.components().all(|c| matches!(c, std::path::Component::Normal(_)))
}

impl Pack {
    /// Ids are simple slugs; every path stays inside the library.
    pub fn is_safe(&self) -> bool {
        let slug = |s: &str| !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        // Map pieces are named after their region: "map:Germany_Bavaria".
        let map_name = |s: &str| {
            !s.is_empty() && s.len() <= 160 && !s.contains("..") && !s.contains('/') && !s.contains('\\') && !s.chars().any(char::is_control)
        };
        let id_ok = slug(&self.id) || self.id.strip_prefix(crate::maps::MAP_ID_PREFIX).is_some_and(map_name);
        let hash_ok = |f: &PackFile| {
            (f.sha256.len() == 64 && f.sha256.chars().all(|c| c.is_ascii_hexdigit()))
                || (f.sha256.is_empty() && f.sha1_base64.as_deref().is_some_and(|h| h.len() == 28))
        };
        // A (zip) archive says where it unpacks to; a default folder could wipe
        // other programs. Only archives have such a folder.
        let unpack_ok = |f: &PackFile| match (f.unpack.as_deref(), &f.unpack_to) {
            (Some("zip"), Some(dir)) => is_safe_relative(dir),
            (None, None) => true,
            _ => false,
        };
        id_ok && !self.files.is_empty() && self.files.iter().all(|f| is_safe_relative(&f.path) && unpack_ok(f) && hash_ok(f))
    }
}

impl Catalog {
    /// A catalog from its JSON. A pack from an older catalog, with one
    /// `topic` and no `topics`, gets the topics that old one stands for; a
    /// catalog may carry both, `topic` for older hubs and `topics` for this one.
    pub fn parse(text: &str) -> serde_json::Result<Catalog> {
        let mut v: serde_json::Value = serde_json::from_str(text)?;
        if let Some(packs) = v.get_mut("packs").and_then(|p| p.as_array_mut()) {
            for p in packs.iter_mut().filter_map(|p| p.as_object_mut()) {
                if p.contains_key("topics") {
                    continue;
                }
                if let Some(old) = p.get("topic").and_then(|t| t.as_str()) {
                    let topics = legacy_topics(old).into_iter().map(serde_json::Value::from).collect();
                    p.insert("topics".into(), serde_json::Value::Array(topics));
                }
            }
        }
        serde_json::from_value(v)
    }

    /// The catalog compiled into the binary.
    pub fn bundled() -> Catalog {
        Self::parse(BUNDLED).expect("bundled catalog is valid JSON")
    }

    /// Bundled catalog, overridden by `<catalog_dir>/catalog.json` when that
    /// file exists, parses, and is at least as new. Packs with unsafe ids or
    /// file paths are dropped.
    pub fn load(catalog_dir: &Path) -> Catalog {
        let bundled = Self::bundled();
        let path = catalog_dir.join("catalog.json");
        let mut c = match std::fs::read_to_string(&path).ok().and_then(|t| Self::parse(&t).ok()) {
            Some(c) if c.generated >= bundled.generated => c,
            _ => bundled,
        };
        // The world's maps, split the way CoMaps publishes them, and the CoMaps app.
        let known: std::collections::HashSet<String> = c.packs.iter().map(|p| p.id.clone()).collect();
        c.packs.extend(crate::maps::packs().into_iter().filter(|p| !known.contains(&p.id)));
        c.packs.retain(|p| {
            let ok = p.is_safe();
            if !ok {
                tracing::warn!(pack = %p.id, "catalog entry with an unsafe id or path ignored");
            }
            ok
        });
        c.tidy();
        c
    }

    /// Keep the rules whatever a catalog file says: a withdrawn pack is one
    /// the catalog does not offer (and its paths stay inside the library),
    /// and a starter set holds only packs offered to everyone.
    fn tidy(&mut self) {
        let offered: std::collections::HashSet<String> = self.packs.iter().map(|p| p.id.clone()).collect();
        self.withdrawn.retain(|p| p.is_safe() && !offered.contains(&p.id));
        let auto: std::collections::HashSet<String> = self.packs.iter().filter(|p| p.offer == Offer::Auto).map(|p| p.id.clone()).collect();
        for set in &mut self.starter_sets {
            set.packs.retain(|id| {
                let ok = auto.contains(id);
                if !ok {
                    tracing::warn!(pack = %id, "a starter set may only hold packs offered to everyone; left out");
                }
                ok
            });
        }
    }

    /// A pack the catalog offers.
    pub fn pack(&self, id: &str) -> Option<&Pack> {
        self.packs.iter().find(|p| p.id == id)
    }

    /// A pack the catalog no longer offers, as it was listed.
    pub fn withdrawn_pack(&self, id: &str) -> Option<&Pack> {
        self.withdrawn.iter().find(|p| p.id == id)
    }

    pub fn by_id(&self) -> HashMap<&str, &Pack> {
        self.packs.iter().map(|p| (p.id.as_str(), p)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_catalog_is_sane() {
        let c = Catalog::bundled();
        assert!(c.packs.len() >= 5);
        for p in &c.packs {
            assert!(!p.id.is_empty());
            assert_eq!(p.size, p.files.iter().map(|f| f.size).sum::<u64>(), "size mismatch in {}", p.id);
            for f in &p.files {
                assert!(f.sha256.len() == 64 || f.sha1_base64.is_some(), "no hash in {}", p.id);
                assert!(!f.urls.is_empty());
                assert!(!f.path.starts_with('/') && !f.path.contains(".."));
            }
        }
        assert!(c.pack("kiwix-tools").is_some());
        // Every knowledge pack names what it is about: the Tools screen's
        // categories and the Add-ons folders it shows in. AI models and
        // programs have folders of their own and no topics.
        for p in &c.packs {
            match p.category {
                Category::Knowledge => assert!(!p.topics.is_empty(), "no topics for {}", p.id),
                Category::Model | Category::App => assert!(p.topics.is_empty(), "{} is not about a topic", p.id),
                Category::Maps => {}
            }
            for t in &p.topics {
                assert!(TOPICS.contains(&t.as_str()), "unknown topic {t} for {}", p.id);
            }
            let mut seen = p.topics.clone();
            seen.sort();
            seen.dedup();
            assert_eq!(seen.len(), p.topics.len(), "a topic twice for {}", p.id);
        }
        // A guide about more than one thing is in more than one place.
        assert!(c.packs.iter().any(|p| p.topics.len() > 1));
        for p in &c.packs {
            assert!(p.is_safe(), "unsafe pack {}", p.id);
            for f in &p.files {
                let name = f.path.rsplit('/').next().unwrap();
                let encoded = name.replace(' ', "%20");
                for u in &f.urls {
                    assert!(u.starts_with("https://"), "{u}");
                    // A branch can be re-uploaded under the same name; a commit cannot.
                    assert!(!u.contains("/resolve/main/"), "pin Hugging Face files to a commit: {u}");
                    let file_part = u.rsplit('/').next().unwrap();
                    assert!(file_part == name || file_part.replace("%20", " ") == name || u.ends_with(&encoded) || p.id.starts_with("map:") || p.id == "comaps-app",
                        "mirror URL must point at the file: {u}");
                }
            }
        }
    }

    /// The bundled catalog as written, before any defaults are filled in.
    fn bundled_json() -> serde_json::Value {
        serde_json::from_str(BUNDLED).unwrap()
    }

    #[test]
    fn every_pack_says_what_it_is_and_how_it_may_be_offered() {
        let c = Catalog::bundled();
        let json = bundled_json();
        let raw = json["packs"].as_array().unwrap();
        assert_eq!(raw.len(), c.packs.len());
        for (p, r) in c.packs.iter().zip(raw) {
            // Size, SHA-256 and topics, checked above for every file; here every
            // pack in the file names its size and hash, not only some.
            assert!(p.size > 0, "no size for {}", p.id);
            assert!(p.files.iter().all(|f| f.size > 0 && f.sha256.len() == 64), "size and SHA-256 for every file of {}", p.id);
            // How it may be offered is a decision about its license, made for
            // each pack: never left to the default.
            assert!(matches!(r["offer"].as_str(), Some("auto" | "user")), "{} must say \"offer\": \"auto\" or \"user\"", p.id);
            match p.offer {
                Offer::Auto => assert!(p.offer_reason.is_empty(), "{} is offered to everyone; no reason needed", p.id),
                Offer::User => assert!(
                    ["noncommercial", "mixed_licenses"].contains(&p.offer_reason.as_str()),
                    "{}: a pack people download themselves says why (the app explains the reason)",
                    p.id
                ),
            }
            // What the app shows in a pack's details and the credit its license asks for.
            if p.category == Category::Knowledge {
                assert!(!p.license.is_empty() && !p.attribution.is_empty(), "license and attribution for {}", p.id);
                assert!(p.source.starts_with("https://"), "source for {}", p.id);
                assert!(!p.title.sr.is_empty() && !p.description.en.is_empty() && !p.description.sr.is_empty(), "texts in both languages for {}", p.id);
            }
        }
        // The research of 2026-09-29: iFixit is non-commercial, the safe water
        // guides are a collection of documents under their own terms.
        let pack = |id: &str| c.pack(id).unwrap_or_else(|| panic!("{id} in the catalog"));
        assert_eq!((pack("ifixit-en").offer, pack("ifixit-en").offer_reason.as_str()), (Offer::User, "noncommercial"));
        assert_eq!((pack("zimgit-water-en").offer, pack("zimgit-water-en").offer_reason.as_str()), (Offer::User, "mixed_licenses"));
        assert_eq!(pack("wikimed-en").offer, Offer::Auto);
        // Written back with the field, so older and newer hubs read it the same.
        let back = serde_json::to_value(pack("ifixit-en")).unwrap();
        assert_eq!((back["offer"].as_str(), back["offer_reason"].as_str()), (Some("user"), Some("noncommercial")));
    }

    #[test]
    fn starter_sets_hold_only_packs_offered_to_everyone() {
        let c = Catalog::bundled();
        let langs: Vec<&str> = c.starter_sets.iter().map(|s| s.lang.as_str()).collect();
        assert_eq!(langs, ["sr", "en"], "one starter set per app language");
        for set in &c.starter_sets {
            assert!(!set.packs.is_empty());
            for id in &set.packs {
                let p = c.pack(id).unwrap_or_else(|| panic!("starter set {}: {id} is not in the catalog", set.lang));
                assert_eq!(p.offer, Offer::Auto, "starter set {}: {id} is a pack people download themselves", set.lang);
                assert_eq!(p.category, Category::Knowledge, "starter set {}: the AI model is chosen by the computer, not listed", set.lang);
            }
            let mut ids = set.packs.clone();
            ids.sort();
            ids.dedup();
            assert_eq!(ids.len(), set.packs.len(), "starter set {}: a pack twice", set.lang);
            // A sensible size: a household downloads it in one go.
            let bytes: u64 = set.packs.iter().map(|id| c.pack(id).unwrap().size).sum();
            assert!(bytes < 25_000_000_000, "starter set {} is {bytes} bytes", set.lang);
        }
        assert_eq!(c.starter_sets[0].map.as_deref(), Some("Serbia"));
        // Loading keeps them as the file has them: nothing had to be left out.
        let loaded = Catalog::load(&std::env::temp_dir().join(format!("zaklon-catalog-none-{}", std::process::id())));
        assert_eq!(loaded.starter_sets, c.starter_sets);
    }

    #[test]
    fn a_catalog_file_cannot_put_other_packs_in_a_starter_set() {
        let dir = std::env::temp_dir().join(format!("zaklon-catalog-starter-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = |id: &str| format!(r#"[{{"path":"zim/{id}.zim","urls":["https://example.org/{id}.zim"],"sha256":"{}","size":1}}]"#, "0".repeat(64));
        let pack = |id: &str, extra: &str| {
            format!(r#"{{"id":"{id}","title":{{"en":"{id}"}},"category":"knowledge","topics":["build"],"version":"1","size":1{extra},"files":{}}}"#, file(id))
        };
        let text = format!(
            r#"{{"version":1,"generated":"2999-01-01",
                "starter_sets":[{{"lang":"en","packs":["free","nc","odd","gone","nowhere"]}}],
                "packs":[{},{},{}],
                "withdrawn":[{},{}]}}"#,
            pack("free", r#","offer":"auto""#),
            pack("nc", r#","offer":"user","offer_reason":"noncommercial""#),
            // A kind of offer this version does not know yet: the careful one.
            pack("odd", r#","offer":"sponsored""#),
            pack("gone", ""),
            // Listed as offered too: it is offered.
            pack("free", ""),
        );
        std::fs::write(dir.join("catalog.json"), text).unwrap();
        let c = Catalog::load(&dir);
        assert_eq!(c.pack("free").unwrap().offer, Offer::Auto);
        assert_eq!(c.pack("nc").unwrap().offer, Offer::User);
        assert_eq!(c.pack("odd").unwrap().offer, Offer::User);
        assert_eq!(c.starter_sets[0].packs, ["free"], "only packs offered to everyone");
        assert!(c.withdrawn_pack("gone").is_some() && c.pack("gone").is_none());
        assert!(c.withdrawn_pack("free").is_none(), "a pack is offered or withdrawn, not both");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn withdrawn_packs_are_known_by_name_but_never_downloaded() {
        let c = Catalog::bundled();
        // The research of 2026-09-29: commercially published books and
        // Hesperian titles, which need permission for any digital use.
        for id in ["zimgit-medicine-en", "zimgit-food-preparation-en"] {
            assert!(c.pack(id).is_none(), "{id} is not offered");
            let p = c.withdrawn_pack(id).unwrap_or_else(|| panic!("{id} is known by name"));
            assert!(!p.title.en.is_empty() && !p.title.sr.is_empty() && !p.topics.is_empty());
            assert!(p.is_safe());
        }
        for p in &c.withdrawn {
            assert!(p.files.iter().all(|f| f.urls.is_empty()), "{} has nowhere to be downloaded from", p.id);
            assert!(c.starter_sets.iter().all(|s| !s.packs.contains(&p.id)));
        }
    }

    #[test]
    fn maps_are_about_maps() {
        let dir = std::env::temp_dir().join(format!("zaklon-catalog-maps-{}", std::process::id()));
        let c = Catalog::load(&dir);
        let maps: Vec<&Pack> = c.packs.iter().filter(|p| p.category == Category::Maps).collect();
        assert!(!maps.is_empty());
        assert!(maps.iter().all(|p| p.topics == ["maps"]));
        assert!(c.pack(crate::maps::COMAPS_APK_ID).unwrap().topics.is_empty(), "the map app is a program");
    }

    #[test]
    fn topics_of_older_catalogs_are_read() {
        let pack = |extra: &str| {
            format!(
                r#"{{"id":"p","title":{{"en":"P"}},"category":"knowledge"{extra},"version":"1","size":1,
                   "files":[{{"path":"zim/p.zim","urls":["https://example.org/p.zim"],"sha256":"{}","size":1}}]}}"#,
                "0".repeat(64)
            )
        };
        let topics = |extra: &str| {
            let text = format!(r#"{{"version":1,"generated":"2026-01-01","packs":[{}]}}"#, pack(extra));
            Catalog::parse(&text).unwrap().packs[0].topics.clone()
        };
        // The old folders, as the topics they stand for.
        assert_eq!(topics(r#","topic":"reference""#), ["knowledge"]);
        assert_eq!(topics(r#","topic":"health""#), ["health"]);
        assert_eq!(topics(r#","topic":"garden""#), ["garden", "food"]);
        assert_eq!(topics(r#","topic":"skills""#), ["build"]);
        assert_eq!(topics(r#","topic":"water""#), ["water"]);
        assert!(topics(r#","topic":"""#).is_empty());
        assert!(topics("").is_empty());
        // The list wins when a catalog has both (the old field for older hubs).
        assert_eq!(topics(r#","topic":"reference","topics":["water","health"]"#), ["water", "health"]);
        // Written back as the list only.
        let text = format!(r#"{{"version":1,"generated":"2026-01-01","packs":[{}]}}"#, pack(r#","topic":"skills""#));
        let json = serde_json::to_value(Catalog::parse(&text).unwrap()).unwrap();
        assert_eq!(json["packs"][0]["topics"], serde_json::json!(["build"]));
        assert!(json["packs"][0].get("topic").is_none());
        assert!(Catalog::parse("not json").is_err());
    }

    #[test]
    fn installed_files_are_compared_with_the_catalog() {
        let c = Catalog::bundled();
        let pack = c.pack("kiwix-tools").unwrap().clone();
        let mut st = PackState::not_installed(pack.size);
        assert!(st.is_blank());
        assert!(!st.matches(&pack), "nothing on disk");
        st.installed_version = Some(pack.version.clone());
        st.files = pack.files.iter().map(InstalledFile::from).collect();
        assert!(st.matches(&pack));
        assert!(!st.is_blank());
        // A new version with a new file name.
        let mut newer = pack.clone();
        newer.version = "9.9.9".into();
        newer.files[0].path = "bin/kiwix-tools-9.9.9.zip".into();
        assert!(!st.matches(&newer));
        // The same file name republished with other content.
        let mut republished = pack.clone();
        republished.files[0].sha256 = "0".repeat(64);
        assert!(!st.matches(&republished));
        // Old state files (without these fields) still load.
        let old: PackState = serde_json::from_str(r#"{"status":"installed","bytes_done":1,"bytes_total":1,"installed_version":"1"}"#).unwrap();
        assert!(old.files.is_empty() && !old.update_available && old.synced.is_empty());
    }

    #[test]
    fn archives_must_name_their_folder() {
        let c = Catalog::bundled();
        let mut pack = c.pack("kiwix-tools").unwrap().clone();
        assert!(pack.is_safe());
        pack.files[0].unpack_to = None;
        assert!(!pack.is_safe(), "unpacking into a default folder could wipe other programs");
        pack.files[0].unpack = None;
        assert!(pack.is_safe(), "a plain file");
        pack.files[0].unpack_to = Some("bin/x".into());
        assert!(!pack.is_safe(), "a folder only for archives");
    }

    #[test]
    fn rejects_unsafe_paths() {
        assert!(is_safe_relative("zim/a.zim"));
        assert!(!is_safe_relative("../a.zim"));
        assert!(!is_safe_relative("/etc/passwd"));
        assert!(!is_safe_relative("C:/Windows/x"));
        assert!(!is_safe_relative("zim\\..\\x"));
        assert!(!is_safe_relative(""));
    }
}
