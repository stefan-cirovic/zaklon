//! Backups of the household's own data: supplies and their history, paired
//! devices, settings, the hub's identity. The library, maps and AI models are
//! not included: they are large and come back by downloading or from USB.
//!
//! A backup is one zip file. The hub makes one a day by itself (keeping the
//! last seven) and one on request into any folder, e.g. a USB stick.
//!
//! Restoring never touches the open database: the backup is checked and
//! unpacked into `restore-pending/`, and the next start swaps it in, keeping
//! the replaced data under `backups/` first.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use zaklon_core::{Config, Db};

const FORMAT: u32 = 1;
const AUTO_PREFIX: &str = "zaklon-auto-";
const KEEP_AUTO: usize = 7;
const PENDING: &str = "restore-pending";
/// Largest file accepted inside a backup (the database of a very busy household).
const MAX_ENTRY: u64 = 2 << 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    pub app_version: String,
    /// RFC 3339.
    pub created: String,
    pub hub_id: String,
    pub hub_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupFile {
    pub path: String,
    pub name: String,
    pub size: u64,
    /// RFC 3339.
    pub created: String,
    pub automatic: bool,
}

pub fn now_rfc3339() -> String {
    let secs = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (y, m, d, hh, mm, ss) = civil(secs);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// UTC civil time from Unix seconds.
fn civil(secs: u64) -> (i64, i64, i64, u64, u64, u64) {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d, rem / 3600, rem % 3600 / 60, rem % 60)
}

fn stamp() -> String {
    let secs = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (y, m, d, hh, mm, ss) = civil(secs);
    format!("{y:04}-{m:02}-{d:02}-{hh:02}{mm:02}{ss:02}")
}

/// Write a backup zip into `dir`; returns its path.
pub fn create(cfg: &Config, db: &Db, dir: &Path, automatic: bool) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    let name = if automatic { format!("{AUTO_PREFIX}{}.zip", stamp()) } else { format!("zaklon-backup-{}.zip", stamp()) };
    let target = dir.join(&name);
    let part = dir.join(format!("{name}.part"));

    // A consistent copy of the database, even while it is in use.
    let tmp_db = std::env::temp_dir().join(format!("zaklon-backup-{}.db", uuid::Uuid::new_v4()));
    db.snapshot_to(&tmp_db).map_err(|e| format!("copying the database: {e}"))?;
    let result = (|| -> Result<(), String> {
        let file = std::fs::File::create(&part).map_err(|e| format!("writing the backup: {e}"))?;
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let manifest = Manifest {
            format: FORMAT,
            app_version: env!("CARGO_PKG_VERSION").into(),
            created: now_rfc3339(),
            hub_id: cfg.hub_id.clone(),
            hub_name: cfg.hub_name.clone(),
        };
        let mut add = |name: &str, bytes: &[u8]| -> Result<(), String> {
            zip.start_file(name, opts).map_err(|e| e.to_string())?;
            zip.write_all(bytes).map_err(|e| e.to_string())
        };
        add("manifest.json", &serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?)?;
        add("household.db", &std::fs::read(&tmp_db).map_err(|e| e.to_string())?)?;
        add("hub.json", &std::fs::read(cfg.config_path()).map_err(|e| e.to_string())?)?;
        if let Ok(rd) = std::fs::read_dir(cfg.tls_dir()) {
            for e in rd.flatten() {
                if e.path().is_file() {
                    let n = e.file_name().to_string_lossy().to_string();
                    add(&format!("tls/{n}"), &std::fs::read(e.path()).map_err(|e| e.to_string())?)?;
                }
            }
        }
        let file = zip.finish().map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| format!("writing the backup: {e}"))?;
        Ok(())
    })();
    let _ = std::fs::remove_file(&tmp_db);
    if let Err(e) = result {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    std::fs::rename(&part, &target).map_err(|e| format!("finishing the backup: {e}"))?;
    info!(path = %target.display(), "backup written");
    Ok(target)
}

