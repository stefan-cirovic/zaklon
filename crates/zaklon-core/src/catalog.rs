//! The add-on catalog: what can be downloaded, from where, and how to verify
//! it. A default catalog is bundled into the binary; a newer one can be placed
//! at `<root>/catalog/catalog.json` (fetched later from the project's signed
//! catalog endpoint or imported from USB).

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde::{Deserialize, Serialize};

const BUNDLED: &str = include_str!("../catalog/default-catalog.json");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    pub version: u32,
    pub generated: String,
    pub packs: Vec<Pack>,
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
    /// Download locations, tried in order.
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
    /// What a knowledge pack is about; the Add-ons screen shows it in the
    /// folder of that name: "reference" (encyclopedias, dictionaries, books),
    /// "health", "garden" (garden and food) or "skills" (repair and know-how).
    /// Empty or unknown: the "Other knowledge" folder.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub topic: String,
    pub version: String,
    pub size: u64,
    pub files: Vec<PackFile>,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub attribution: String,
    #[serde(default)]
    pub source: String,
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
    /// The catalog compiled into the binary.
    pub fn bundled() -> Catalog {
        serde_json::from_str(BUNDLED).expect("bundled catalog is valid JSON")
    }

    /// Bundled catalog, overridden by `<catalog_dir>/catalog.json` when that
    /// file exists, parses, and is at least as new. Packs with unsafe ids or
    /// file paths are dropped.
    pub fn load(catalog_dir: &Path) -> Catalog {
        let bundled = Self::bundled();
        let path = catalog_dir.join("catalog.json");
        let mut c = match std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Catalog>(&t).ok()) {
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
        c
    }

    pub fn pack(&self, id: &str) -> Option<&Pack> {
        self.packs.iter().find(|p| p.id == id)
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
        // Every knowledge pack names the folder the Add-ons screen shows it in.
        for p in c.packs.iter().filter(|p| p.category == Category::Knowledge) {
            assert!(["reference", "health", "garden", "skills"].contains(&p.topic.as_str()), "no known topic for {}", p.id);
        }
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
