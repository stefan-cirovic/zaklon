use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
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
///
/// Every field except `hub_id` has a default, so a file written by an older
/// or a newer Zaklon (or restored from a backup) still loads. A new field
/// must always get a default too.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Root data folder chosen at install time (e.g. `D:\Zaklon`). Always
    /// replaced on load by the folder the file was actually found in.
    #[serde(default)]
    pub root: PathBuf,
    /// TCP port the hub listens on for phones (TLS).
    #[serde(default = "default_port")]
    pub port: u16,
    /// Loopback port for the desktop window.
    #[serde(default = "default_local_port")]
    pub local_port: u16,
    /// Plain-HTTP port for the "install the app" page.
    #[serde(default = "default_install_port")]
    pub install_port: u16,
    /// UDP port for the discovery beacon.
    #[serde(default = "default_beacon_port")]
    pub beacon_port: u16,
    /// Ports as stored on disk, so env overrides are never written back.
    #[serde(skip)]
    disk_ports: Option<[u16; 4]>,
    /// Human-readable hub name shown to phones.
    #[serde(default = "default_hub_name")]
    pub hub_name: String,
    /// Stable hub identifier, generated once.
    pub hub_id: String,
    /// UI language ("en" or "sr").
    #[serde(default = "default_language")]
    pub language: String,
    /// Whether to check for app updates once a day (on by default; switched in Household).
    #[serde(default = "default_true")]
    pub auto_update_check: bool,
}

/// Write a file so that a crash or power cut leaves either the old or the new
/// content, never half of it: write a temporary file, flush it to disk, rename.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

fn default_port() -> u16 {
    DEFAULT_PORT
}
fn default_language() -> String {
    "en".to_string()
}
fn default_true() -> bool {
    true
}
fn default_local_port() -> u16 {
    LOCAL_PORT
}
fn default_install_port() -> u16 {
    INSTALL_PORT
}
fn default_beacon_port() -> u16 {
    BEACON_PORT
}

/// `ZAKLON_LOOPBACK_ONLY=1`: listen on 127.0.0.1 only and skip DNS-SD, for
/// tests and development. Nothing then listens on the network, so Windows
/// does not ask about its firewall for every freshly built test program.
/// Never saved; a real hub always listens for phones.
pub fn loopback_only() -> bool {
    std::env::var("ZAKLON_LOOPBACK_ONLY").is_ok_and(|v| v.trim() == "1")
}

/// `ZAKLON_TLS_PORT`, `ZAKLON_LOCAL_PORT`, `ZAKLON_INSTALL_PORT` and
/// `ZAKLON_BEACON_PORT` override the ports for this run only (used by tests
/// and for running a second hub on one machine). They are never saved.
fn env_port(name: &str) -> Option<u16> {
    std::env::var(name).ok().and_then(|v| v.trim().parse().ok())
}

impl Config {
    fn apply_env(&mut self) {
        self.disk_ports = Some([self.port, self.local_port, self.install_port, self.beacon_port]);
        if let Some(p) = env_port("ZAKLON_TLS_PORT") {
            self.port = p;
        }
        if let Some(p) = env_port("ZAKLON_LOCAL_PORT") {
            self.local_port = p;
        }
        if let Some(p) = env_port("ZAKLON_INSTALL_PORT") {
            self.install_port = p;
        }
        if let Some(p) = env_port("ZAKLON_BEACON_PORT") {
            self.beacon_port = p;
        }
    }

    pub fn household_dir(&self) -> PathBuf { self.root.join("household") }
    pub fn library_dir(&self) -> PathBuf { self.root.join("library") }
    pub fn catalog_dir(&self) -> PathBuf { self.root.join("catalog") }
    pub fn backups_dir(&self) -> PathBuf { self.root.join("backups") }
    pub fn logs_dir(&self) -> PathBuf { self.root.join("logs") }
    pub fn db_path(&self) -> PathBuf { self.household_dir().join("household.db") }
    pub fn config_path(&self) -> PathBuf { self.household_dir().join("hub.json") }
    pub fn tls_dir(&self) -> PathBuf { self.household_dir().join("tls") }

    /// Read the text of a hub.json. Missing fields get their defaults; only
    /// the hub's identifier is required. A byte-order mark (left by some
    /// Windows editors) is ignored.
    pub fn from_json(text: &str) -> Result<Self> {
        let cfg: Config = serde_json::from_str(text.trim_start_matches('\u{feff}')).context("parsing hub.json")?;
        if cfg.hub_id.trim().is_empty() {
            bail!("hub.json has no hub id");
        }
        Ok(cfg)
    }