/// The automatic backups and those saved into the backups folder, newest first.
pub fn list(cfg: &Config) -> Vec<BackupFile> {
    let mut out: Vec<BackupFile> = std::fs::read_dir(cfg.backups_dir())
        .map(|rd| {
            rd.flatten()
                .filter(|e| {
                    let n = e.file_name().to_string_lossy().to_string();
                    n.ends_with(".zip") && (n.starts_with(AUTO_PREFIX) || n.starts_with("zaklon-backup-"))
                })
                .filter_map(|e| {
                    let meta = e.metadata().ok()?;
                    let name = e.file_name().to_string_lossy().to_string();
                    let created = read_manifest(&e.path()).map(|m| m.created).unwrap_or_default();
                    Some(BackupFile { path: e.path().display().to_string(), automatic: name.starts_with(AUTO_PREFIX), name, size: meta.len(), created })
                })
                .collect()
        })
        .unwrap_or_default();
    // Newest first by the time inside each backup (names differ between
    // daily and hand-made ones, so they cannot be compared).
    out.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| b.name.cmp(&a.name)));
    out
}

/// A daily backup, if the newest automatic one is older than a day. Keeps the last seven.
pub fn auto_backup_if_due(cfg: &Config, db: &Db) {
    let dir = cfg.backups_dir();
    let newest = std::fs::read_dir(&dir)
        .ok()
        .into_iter()
        .flat_map(|rd| rd.flatten())
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            n.starts_with(AUTO_PREFIX) && n.ends_with(".zip")
        })
        .filter_map(|e| e.metadata().ok()?.modified().ok())
        .max();
    let due = newest.is_none_or(|t| t.elapsed().unwrap_or_default() >= Duration::from_secs(24 * 3600));
    if !due {
        return;
    }
    if let Err(e) = create(cfg, db, &dir, true) {
        warn!("automatic backup failed: {e}");
        return;
    }
    let mut autos: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(AUTO_PREFIX) && n.to_string_lossy().ends_with(".zip")))
                .collect()
        })
        .unwrap_or_default();
    autos.sort();
    while autos.len() > KEEP_AUTO {
        let old = autos.remove(0);
        let _ = std::fs::remove_file(old);
    }
}

pub fn read_manifest(path: &Path) -> Option<Manifest> {
    let file = std::fs::File::open(path).ok()?;
    let mut zip = zip::ZipArchive::new(file).ok()?;
    let mut entry = zip.by_name("manifest.json").ok()?;
    let mut text = String::new();
    entry.by_ref().take(64 * 1024).read_to_string(&mut text).ok()?;
    serde_json::from_str(&text).ok()
}

/// Check a backup and unpack it next to the data, ready for the next start.
pub fn stage_restore(cfg: &Config, zip_path: &Path) -> Result<Manifest, String> {
    let manifest = read_manifest(zip_path).ok_or("this is not a Zaklon backup")?;
    if manifest.format > FORMAT {
        return Err("this backup was made by a newer Zaklon; update first".into());
    }
    // Unpack next to the real pending folder and swap it in only when
    // everything is written and checked: a power cut halfway leaves nothing
    // half-done, and a failed second attempt keeps the first one waiting.
    let pending = cfg.root.join(format!("{PENDING}.tmp"));
    let _ = std::fs::remove_dir_all(&pending);
    std::fs::create_dir_all(pending.join("tls")).map_err(|e| e.to_string())?;
    let result = (|| -> Result<(), String> {
        let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
        let mut zip = zip::ZipArchive::new(file).map_err(|_| "this is not a Zaklon backup".to_string())?;
        let mut have_db = false;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
            let name = entry.name().to_string();
            // Only the files a backup is made of; nothing else is written anywhere.
            let allowed = matches!(name.as_str(), "manifest.json" | "household.db" | "hub.json")
                || name.strip_prefix("tls/").is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)));
            if !allowed || entry.size() > MAX_ENTRY {
                continue;
            }
            let mut out = std::fs::File::create(pending.join(&name)).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry.by_ref().take(MAX_ENTRY), &mut out).map_err(|e| e.to_string())?;
            have_db |= name == "household.db";
        }
        if !have_db || !pending.join("hub.json").is_file() {
            return Err("the backup is incomplete".into());
        }
        // It must open as a household database.
        Db::open(&pending.join("household.db")).map_err(|_| "the backup's database is damaged".to_string())?;
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(pending.join("hub.json")).map_err(|e| e.to_string())?)
            .map_err(|_| "the backup's settings are damaged".to_string())?;
        Ok(())
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&pending);
        return Err(e);
    }
    let ready = cfg.root.join(PENDING);
    let _ = std::fs::remove_dir_all(&ready);
    std::fs::rename(&pending, &ready).map_err(|e| format!("preparing the restore: {e}"))?;
    info!(from = %zip_path.display(), "restore staged; it completes on the next start");
    Ok(manifest)
}

