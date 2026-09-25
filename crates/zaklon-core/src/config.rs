use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Default TLS port for hub <-> app traffic.
pub const DEFAULT_PORT: u16 = 8484;
/// UDP port for the discovery beacon.
pub const BEACON_PORT: u16 = 8485;
/// Plain-HTTP port bound to 127.0.0.1 only, used by the desktop window.
pub const LOCAL_PORT: u16 = 8481;
/// Plain-HTTP port on all interfaces that serves only the "install the app" page and APKs.
pub const INSTALL_PORT: u16 = 8480;

/// Persistent hub configuration. Lives at `<root>/household/hub.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Root data folder chosen at install time (e.g. `D:\Zaklon`).
    pub root: PathBuf,
    /// TCP port the hub listens on.
    pub port: u16,
    /// Human-readable hub name shown to phones.
    pub hub_name: String,
    /// Stable hub identifier, generated once.
    pub hub_id: String,
    /// UI language ("en" or "sr").
    pub language: String,
    /// Whether to check for app updates automatically (asked at install).
    pub auto_update_check: bool,
}

impl Config {
    pub fn household_dir(&self) -> PathBuf { self.root.join("household") }
    pub fn library_dir(&self) -> PathBuf { self.root.join("library") }
    pub fn profiles_dir(&self) -> PathBuf { self.root.join("profiles") }
    pub fn catalog_dir(&self) -> PathBuf { self.root.join("catalog") }
    pub fn backups_dir(&self) -> PathBuf { self.root.join("backups") }
    pub fn logs_dir(&self) -> PathBuf { self.root.join("logs") }
    pub fn db_path(&self) -> PathBuf { self.household_dir().join("household.db") }
    pub fn config_path(&self) -> PathBuf { self.household_dir().join("hub.json") }
    pub fn tls_dir(&self) -> PathBuf { self.household_dir().join("tls") }

    /// Load the config from `<root>/household/hub.json`, or create a fresh one
    /// (and the folder layout) if this is the first run.
    pub fn load_or_init(root: &Path) -> Result<Self> {
        let path = root.join("household").join("hub.json");
        if path.exists() {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            let mut cfg: Config = serde_json::from_str(&text).context("parsing hub.json")?;
            cfg.root = root.to_path_buf();
            return Ok(cfg);
        }
        let cfg = Config {
            root: root.to_path_buf(),
            port: DEFAULT_PORT,
            hub_name: default_hub_name(),
            hub_id: uuid::Uuid::new_v4().to_string(),
            language: "en".to_string(),
            auto_update_check: true,
        };
        cfg.ensure_layout()?;
        cfg.save()?;
        Ok(cfg)
    }

    pub fn ensure_layout(&self) -> Result<()> {
        for dir in [
            self.household_dir(),
            self.library_dir().join("zim"),
            self.library_dir().join("maps"),
            self.library_dir().join("models"),
            self.library_dir().join("apk"),
            self.profiles_dir(),
            self.catalog_dir(),
            self.backups_dir(),
            self.logs_dir(),
            self.tls_dir(),
        ] {
            std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        Ok(())
    }

    pub fn save(&self) -> Result<()> {
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(self.config_path(), text).context("writing hub.json")
    }
}

fn default_hub_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .map(|h| format!("Zaklon on {h}"))
        .unwrap_or_else(|_| "Zaklon".to_string())
}