    /// Load the config from `<root>/household/hub.json`, or create a fresh one
    /// (and the folder layout) if this is the first run.
    pub fn load_or_init(root: &Path) -> Result<Self> {
        let path = root.join("household").join("hub.json");
        if path.exists() {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            let mut cfg = Self::from_json(&text)?;
            cfg.root = root.to_path_buf();
            cfg.apply_env();
            return Ok(cfg);
        }
        let mut cfg = Config {
            root: root.to_path_buf(),
            port: DEFAULT_PORT,
            local_port: LOCAL_PORT,
            install_port: INSTALL_PORT,
            beacon_port: BEACON_PORT,
            disk_ports: None,
            hub_name: default_hub_name(),
            hub_id: uuid::Uuid::new_v4().to_string(),
            language: default_language(),
            auto_update_check: true,
        };
        cfg.ensure_layout()?;
        cfg.save()?;
        cfg.apply_env();
        Ok(cfg)
    }

    pub fn ensure_layout(&self) -> Result<()> {
        for dir in [
            self.household_dir(),
            self.library_dir().join("zim"),
            self.library_dir().join("maps"),
            self.library_dir().join("models"),
            self.library_dir().join("apk"),
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
        // Never persist ports that came from environment overrides.
        let mut on_disk = self.clone();
        if let Some([tls, local, install, beacon]) = self.disk_ports {
            if env_port("ZAKLON_TLS_PORT").is_some() {
                on_disk.port = tls;
            }
            if env_port("ZAKLON_LOCAL_PORT").is_some() {
                on_disk.local_port = local;
            }
            if env_port("ZAKLON_INSTALL_PORT").is_some() {
                on_disk.install_port = install;
            }
            if env_port("ZAKLON_BEACON_PORT").is_some() {
                on_disk.beacon_port = beacon;
            }
        }
        let text = serde_json::to_string_pretty(&on_disk)?;
        write_atomic(&self.config_path(), text.as_bytes()).context("writing hub.json")
    }
}

fn default_hub_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .map(|h| format!("Zaklon on {h}"))
        .unwrap_or_else(|_| "Zaklon".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> PathBuf {
        std::env::temp_dir().join(format!("zaklon-config-{}", uuid::Uuid::new_v4()))
    }

    fn load_file(json: &str) -> Result<Config> {
        let root = temp_root();
        std::fs::create_dir_all(root.join("household")).unwrap();
        std::fs::write(root.join("household").join("hub.json"), json).unwrap();
        let cfg = Config::load_or_init(&root);
        let _ = std::fs::remove_dir_all(&root);
        cfg
    }

    #[test]
    fn files_from_other_versions_load() {
        // What 0.1.0 writes.
        let current = r#"{
  "root": "D:\\Zaklon",
  "port": 9000,
  "local_port": 8481,
  "install_port": 8480,
  "beacon_port": 8485,
  "hub_name": "Zaklon on KUCA",
  "hub_id": "7b0f7c1e-5b39-4c43-9d57-3f1f3c1b2a10",
  "language": "sr",
  "auto_update_check": false
}"#;
        let cfg = load_file(current).unwrap();
        assert_eq!((cfg.port, cfg.hub_name.as_str(), cfg.language.as_str()), (9000, "Zaklon on KUCA", "sr"));
        assert!(!cfg.auto_update_check);

        // The oldest shape: no language, update setting or extra ports yet.
        let cfg = load_file(r#"{"root": "C:\\old", "port": 8484, "hub_name": "Zaklon", "hub_id": "abc"}"#).unwrap();
        assert_eq!(cfg.language, "en");
        assert!(cfg.auto_update_check);
        assert_eq!((cfg.local_port, cfg.install_port, cfg.beacon_port), (LOCAL_PORT, INSTALL_PORT, BEACON_PORT));

        // Only the identity; and a newer file with fields this version does not know.
        let cfg = load_file(r#"{"hub_id": "abc"}"#).unwrap();
        assert_eq!(cfg.port, DEFAULT_PORT);
        assert!(!cfg.hub_name.is_empty());
        assert!(load_file(r#"{"hub_id": "abc", "theme": "dark", "later": {"a": 1}}"#).is_ok());
        // Saved by hand in an editor that adds a byte-order mark.
        assert_eq!(load_file("\u{feff}{\"hub_id\": \"abc\"}").unwrap().hub_id, "abc");
    }

    #[test]
    fn a_wrong_file_is_refused() {
        for bad in ["[]", "{}", r#"{"hub_id": " "}"#, r#"{"hub_id": "abc", "port": "8484"}"#, r#"{"hub_id": 5}"#, "not json"] {
            assert!(Config::from_json(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_folder_it_is_found_in_wins() {
        let root = temp_root();
        let first = Config::load_or_init(&root).unwrap();
        assert_eq!(first.root, root);
        std::fs::write(first.config_path(), r#"{"root": "Z:\\elsewhere", "hub_id": "abc"}"#).unwrap();
        let again = Config::load_or_init(&root).unwrap();
        assert_eq!((again.root.as_path(), again.hub_id.as_str()), (root.as_path(), "abc"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