pub fn restore_pending(root: &Path) -> bool {
    root.join(PENDING).join("manifest.json").is_file()
}

/// On start, before the database is opened: swap in a staged restore,
/// keeping the replaced household folder under `backups/`. The new folder is
/// complete before anything is moved, and a failed swap is rolled back, so
/// the household always has either its old data or the restored data.
pub fn finish_pending_restore(root: &Path) -> Result<bool, String> {
    let pending = root.join(PENDING);
    if !restore_pending(root) {
        return Ok(false);
    }
    if !pending.join("household.db").is_file() || !pending.join("hub.json").is_file() {
        let _ = std::fs::remove_dir_all(&pending);
        return Err("the prepared restore was incomplete and was discarded".into());
    }
    let household = root.join("household");
    let fresh = root.join("household.new");
    let _ = std::fs::remove_dir_all(&fresh);
    std::fs::create_dir_all(fresh.join("tls")).map_err(|e| e.to_string())?;
    for name in ["household.db", "hub.json"] {
        std::fs::rename(pending.join(name), fresh.join(name)).map_err(|e| format!("restoring {name}: {e}"))?;
    }
    if let Ok(rd) = std::fs::read_dir(pending.join("tls")) {
        for e in rd.flatten() {
            std::fs::rename(e.path(), fresh.join("tls").join(e.file_name())).map_err(|e| format!("restoring the identity: {e}"))?;
        }
    }
    std::fs::create_dir_all(root.join("backups")).map_err(|e| e.to_string())?;
    let keep = root.join("backups").join(format!("household-before-restore-{}", stamp()));
    if household.exists() {
        std::fs::rename(&household, &keep).map_err(|e| format!("setting the old data aside: {e}"))?;
    }
    if let Err(e) = std::fs::rename(&fresh, &household) {
        // Put the old data back rather than start with nothing.
        if keep.exists() {
            let _ = std::fs::rename(&keep, &household);
        }
        return Err(format!("swapping in the restored data: {e}"));
    }
    // Downloads and the library stay as they are on this computer.
    let _ = std::fs::remove_dir_all(&pending);
    info!(old = %keep.display(), "restore finished");
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_and_restore_round_trip() {
        let root = std::env::temp_dir().join(format!("zaklon-backup-test-{}", uuid::Uuid::new_v4()));
        let cfg = Config::load_or_init(&root).unwrap();
        let db = Db::open(&cfg.db_path()).unwrap();
        db.set_setting("marker", "before").unwrap();
        std::fs::write(cfg.tls_dir().join("cert.pem"), b"CERT").unwrap();

        let zip = create(&cfg, &db, &cfg.backups_dir(), false).unwrap();
        assert!(zip.file_name().unwrap().to_string_lossy().starts_with("zaklon-backup-"));
        assert_eq!(read_manifest(&zip).unwrap().hub_id, cfg.hub_id);
        assert_eq!(list(&cfg).len(), 1);

        db.set_setting("marker", "after").unwrap();
        drop(db);
        stage_restore(&cfg, &zip).unwrap();
        assert!(restore_pending(&root));
        assert!(finish_pending_restore(&root).unwrap());
        assert!(!restore_pending(&root));
        let db = Db::open(&cfg.db_path()).unwrap();
        assert_eq!(db.get_setting("marker").unwrap().as_deref(), Some("before"));
        assert_eq!(std::fs::read(cfg.tls_dir().join("cert.pem")).unwrap(), b"CERT");
        // The replaced data was kept.
        assert!(std::fs::read_dir(cfg.backups_dir()).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with("household-before-restore-")));
        drop(db);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_incomplete_restore_leaves_the_data_alone() {
        let root = std::env::temp_dir().join(format!("zaklon-backup-test-{}", uuid::Uuid::new_v4()));
        let cfg = Config::load_or_init(&root).unwrap();
        let db = Db::open(&cfg.db_path()).unwrap();
        db.set_setting("marker", "current").unwrap();
        drop(db);
        // A pending restore that lost its settings file (e.g. a power cut).
        let pending = root.join(PENDING);
        std::fs::create_dir_all(&pending).unwrap();
        std::fs::write(pending.join("manifest.json"), b"{}").unwrap();
        std::fs::write(pending.join("household.db"), b"half").unwrap();
        assert!(finish_pending_restore(&root).is_err());
        assert!(!restore_pending(&root), "discarded");
        let db = Db::open(&cfg.db_path()).unwrap();
        assert_eq!(db.get_setting("marker").unwrap().as_deref(), Some("current"));
        drop(db);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn not_a_backup_is_refused() {
        let root = std::env::temp_dir().join(format!("zaklon-backup-test-{}", uuid::Uuid::new_v4()));
        let cfg = Config::load_or_init(&root).unwrap();
        let bogus = root.join("bogus.zip");
        std::fs::write(&bogus, b"not a zip").unwrap();
        assert!(stage_restore(&cfg, &bogus).is_err());
        assert!(!restore_pending(&root));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A failed final swap (the restored folder cannot be moved into place)
    /// puts the old household folder back and reports the failure.
    /// Windows only: a directory holding a file that is open without sharing
    /// cannot be deleted or renamed there.
    #[cfg(windows)]
    #[test]
    fn a_failed_swap_puts_the_old_data_back() {
        use std::os::windows::fs::OpenOptionsExt;

        let root = std::env::temp_dir().join(format!("zaklon-backup-test-{}", uuid::Uuid::new_v4()));
        let cfg = Config::load_or_init(&root).unwrap();
        let db = Db::open(&cfg.db_path()).unwrap();
        db.set_setting("marker", "before").unwrap();
        let zip = create(&cfg, &db, &cfg.backups_dir(), false).unwrap();
        db.set_setting("marker", "current").unwrap();
        drop(db);
        stage_restore(&cfg, &zip).unwrap();

        // A leftover "household.new" with a file held open and not shared:
        // the cleanup cannot remove it, and the finished folder cannot be
        // renamed into place.
        let fresh = root.join("household.new");
        std::fs::create_dir_all(fresh.join("tls")).unwrap();
        let lock = std::fs::OpenOptions::new().write(true).create(true).share_mode(0).open(fresh.join("tls").join("lock")).unwrap();

        let result = finish_pending_restore(&root);
        drop(lock);
        let err = result.expect_err("the swap cannot succeed while the folder is locked");
        assert!(err.contains("swapping in the restored data"), "{err}");

        // The old data is back where it belongs, untouched.
        let db = Db::open(&cfg.db_path()).unwrap();
        assert_eq!(db.get_setting("marker").unwrap().as_deref(), Some("current"));
        drop(db);
        assert!(cfg.config_path().is_file());
        // Nothing is left under the "before restore" name: it went back.
        assert!(!std::fs::read_dir(cfg.backups_dir()).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with("household-before-restore-")));
        // The failure is reported (Err above). The restore still shows as
        // pending, but its database and settings were already moved into
        // "household.new", so the next start discards it as incomplete: the
        // household must stage the backup again.
        assert!(restore_pending(&root));
        assert!(!root.join(PENDING).join("household.db").exists());
        assert!(finish_pending_restore(&root).is_err(), "the next start discards the emptied restore");
        assert!(!restore_pending(&root));
        let db = Db::open(&cfg.db_path()).unwrap();
        assert_eq!(db.get_setting("marker").unwrap().as_deref(), Some("current"));
        drop(db);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Entries that try to leave the unpacking folder are never written.
    #[test]
    fn a_backup_cannot_write_outside_the_pending_folder() {
        let base = std::env::temp_dir().join(format!("zaklon-backup-test-{}", uuid::Uuid::new_v4()));
        let root = base.join("root");
        let cfg = Config::load_or_init(&root).unwrap();
        let db = Db::open(&cfg.db_path()).unwrap();
        let good = create(&cfg, &db, &cfg.backups_dir(), false).unwrap();
        drop(db);

        // The real backup's files plus some hostile names.
        let build = |extra: &[&str]| -> PathBuf {
            let path = base.join(format!("evil-{}.zip", uuid::Uuid::new_v4()));
            let mut src = zip::ZipArchive::new(std::fs::File::open(&good).unwrap()).unwrap();
            let mut out = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            for name in extra {
                out.start_file(*name, opts).unwrap();
                out.write_all(b"pwned").unwrap();
            }
            for i in 0..src.len() {
                let mut e = src.by_index(i).unwrap();
                let name = e.name().to_string();
                let mut bytes = Vec::new();
                e.read_to_end(&mut bytes).unwrap();
                out.start_file(name, opts).unwrap();
                out.write_all(&bytes).unwrap();
            }
            out.finish().unwrap();
            path
        };
        let outside = |root: &Path| -> Vec<PathBuf> {
            [
                base.join("evil.txt"),
                base.join("x"),
                root.join("evil.txt"),
                root.join("x"),
                root.join("household").join("evil.txt"),
                root.join(PENDING).join("x"),
                root.join(format!("{PENDING}.tmp")).join("x"),
                root.join(PENDING).join("evil.txt"),
                root.join(PENDING).join("abs.txt"),
            ]
            .into_iter()
            .filter(|p| p.exists())
            .collect()
        };

        let evil = build(&["../evil.txt", "tls/../x", "../../evil.txt", "tls/../../evil.txt", "tls\\..\\..\\evil.txt", "/abs.txt", "household/../../evil.txt"]);
        let staged = stage_restore(&cfg, &evil);
        assert!(staged.is_ok(), "the hostile names are skipped, the rest restores: {staged:?}");
        assert_eq!(outside(&root), Vec::<PathBuf>::new(), "nothing written outside");
        let tls: Vec<String> = std::fs::read_dir(root.join(PENDING).join("tls")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        assert!(tls.iter().all(|n| n != "x" && !n.contains("..")), "{tls:?}");

        // "tls/.." names the pending folder itself: the file cannot be
        // created, so the restore fails, but nothing is written anywhere and
        // the earlier staged restore is kept.
        let evil = build(&["tls/.."]);
        assert!(stage_restore(&cfg, &evil).is_err());
        assert_eq!(outside(&root), Vec::<PathBuf>::new());
        assert!(!root.join(format!("{PENDING}.tmp")).exists(), "the half-unpacked folder is removed");
        assert!(restore_pending(&root), "the earlier staged restore still waits");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn automatic_backups_keep_seven() {
        let root = std::env::temp_dir().join(format!("zaklon-backup-test-{}", uuid::Uuid::new_v4()));
        let cfg = Config::load_or_init(&root).unwrap();
        let db = Db::open(&cfg.db_path()).unwrap();
        for i in 0..9 {
            std::fs::write(cfg.backups_dir().join(format!("{AUTO_PREFIX}2020-01-0{i}-000000.zip")), b"x").unwrap();
        }
        // All are "old" by name but new by time: nothing is due, nothing pruned.
        auto_backup_if_due(&cfg, &db);
        assert_eq!(std::fs::read_dir(cfg.backups_dir()).unwrap().count(), 9);
        drop(db);
        let _ = std::fs::remove_dir_all(&root);
    }
}
