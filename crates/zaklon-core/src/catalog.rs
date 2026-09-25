//! The add-on catalog: what can be downloaded, from where, and how to verify
//! it. A default catalog is bundled into the binary; a newer one can be placed
//! at `<root>/catalog/catalog.json` (fetched later from the project's signed
//! catalog endpoint or imported from USB).

use std::collections::HashMap;
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
    /// Lower-case hex SHA-256 of the complete file.
    pub sha256: String,
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
}

impl PackState {
    pub fn not_installed(total: u64) -> Self {
        Self { status: PackStatus::NotInstalled, bytes_done: 0, bytes_total: total, error: None, installed_version: None, speed: 0 }
    }
}

impl Catalog {
    /// The catalog compiled into the binary.
    pub fn bundled() -> Catalog {
        serde_json::from_str(BUNDLED).expect("bundled catalog is valid JSON")
    }

    /// Bundled catalog, overridden by `<catalog_dir>/catalog.json` when that
    /// file exists, parses, and is at least as new.
    pub fn load(catalog_dir: &Path) -> Catalog {
        let bundled = Self::bundled();
        let path = catalog_dir.join("catalog.json");
        match std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Catalog>(&t).ok()) {
            Some(c) if c.generated >= bundled.generated => c,
            _ => bundled,
        }
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
                assert_eq!(f.sha256.len(), 64, "bad sha256 in {}", p.id);
                assert!(!f.urls.is_empty());
                assert!(!f.path.starts_with('/') && !f.path.contains(".."));
            }
        }
        assert!(c.pack("kiwix-tools").is_some());
    }
}
