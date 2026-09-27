//! Backups of the household's own data: supplies and their history, paired
//! devices, settings, the hub's identity. The library, maps and AI models are
//! not included: they are large and come back by downloading or from USB.
//!
//! A backup is one zip file. The hub makes one a day by itself (keeping the
//! last seven) and one on request into any folder, e.g. a USB stick.
//!
//! Backups are encrypted with the household password (see [`BackupKey`]).
//! An encrypted backup holds `manifest.json` in the clear (when and where it
//! was made), `key.age` (the backup key, locked with the password) and
//! `data.age`: an unencrypted backup (the manifest, `household.db`,
//! `hub.json` and `tls/`) encrypted with age (<https://age-encryption.org>),
//! so the standard `age` tool can open one too. A hub set up before backups
//! were encrypted makes unencrypted backups until the household password is
//! entered once; those, and older backups, still restore.
//!
//! Restoring never touches the open database: the backup is checked and
//! unpacked into `restore-pending/`, and the next start swaps it in, keeping
//! the replaced data under `backups/` first. On a hub with paired phones, the
//! swap keeps who may connect as it is (see [`finish_pending_restore`]).

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use age::secrecy::{ExposeSecret, SecretString};
use base64::Engine;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use zaklon_core::dates::file_stamp;
use zaklon_core::{Config, Db};

/// Kept here too for callers that know it from before `zaklon_core::dates`.
pub use zaklon_core::dates::now_rfc3339;

/// A backup with its data in the clear. Older versions of Zaklon restore it.
const FORMAT_PLAIN: u32 = 1;
/// An encrypted backup: `key.age` and `data.age`, a plain backup encrypted.
const FORMAT_ENCRYPTED: u32 = 2;
/// The newest format this version restores.
const FORMAT: u32 = FORMAT_ENCRYPTED;
const AUTO_PREFIX: &str = "zaklon-auto-";
const KEEP_AUTO: usize = 7;
const PENDING: &str = "restore-pending";
/// Largest file accepted inside a backup (the database of a very busy household).
const MAX_ENTRY: u64 = 2 << 30;
/// An encrypted backup's own files.
const KEY_FILE: &str = "key.age";
const DATA_FILE: &str = "data.age";
/// Far more than a locked key takes (about 200 bytes).
const MAX_KEY_FILE: u64 = 64 * 1024;

/// The setting that holds the household's [`BackupKey`] (JSON).
pub const SETTING_KEY: &str = "backup_key";
/// The setting that holds the household password's Argon2 hash.
const SETTING_PASSWORD: &str = "household_password_hash";
/// scrypt work factor for locking the backup key with the password:
/// N = 2^18, 256 MiB and well under a second on a laptop. Tests use less.
const WORK_FACTOR: u8 = if cfg!(test) { 10 } else { 18 };
/// The most work opening a backup's key may take (N = 2^20, 1 GiB), so a
/// damaged or hostile file cannot keep the hub busy for hours.
const MAX_WORK_FACTOR: u8 = 20;

/// Settings that stay as they are when a backup is restored onto a hub with
/// paired phones: who may connect, and with what.
const KEPT_SETTINGS: &[&str] = &[SETTING_PASSWORD, crate::hotspot::SETTING_PASSPHRASE, crate::hotspot::SETTING_PREVIOUS, SETTING_KEY];
/// hub.json fields kept the same way: the hub's identity and where its phones find it.
const KEPT_CONFIG: &[&str] = &["hub_id", "port", "local_port", "install_port", "beacon_port"];

const NOT_A_BACKUP: &str = "this is not a Zaklon backup";
const INCOMPLETE: &str = "the backup is incomplete";
const NEWER: &str = "this backup was made by a newer Zaklon; update first";
const WRONG_PASSWORD: &str = "the password does not open this backup";
const NEEDS_PASSWORD: &str = "this backup is encrypted; enter the household password";
const KEY_DAMAGED: &str = "the backup's key is damaged";
const HUB_KEY_DAMAGED: &str = "this hub's backup key cannot be read; turn backup encryption on again";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    pub app_version: String,
    /// RFC 3339.
    pub created: String,
    pub hub_id: String,
    pub hub_name: String,
    /// The data is encrypted with the household password.
    #[serde(default)]
    pub encrypted: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupFile {
    pub path: String,
    pub name: String,
    pub size: u64,
    /// RFC 3339.
    pub created: String,
    pub automatic: bool,
    pub encrypted: bool,
}

// ---- the backup key ---------------------------------------------------------------

/// The household's backup key. Every backup is encrypted to `recipient`, an
/// age X25519 public key, so the hub makes backups by itself without anyone
/// typing the password. The matching secret key is kept only locked with the
/// household password (`locked`: a small age file with an scrypt passphrase
/// stanza, in base64), and every backup carries that locked copy: a backup
/// opens on any computer with the password that was in force when it was
/// made, and with nothing else. A new password gets a new key, so backups
/// made before a change keep needing the password from before.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupKey {
    pub recipient: String,
    pub locked: String,
}

impl BackupKey {
    /// A new key, locked with `password`. Slow on purpose (scrypt): call it
    /// from a blocking thread.
    pub fn new(password: &str) -> Result<Self, String> {
        let fail = |e: &dyn std::fmt::Display| format!("making the backup key: {e}");
        let secret = age::x25519::Identity::generate();
        let mut lock = age::scrypt::Recipient::new(SecretString::from(password.to_owned()));
        lock.set_work_factor(WORK_FACTOR);
        let encryptor = age::Encryptor::with_recipients(std::iter::once(&lock as &dyn age::Recipient)).map_err(|e| fail(&e))?;
        let mut locked = Vec::new();
        let mut writer = encryptor.wrap_output(&mut locked).map_err(|e| fail(&e))?;
        writer.write_all(secret.to_string().expose_secret().as_bytes()).map_err(|e| fail(&e))?;
        writer.finish().map_err(|e| fail(&e))?;
        Ok(Self { recipient: secret.to_public().to_string(), locked: base64::engine::general_purpose::STANDARD.encode(locked) })
    }

    /// This hub's key, if its backups are encrypted yet. A key that cannot
    /// be read is an error: backups then stop rather than quietly go out
    /// unencrypted.
    pub fn load(db: &Db) -> Result<Option<Self>, String> {
        let Some(text) = db.get_setting(SETTING_KEY).map_err(|e| format!("reading the backup key: {e:#}"))? else {
            return Ok(None);
        };
        let key: Self = serde_json::from_str(&text).map_err(|_| HUB_KEY_DAMAGED.to_string())?;
        key.recipient_key()?;
        key.locked_bytes()?;
        Ok(Some(key))
    }

    pub fn to_setting(&self) -> String {
        serde_json::json!({ "recipient": self.recipient, "locked": self.locked }).to_string()
    }

    fn recipient_key(&self) -> Result<age::x25519::Recipient, String> {
        self.recipient.parse().map_err(|_| HUB_KEY_DAMAGED.to_string())
    }

    fn locked_bytes(&self) -> Result<Vec<u8>, String> {
        base64::engine::general_purpose::STANDARD.decode(&self.locked).map_err(|_| HUB_KEY_DAMAGED.to_string())
    }
}

/// Set the household password (first setup or a change) together with a new
/// backup key locked with it: written at once, so they always match. Slow
/// (Argon2 and scrypt): call it from a blocking thread.
pub fn set_household_password(db: &Db, password: &str) -> anyhow::Result<()> {
    let hash = zaklon_core::pairing::hash_password(password)?;
    let key = BackupKey::new(password).map_err(anyhow::Error::msg)?.to_setting();
    db.set_settings(&[(SETTING_PASSWORD, hash.as_str()), (SETTING_KEY, key.as_str())])
}

/// For a hub set up before backups were encrypted: check the household
/// password and encrypt every backup from now on. Slow (Argon2 and scrypt):
/// call it from a blocking thread.
pub fn turn_on_encryption(db: &Db, password: &str) -> Result<(), String> {
    let hash = db.get_setting(SETTING_PASSWORD).map_err(|e| format!("{e:#}"))?.ok_or("set a household password first")?;
    if !zaklon_core::pairing::verify_password(password, &hash) {
        return Err("wrong household password".into());
    }
    let key = BackupKey::new(password)?;
    db.set_setting(SETTING_KEY, &key.to_setting()).map_err(|e| format!("{e:#}"))
}

/// "on" when backups are encrypted, "off" when not yet (or when this hub's
/// key cannot be read: turning encryption on again mends it), "no_password"
/// before the household password is set.
pub fn encryption_state(db: &Db) -> &'static str {
    match BackupKey::load(db) {
        Ok(Some(_)) => "on",
        _ if !db.is_set_up().unwrap_or(false) => "no_password",
        _ => "off",
    }
}

/// Open a backup's locked key with the password.
fn unlock_key(locked: &[u8], password: &str) -> Result<age::x25519::Identity, String> {
    let decryptor = age::Decryptor::new(locked).map_err(|_| KEY_DAMAGED.to_string())?;
    if !decryptor.is_scrypt() {
        return Err(KEY_DAMAGED.into());
    }
    let mut lock = age::scrypt::Identity::new(SecretString::from(password.to_owned()));
    lock.set_max_work_factor(MAX_WORK_FACTOR);
    let reader = match decryptor.decrypt(std::iter::once(&lock as &dyn age::Identity)) {
        Ok(reader) => reader,
        Err(age::DecryptError::DecryptionFailed | age::DecryptError::NoMatchingKeys) => return Err(WRONG_PASSWORD.into()),
        Err(_) => return Err(KEY_DAMAGED.into()),
    };
    let mut text = String::new();
    reader.take(1024).read_to_string(&mut text).map_err(|_| KEY_DAMAGED.to_string())?;
    let text = SecretString::from(text);
    text.expose_secret().trim().parse().map_err(|_| KEY_DAMAGED.to_string())
}

// ---- making backups ---------------------------------------------------------------

/// Write a backup zip into `dir`; returns its path. Encrypted once the hub
/// has a backup key. Slow disk work: call it from a blocking thread. The
/// database stays usable meanwhile.
pub fn create(cfg: &Config, db: &Db, dir: &Path, automatic: bool) -> Result<PathBuf, String> {
    let key = BackupKey::load(db)?;
    std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    let stamp = file_stamp();
    let name = if automatic { format!("{AUTO_PREFIX}{stamp}.zip") } else { format!("zaklon-backup-{stamp}.zip") };
    let target = dir.join(&name);
    let part = dir.join(format!("{name}.part"));

    // A consistent copy of the database, even while it is in use.
    let tmp = std::env::temp_dir().join(format!("zaklon-backup-{}", uuid::Uuid::new_v4()));
    let (tmp_db, tmp_zip) = (tmp.with_extension("db"), tmp.with_extension("zip"));
    db.snapshot_to(&tmp_db).map_err(|e| format!("copying the database: {e}"))?;
    let manifest = |format, encrypted| Manifest {
        format,
        app_version: env!("CARGO_PKG_VERSION").into(),
        created: now_rfc3339(),
        hub_id: cfg.hub_id.clone(),
        hub_name: cfg.hub_name.clone(),
        encrypted,
    };
    let result = match &key {
        None => write_plain(cfg, &tmp_db, &part, &manifest(FORMAT_PLAIN, false)),
        // The plain backup is made first, then encrypted as a stream: the
        // database is never held in memory.
        Some(key) => write_plain(cfg, &tmp_db, &tmp_zip, &manifest(FORMAT_PLAIN, false))
            .and_then(|()| write_encrypted(&tmp_zip, &part, &manifest(FORMAT_ENCRYPTED, true), key)),
    };
    let _ = std::fs::remove_file(&tmp_db);
    let _ = std::fs::remove_file(&tmp_zip);
    if let Err(e) = result {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    std::fs::rename(&part, &target).map_err(|e| format!("finishing the backup: {e}"))?;
    info!(path = %target.display(), encrypted = key.is_some(), "backup written");
    Ok(target)
}

fn write_manifest<W: Write + std::io::Seek>(zip: &mut zip::ZipWriter<W>, manifest: &Manifest) -> Result<(), String> {
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("manifest.json", opts).map_err(|e| e.to_string())?;
    zip.write_all(&serde_json::to_vec_pretty(manifest).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// A backup with its data in the clear, from the database copy `db_copy`.
fn write_plain(cfg: &Config, db_copy: &Path, to: &Path, manifest: &Manifest) -> Result<(), String> {
    let file = File::create(to).map_err(|e| format!("writing the backup: {e}"))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    write_manifest(&mut zip, manifest)?;
    // The database can be large: streamed, not read into memory.
    zip.start_file("household.db", opts).map_err(|e| e.to_string())?;
    let mut copy = File::open(db_copy).map_err(|e| e.to_string())?;
    std::io::copy(&mut copy, &mut zip).map_err(|e| format!("writing the backup: {e}"))?;
    drop(copy);
    let mut add = |name: &str, bytes: &[u8]| -> Result<(), String> {
        zip.start_file(name, opts).map_err(|e| e.to_string())?;
        zip.write_all(bytes).map_err(|e| e.to_string())
    };
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
    file.sync_all().map_err(|e| format!("writing the backup: {e}"))
}

/// The plain backup at `plain`, encrypted to the backup key, with the key's
/// locked copy next to it.
fn write_encrypted(plain: &Path, to: &Path, manifest: &Manifest, key: &BackupKey) -> Result<(), String> {
    let write_err = |e: &dyn std::fmt::Display| format!("writing the backup: {e}");
    let recipient = key.recipient_key()?;
    let locked = key.locked_bytes()?;
    let file = File::create(to).map_err(|e| write_err(&e))?;
    let mut zip = zip::ZipWriter::new(file);
    // Encrypted data does not compress.
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    write_manifest(&mut zip, manifest)?;
    zip.start_file(KEY_FILE, stored).map_err(|e| e.to_string())?;
    zip.write_all(&locked).map_err(|e| write_err(&e))?;
    zip.start_file(DATA_FILE, stored).map_err(|e| e.to_string())?;
    let encryptor = age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient)).map_err(|e| write_err(&e))?;
    let mut writer = encryptor.wrap_output(&mut zip).map_err(|e| write_err(&e))?;
    let mut data = File::open(plain).map_err(|e| write_err(&e))?;
    std::io::copy(&mut data, &mut writer).map_err(|e| write_err(&e))?;
    writer.finish().map_err(|e| write_err(&e))?;
    let file = zip.finish().map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| write_err(&e))
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
                    let (created, encrypted) = read_manifest(&e.path()).map(|m| (m.created, m.encrypted)).unwrap_or_default();
                    Some(BackupFile { path: e.path().display().to_string(), automatic: name.starts_with(AUTO_PREFIX), name, size: meta.len(), created, encrypted })
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
    let file = File::open(path).ok()?;
    let mut zip = zip::ZipArchive::new(file).ok()?;
    let mut entry = zip.by_name("manifest.json").ok()?;
    let mut text = String::new();
    entry.by_ref().take(64 * 1024).read_to_string(&mut text).ok()?;
    serde_json::from_str(&text).ok()
}

// ---- restoring --------------------------------------------------------------------

/// A backup ready to be unpacked: this version can restore it, and if it is
/// encrypted, its key was opened with the password.
pub struct Unlocked {
    path: PathBuf,
    manifest: Manifest,
    secret: Option<age::x25519::Identity>,
}

/// Check that `zip_path` is a backup this version restores and, if it is
/// encrypted, open its key with `password` (not needed for an unencrypted
/// one). Slow for an encrypted backup (scrypt): call it from a blocking thread.
pub fn unlock(zip_path: &Path, password: &str) -> Result<Unlocked, String> {
    let manifest = read_manifest(zip_path).ok_or(NOT_A_BACKUP)?;
    if manifest.format > FORMAT {
        return Err(NEWER.into());
    }
    let file = File::open(zip_path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| NOT_A_BACKUP.to_string())?;
    let locked = match zip.by_name(KEY_FILE) {
        Ok(entry) if entry.size() <= MAX_KEY_FILE => {
            let mut bytes = Vec::new();
            entry.take(MAX_KEY_FILE).read_to_end(&mut bytes).map_err(|_| KEY_DAMAGED.to_string())?;
            Some(bytes)
        }
        Ok(_) => return Err(KEY_DAMAGED.into()),
        Err(_) if manifest.encrypted => return Err(INCOMPLETE.into()),
        Err(_) => None,
    };
    let secret = match locked {
        None => None,
        Some(_) if password.is_empty() => return Err(NEEDS_PASSWORD.into()),
        Some(bytes) => Some(unlock_key(&bytes, password)?),
    };
    Ok(Unlocked { path: zip_path.to_path_buf(), manifest, secret })
}

/// Check a backup and unpack it next to the data, ready for the next start.
pub fn stage_restore(cfg: &Config, zip_path: &Path, password: &str) -> Result<Manifest, String> {
    stage_unlocked(cfg, unlock(zip_path, password)?)
}

/// Unpack a backup opened with [`unlock`] next to the data, ready for the next start.
pub fn stage_unlocked(cfg: &Config, backup: Unlocked) -> Result<Manifest, String> {
    // Unpack next to the real pending folder and swap it in only when
    // everything is written and checked: a power cut halfway leaves nothing
    // half-done, and a failed second attempt keeps the first one waiting.
    let pending = cfg.root.join(format!("{PENDING}.tmp"));
    let _ = std::fs::remove_dir_all(&pending);
    std::fs::create_dir_all(pending.join("tls")).map_err(|e| e.to_string())?;
    let result = (|| -> Result<(), String> {
        match &backup.secret {
            None => unpack(&backup.path, &pending)?,
            Some(secret) => {
                // Decrypted into a file, not memory: the database may be large.
                let plain = pending.join("backup.zip");
                decrypt_data(&backup.path, secret, &plain)?;
                // Inside is an unencrypted backup, and only that.
                let inner = read_manifest(&plain).ok_or(NOT_A_BACKUP)?;
                if inner.format != FORMAT_PLAIN {
                    return Err(NOT_A_BACKUP.into());
                }
                let unpacked = unpack(&plain, &pending);
                let _ = std::fs::remove_file(&plain);
                unpacked?;
            }
        }
        if !pending.join("household.db").is_file() || !pending.join("hub.json").is_file() {
            return Err(INCOMPLETE.into());
        }
        // It must open as a household database, and the hub must be able
        // to start with its settings: a restore that cannot start is refused
        // now, not found out after the swap.
        Db::open(&pending.join("household.db")).map_err(|_| "the backup's database is damaged".to_string())?;
        settings_ok(&pending)?;
        Ok(())
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&pending);
        return Err(e);
    }
    let ready = cfg.root.join(PENDING);
    let _ = std::fs::remove_dir_all(&ready);
    std::fs::rename(&pending, &ready).map_err(|e| format!("preparing the restore: {e}"))?;
    info!(from = %backup.path.display(), encrypted = backup.secret.is_some(), "restore staged; it completes on the next start");
    Ok(backup.manifest)
}

/// Decrypt an encrypted backup's data (an unencrypted backup) into `to`.
fn decrypt_data(zip_path: &Path, secret: &age::x25519::Identity, to: &Path) -> Result<(), String> {
    let file = File::open(zip_path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| NOT_A_BACKUP.to_string())?;
    let entry = zip.by_name(DATA_FILE).map_err(|_| INCOMPLETE.to_string())?;
    let decryptor = age::Decryptor::new(entry).map_err(|_| INCOMPLETE.to_string())?;
    // The key opened, but it is not the one this data was encrypted to.
    let reader = decryptor.decrypt(std::iter::once(secret as &dyn age::Identity)).map_err(|_| KEY_DAMAGED.to_string())?;
    let mut out = File::create(to).map_err(|e| format!("unpacking the backup: {e}"))?;
    // Room for the largest database and the rest.
    let mut limited = reader.take(2 * MAX_ENTRY);
    let copied = std::io::copy(&mut limited, &mut out);
    match copied {
        Ok(_) => Ok(()),
        // Cut short (a USB stick pulled out too early) or changed.
        Err(e) if matches!(e.kind(), std::io::ErrorKind::InvalidData | std::io::ErrorKind::UnexpectedEof) => Err(INCOMPLETE.into()),
        Err(e) => Err(format!("unpacking the backup: {e}")),
    }
}

/// Unpack an unencrypted backup's files into `into`. Only the files a backup
/// is made of; nothing else is written anywhere.
fn unpack(zip_path: &Path, into: &Path) -> Result<(), String> {
    let file = File::open(zip_path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| NOT_A_BACKUP.to_string())?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().to_string();
        let allowed = matches!(name.as_str(), "manifest.json" | "household.db" | "hub.json")
            || name.strip_prefix("tls/").is_some_and(|n| !n.is_empty() && n != "." && n != ".." && n.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)));
        if !allowed || entry.size() > MAX_ENTRY {
            continue;
        }
        let mut out = File::create(into.join(&name)).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry.by_ref().take(MAX_ENTRY), &mut out).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The unpacked backup's hub.json is settings this hub can start with.
fn settings_ok(dir: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(dir.join("hub.json")).map_err(|_| "the backup's settings are damaged".to_string())?;
    Config::from_json(&text).map(|_| ()).map_err(|_| "the backup's settings are damaged".to_string())
}

pub fn restore_pending(root: &Path) -> bool {
    root.join(PENDING).join("manifest.json").is_file()
}

/// On start, before the database is opened: swap in a staged restore,
/// keeping the replaced household folder under `backups/`. The new folder is
/// complete before anything is moved, and a failed swap is rolled back, so
/// the household always has either its old data or the restored data.
///
/// Who may connect: if this hub has paired phones, they stay as they are,
/// with the household password, the Wi-Fi network's password, the backup
/// key and the hub's identity (TLS key, id, ports); everything else comes
/// from the backup. So a phone removed after the backup was made, or an old
/// password, never comes back. A hub with no paired phones (a new install)
/// takes everything from the backup, so the household's phones keep working.
pub fn finish_pending_restore(root: &Path) -> Result<bool, String> {
    let pending = root.join(PENDING);
    if !restore_pending(root) {
        return Ok(false);
    }
    if !pending.join("household.db").is_file() || !pending.join("hub.json").is_file() {
        let _ = std::fs::remove_dir_all(&pending);
        return Err("the prepared restore was incomplete and was discarded".into());
    }
    // Checked when it was prepared, but perhaps by an older version that
    // checked less: never swap in settings the hub cannot start with.
    if let Err(e) = settings_ok(&pending) {
        let _ = std::fs::remove_dir_all(&pending);
        return Err(format!("{e}; the prepared restore was discarded"));
    }
    let household = root.join("household");
    let fresh = root.join("household.new");
    let _ = std::fs::remove_dir_all(&fresh);
    std::fs::create_dir_all(fresh.join("tls")).map_err(|e| e.to_string())?;
    // Copied, not moved: if the swap fails, the staged restore is still
    // whole and the next start tries again.
    std::fs::copy(pending.join("household.db"), fresh.join("household.db")).map_err(|e| format!("restoring household.db: {e}"))?;
    let keep_access = has_phones(&household.join("household.db"));
    let (settings, tls) = if keep_access {
        // Not read, not kept: rather no restore than the backup's phones.
        let restored = Db::open(&fresh.join("household.db")).map_err(|e| format!("restoring household.db: {e:#}"))?;
        restored
            .take_devices_and_settings_from(&household.join("household.db"), KEPT_SETTINGS)
            .map_err(|e| format!("keeping the paired phones: {e:#}"))?;
        drop(restored);
        (merged_settings(&pending.join("hub.json"), &household.join("hub.json"))?, household.join("tls"))
    } else {
        (std::fs::read(pending.join("hub.json")).map_err(|e| format!("restoring hub.json: {e}"))?, pending.join("tls"))
    };
    std::fs::write(fresh.join("hub.json"), settings).map_err(|e| format!("restoring hub.json: {e}"))?;
    if let Ok(rd) = std::fs::read_dir(&tls) {
        for e in rd.flatten().filter(|e| e.path().is_file()) {
            std::fs::copy(e.path(), fresh.join("tls").join(e.file_name())).map_err(|e| format!("restoring the identity: {e}"))?;
        }
    }
    std::fs::create_dir_all(root.join("backups")).map_err(|e| e.to_string())?;
    let keep = unused_path(&root.join("backups"), &format!("household-before-restore-{}", file_stamp()));
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
    info!(old = %keep.display(), kept_phones = keep_access, "restore finished");
    Ok(true)
}

/// `dir/name`, or `dir/name-2`, `-3`... when that is taken (two restores
/// within one second).
fn unused_path(dir: &Path, name: &str) -> PathBuf {
    let mut path = dir.join(name);
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{name}-{n}"));
        n += 1;
    }
    path
}

/// Whether the database at `path` has paired phones. One that cannot be
/// opened (missing or damaged, perhaps the reason for the restore) has none.
fn has_phones(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    match Db::open(path).and_then(|db| db.count_devices()) {
        Ok(n) => n > 0,
        Err(e) => {
            warn!("the current database cannot be read ({e:#}); the restore takes the phones and the password from the backup");
            false
        }
    }
}

/// The backup's hub.json with this hub's identity and ports, so the phones
/// paired with it keep finding it. If this hub's own file cannot be read,
/// the backup's is used as it is.
fn merged_settings(restored: &Path, current: &Path) -> Result<Vec<u8>, String> {
    let text = std::fs::read_to_string(restored).map_err(|e| format!("restoring hub.json: {e}"))?;
    let merged = (|| -> Option<String> {
        let mut merged: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
        let current: serde_json::Value = serde_json::from_str(std::fs::read_to_string(current).ok()?.trim_start_matches('\u{feff}')).ok()?;
        let fields = merged.as_object_mut()?;
        for name in KEPT_CONFIG {
            match current.get(name) {
                Some(value) => fields.insert(name.to_string(), value.clone()),
                None => fields.remove(*name),
            };
        }
        let out = serde_json::to_string_pretty(&merged).ok()?;
        Config::from_json(&out).ok()?;
        Some(out)
    })();
    Ok(merged
        .unwrap_or_else(|| {
            warn!("this hub's settings cannot be read; the restore takes the backup's as they are");
            text
        })
        .into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use zaklon_core::db::Device;

    fn temp_root() -> PathBuf {
        std::env::temp_dir().join(format!("zaklon-backup-test-{}", uuid::Uuid::new_v4()))
    }

    fn add_phone(db: &Db, id: &str) {
        let dev = Device { id: id.into(), name: id.into(), platform: "android".into(), created_at: now_rfc3339(), last_seen: None };
        db.insert_device(&dev, &format!("{id}-token-hash")).unwrap();
    }

    fn phones(db: &Db) -> Vec<String> {
        db.list_devices().unwrap().into_iter().map(|d| d.id).collect()
    }

    fn password_is(db: &Db, password: &str) -> bool {
        zaklon_core::pairing::verify_password(password, &db.get_setting(SETTING_PASSWORD).unwrap().unwrap_or_default())
    }

    fn entry_names(zip: &Path) -> Vec<String> {
        let mut names: Vec<String> = zip::ZipArchive::new(File::open(zip).unwrap()).unwrap().file_names().map(str::to_string).collect();
        names.sort();
        names
    }

    fn entry_bytes(zip: &Path, name: &str) -> Vec<u8> {
        let mut zip = zip::ZipArchive::new(File::open(zip).unwrap()).unwrap();
        let mut bytes = Vec::new();
        zip.by_name(name).unwrap().read_to_end(&mut bytes).unwrap();
        bytes
    }

    fn write_tls(cfg: &Config, cert: &str, key: &str) {
        std::fs::write(cfg.tls_dir().join("hub-cert.pem"), cert).unwrap();
        std::fs::write(cfg.tls_dir().join("hub-key.pem"), key).unwrap();
    }

    fn read_tls(cfg: &Config) -> (String, String) {
        (std::fs::read_to_string(cfg.tls_dir().join("hub-cert.pem")).unwrap(), std::fs::read_to_string(cfg.tls_dir().join("hub-key.pem")).unwrap())
    }

    /// Restore as the hub does: staged now, swapped in on the next start.
    fn restore(cfg: &Config, zip: &Path, password: &str) -> Result<(), String> {
        stage_restore(cfg, zip, password)?;
        assert!(finish_pending_restore(&cfg.root).unwrap());
        Ok(())
    }

    #[test]
    fn backup_and_restore_round_trip() {
        let root = temp_root();
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
        stage_restore(&cfg, &zip, "").unwrap();
        assert!(restore_pending(&root));
        // A restore finished within the same second set its data aside already.
        let earlier = cfg.backups_dir().join(format!("household-before-restore-{}", file_stamp()));
        std::fs::create_dir_all(&earlier).unwrap();
        assert!(finish_pending_restore(&root).unwrap());
        assert!(!restore_pending(&root));
        let db = Db::open(&cfg.db_path()).unwrap();
        assert_eq!(db.get_setting("marker").unwrap().as_deref(), Some("before"));
        assert_eq!(std::fs::read(cfg.tls_dir().join("cert.pem")).unwrap(), b"CERT");
        // The replaced data was kept, next to the earlier one.
        let kept = std::fs::read_dir(cfg.backups_dir()).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("household-before-restore-")).count();
        assert_eq!(kept, 2);
        assert!(std::fs::read_dir(&earlier).unwrap().next().is_none(), "the earlier one is left alone");
        drop(db);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_encrypted_backup_round_trip() {
        let root = temp_root();
        let cfg = Config::load_or_init(&root).unwrap();
        let db = Db::open(&cfg.db_path()).unwrap();
        set_household_password(&db, "correct horse").unwrap();
        assert_eq!(encryption_state(&db), "on");
        db.set_setting("marker", "before").unwrap();
        write_tls(&cfg, "CERT", "-----BEGIN PRIVATE KEY----- the hub's secret");

        let zip = create(&cfg, &db, &cfg.backups_dir(), false).unwrap();
        // Only the manifest can be read without the password.
        assert_eq!(entry_names(&zip), ["data.age", "key.age", "manifest.json"]);
        let manifest = read_manifest(&zip).unwrap();
        assert!(manifest.encrypted);
        assert_eq!(manifest.format, FORMAT_ENCRYPTED);
        assert!(list(&cfg)[0].encrypted);
        let data = entry_bytes(&zip, DATA_FILE);
        assert!(data.starts_with(b"age-encryption.org/v1\n-> X25519 "), "an age file for the backup key");
        assert!(!data.windows(b"PRIVATE KEY".len()).any(|w| w == b"PRIVATE KEY"));
        // The locked key is the one this hub keeps.
        let key = BackupKey::load(&db).unwrap().unwrap();
        assert_eq!(entry_bytes(&zip, KEY_FILE), key.locked_bytes().unwrap());

        db.set_setting("marker", "after").unwrap();
        drop(db);
        restore(&cfg, &zip, "correct horse").unwrap();
        let db = Db::open(&cfg.db_path()).unwrap();
        assert_eq!(db.get_setting("marker").unwrap().as_deref(), Some("before"));
        assert_eq!(read_tls(&cfg).1, "-----BEGIN PRIVATE KEY----- the hub's secret");
        assert!(!root.join(PENDING).exists());
        let names: Vec<String> = std::fs::read_dir(root.join("household")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        assert!(!names.iter().any(|n| n.ends_with(".zip")), "nothing decrypted is left behind: {names:?}");
        drop(db);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_wrong_or_missing_password_is_refused() {
        let root = temp_root();
        let cfg = Config::load_or_init(&root).unwrap();
        let db = Db::open(&cfg.db_path()).unwrap();
        set_household_password(&db, "correct horse").unwrap();
        let zip = create(&cfg, &db, &cfg.backups_dir(), false).unwrap();
        drop(db);

        assert_eq!(stage_restore(&cfg, &zip, "wrong horse").unwrap_err(), WRONG_PASSWORD);
        assert_eq!(stage_restore(&cfg, &zip, "").unwrap_err(), NEEDS_PASSWORD);
        assert!(!restore_pending(&root));
        assert!(!root.join(format!("{PENDING}.tmp")).exists());

        // A damaged key, and damaged data, are reported as such.
        let bad_key = with_files(&zip, &[(KEY_FILE, b"not an age file")]);
        assert_eq!(stage_restore(&cfg, &bad_key, "correct horse").unwrap_err(), KEY_DAMAGED);
        let mut data = entry_bytes(&zip, DATA_FILE);
        let at = data.len() - 20;
        data[at] ^= 0x55;
        let bad_data = with_files(&zip, &[(DATA_FILE, &data)]);
        assert_eq!(stage_restore(&cfg, &bad_data, "correct horse").unwrap_err(), INCOMPLETE);
        let cut = entry_bytes(&zip, DATA_FILE);
        let cut_short = with_files(&zip, &[(DATA_FILE, &cut[..cut.len() / 2])]);
        assert_eq!(stage_restore(&cfg, &cut_short, "correct horse").unwrap_err(), INCOMPLETE);
        // The key of another backup opens, but not this data.
        let other_root = temp_root();
        let other_cfg = Config::load_or_init(&other_root).unwrap();
        let other_db = Db::open(&other_cfg.db_path()).unwrap();
        set_household_password(&other_db, "correct horse").unwrap();
        let other = create(&other_cfg, &other_db, &other_cfg.backups_dir(), false).unwrap();
        let swapped = with_files(&zip, &[(KEY_FILE, &entry_bytes(&other, KEY_FILE))]);
        assert_eq!(stage_restore(&cfg, &swapped, "correct horse").unwrap_err(), KEY_DAMAGED);
        assert!(!restore_pending(&root));

        stage_restore(&cfg, &zip, "correct horse").unwrap();
        assert!(restore_pending(&root));
        drop(other_db);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&other_root);
    }

    #[test]
    fn backups_need_the_password_from_when_they_were_made() {
        let root = temp_root();
        let cfg = Config::load_or_init(&root).unwrap();
        let db = Db::open(&cfg.db_path()).unwrap();
        set_household_password(&db, "first password").unwrap();
        db.set_setting("marker", "first").unwrap();
        let first = create(&cfg, &db, &root.join("one"), false).unwrap();
        let first_key = BackupKey::load(&db).unwrap().unwrap();

        set_household_password(&db, "second password").unwrap();
        db.set_setting("marker", "second").unwrap();
        let second = create(&cfg, &db, &root.join("two"), false).unwrap();
        assert_ne!(BackupKey::load(&db).unwrap().unwrap(), first_key, "a new password, a new key");

        assert_eq!(unlock(&first, "second password").err().as_deref(), Some(WRONG_PASSWORD));
        assert_eq!(unlock(&second, "first password").err().as_deref(), Some(WRONG_PASSWORD));
        assert!(unlock(&first, "first password").is_ok());
        assert!(unlock(&second, "second password").is_ok());

        // Both restore, each with its own password.
        drop(db);
        restore(&cfg, &first, "first password").unwrap();
        let db = Db::open(&cfg.db_path()).unwrap();
        assert_eq!(db.get_setting("marker").unwrap().as_deref(), Some("first"));
        drop(db);
        restore(&cfg, &second, "second password").unwrap();
        let db = Db::open(&cfg.db_path()).unwrap();
        assert_eq!(db.get_setting("marker").unwrap().as_deref(), Some("second"));
        drop(db);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A phone was stolen and removed and the password changed; then an
    /// older backup is restored to undo a mistake in the supplies.
    #[test]
    fn a_restore_onto_a_hub_with_phones_keeps_who_may_connect() {
        let root = temp_root();
        let mut cfg = Config::load_or_init(&root).unwrap();
        cfg.hub_name = "Name in the backup".into();
        cfg.save().unwrap();
        let db = Db::open(&cfg.db_path()).unwrap();
        set_household_password(&db, "old password").unwrap();
        add_phone(&db, "stolen");
        db.set_setting(crate::hotspot::SETTING_PASSPHRASE, "oldwifipass").unwrap();
        db.set_setting("marker", "from the backup").unwrap();
        write_tls(&cfg, "OLD CERT", "OLD KEY");
        let zip = create(&cfg, &db, &cfg.backups_dir(), false).unwrap();

        db.delete_device("stolen").unwrap();
        add_phone(&db, "new");
        set_household_password(&db, "new password").unwrap();
        let key_now = db.get_setting(SETTING_KEY).unwrap();
        db.set_setting(crate::hotspot::SETTING_PASSPHRASE, "newwifipass").unwrap();
        db.set_setting("marker", "current").unwrap();
        // The hub's identity differs from the backup's too (say, the backup
        // came from the household's old laptop).
        write_tls(&cfg, "NEW CERT", "NEW KEY");
        cfg.hub_id = "this-hub".into();
        cfg.hub_name = "Current name".into();
        cfg.port = 9443;
        cfg.save().unwrap();
        drop(db);

        // The backup opens with the password from when it was made.
        restore(&cfg, &zip, "old password").unwrap();
        let db = Db::open(&cfg.db_path()).unwrap();
        assert_eq!(db.get_setting("marker").unwrap().as_deref(), Some("from the backup"));
        assert_eq!(phones(&db), ["new"], "the removed phone stays removed");
        assert!(db.device_by_token_hash("stolen-token-hash", &now_rfc3339()).unwrap().is_none());
        assert!(password_is(&db, "new password") && !password_is(&db, "old password"));
        assert_eq!(db.get_setting(SETTING_KEY).unwrap(), key_now);
        assert_eq!(db.get_setting(crate::hotspot::SETTING_PASSPHRASE).unwrap().as_deref(), Some("newwifipass"));
        assert_eq!(read_tls(&cfg), ("NEW CERT".to_string(), "NEW KEY".to_string()));
        let restored = Config::load_or_init(&root).unwrap();
        assert_eq!((restored.hub_id.as_str(), restored.port), ("this-hub", 9443));
        assert_eq!(restored.hub_name, "Name in the backup", "other settings come from the backup");
        // Backups made from now on open with the current password.
        let next = create(&restored, &db, &root.join("next"), false).unwrap();
        assert!(unlock(&next, "new password").is_ok());
        drop(db);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A new install (set up, no phones yet) restores the old laptop's backup.
    #[test]
    fn a_restore_onto_a_hub_without_phones_takes_everything() {
        let base = temp_root();
        let old_cfg = Config::load_or_init(&base.join("old")).unwrap();
        let old_db = Db::open(&old_cfg.db_path()).unwrap();
        set_household_password(&old_db, "household password").unwrap();
        add_phone(&old_db, "kitchen");
        old_db.set_setting("marker", "old laptop").unwrap();
        write_tls(&old_cfg, "OLD CERT", "OLD KEY");
        let zip = create(&old_cfg, &old_db, &base.join("usb"), false).unwrap();
        let old_key = old_db.get_setting(SETTING_KEY).unwrap();

        let cfg = Config::load_or_init(&base.join("new")).unwrap();
        let db = Db::open(&cfg.db_path()).unwrap();
        set_household_password(&db, "set up anew").unwrap();
        write_tls(&cfg, "NEW CERT", "NEW KEY");
        drop(db);

        restore(&cfg, &zip, "household password").unwrap();
        let db = Db::open(&cfg.db_path()).unwrap();
        assert_eq!(db.get_setting("marker").unwrap().as_deref(), Some("old laptop"));
        assert_eq!(phones(&db), ["kitchen"], "the household's phones keep working");
        assert!(password_is(&db, "household password") && !password_is(&db, "set up anew"));
        assert_eq!(db.get_setting(SETTING_KEY).unwrap(), old_key);
        assert_eq!(read_tls(&cfg), ("OLD CERT".to_string(), "OLD KEY".to_string()));
        assert_eq!(Config::load_or_init(&cfg.root).unwrap().hub_id, old_cfg.hub_id);
        drop((db, old_db));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn an_old_unencrypted_backup_still_restores() {
        let root = temp_root();
        let cfg = Config::load_or_init(&root).unwrap();
        let db = Db::open(&cfg.db_path()).unwrap();
        db.set_setting("marker", "old").unwrap();
        let old = create(&cfg, &db, &root.join("old"), false).unwrap();
        assert!(entry_names(&old).contains(&"household.db".to_string()));
        assert!(!read_manifest(&old).unwrap().encrypted);
        // As an older version wrote it: no "encrypted" in the manifest.
        let manifest = format!(r#"{{"format": 1, "app_version": "0.1.0", "created": "2026-01-01T00:00:00Z", "hub_id": "{}", "hub_name": "Old"}}"#, cfg.hub_id);
        let older = with_files(&old, &[("manifest.json", manifest.as_bytes())]);

        set_household_password(&db, "correct horse").unwrap();
        assert!(entry_names(&create(&cfg, &db, &root.join("new"), false).unwrap()).contains(&DATA_FILE.to_string()));
        db.set_setting("marker", "current").unwrap();
        drop(db);
        // No password is needed, and one given is not in the way.
        restore(&cfg, &older, "").unwrap();
        let db = Db::open(&cfg.db_path()).unwrap();
        assert_eq!(db.get_setting("marker").unwrap().as_deref(), Some("old"));
        drop(db);
        restore(&cfg, &old, "correct horse").unwrap();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A hub set up before backups were encrypted.
    #[test]
    fn turning_encryption_on_needs_the_household_password() {
        let root = temp_root();
        let cfg = Config::load_or_init(&root).unwrap();
        let db = Db::open(&cfg.db_path()).unwrap();
        assert_eq!(encryption_state(&db), "no_password");
        assert!(turn_on_encryption(&db, "anything at all").unwrap_err().contains("set a household password first"));
        db.set_setting(SETTING_PASSWORD, &zaklon_core::pairing::hash_password("our password").unwrap()).unwrap();
        assert_eq!(encryption_state(&db), "off");
        assert!(!read_manifest(&create(&cfg, &db, &root.join("before"), false).unwrap()).unwrap().encrypted);

        assert_eq!(turn_on_encryption(&db, "not our password").unwrap_err(), "wrong household password");
        assert_eq!(encryption_state(&db), "off");
        turn_on_encryption(&db, "our password").unwrap();
        assert_eq!(encryption_state(&db), "on");
        let zip = create(&cfg, &db, &root.join("after"), false).unwrap();
        assert!(read_manifest(&zip).unwrap().encrypted);
        assert!(unlock(&zip, "our password").is_ok());
        drop(db);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A key that cannot be read stops backups instead of letting them go
    /// out unencrypted; turning encryption on again mends it.
    #[test]
    fn a_damaged_backup_key_stops_backups() {
        let root = temp_root();
        let cfg = Config::load_or_init(&root).unwrap();
        let db = Db::open(&cfg.db_path()).unwrap();
        set_household_password(&db, "correct horse").unwrap();
        db.set_setting(SETTING_KEY, r#"{"recipient": "age1nonsense", "locked": ""}"#).unwrap();
        assert_eq!(create(&cfg, &db, &cfg.backups_dir(), false).unwrap_err(), HUB_KEY_DAMAGED);
        assert_eq!(std::fs::read_dir(cfg.backups_dir()).unwrap().count(), 0, "nothing written");
        assert_eq!(encryption_state(&db), "off");
        turn_on_encryption(&db, "correct horse").unwrap();
        assert!(read_manifest(&create(&cfg, &db, &cfg.backups_dir(), false).unwrap()).unwrap().encrypted);
        drop(db);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_incomplete_restore_leaves_the_data_alone() {
        let root = temp_root();
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

    /// A copy of the backup `good` with some of its files replaced.
    fn with_files(good: &Path, replace: &[(&str, &[u8])]) -> PathBuf {
        let path = good.with_file_name(format!("changed-{}.zip", uuid::Uuid::new_v4()));
        let mut src = zip::ZipArchive::new(File::open(good).unwrap()).unwrap();
        let mut out = zip::ZipWriter::new(File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        for i in 0..src.len() {
            let mut e = src.by_index(i).unwrap();
            let name = e.name().to_string();
            let mut bytes = Vec::new();
            e.read_to_end(&mut bytes).unwrap();
            let bytes = replace.iter().find(|(n, _)| *n == name).map_or(bytes, |(_, b)| b.to_vec());
            out.start_file(name, opts).unwrap();
            out.write_all(&bytes).unwrap();
        }
        out.finish().unwrap();
        path
    }

    #[test]
    fn settings_the_hub_cannot_start_with_are_refused() {
        let root = temp_root();
        let cfg = Config::load_or_init(&root).unwrap();
        let db = Db::open(&cfg.db_path()).unwrap();
        let good = create(&cfg, &db, &cfg.backups_dir(), false).unwrap();
        drop(db);
        // Valid JSON, but not the settings of a hub.
        let bads: [&[u8]; 4] = [b"[1, 2]", b"{}", br#"{"hub_id": "x", "port": "8484"}"#, b"not json"];
        for bad in bads {
            let zip = with_files(&good, &[("hub.json", bad)]);
            let err = stage_restore(&cfg, &zip, "").expect_err(&String::from_utf8_lossy(bad));
            assert!(err.contains("settings are damaged"), "{err}");
            assert!(!restore_pending(&root));
        }
        // Settings from an older version, with fields missing, restore.
        let zip = with_files(&good, &[("hub.json", &br#"{"hub_id": "from-an-old-version", "port": 8484}"#[..])]);
        stage_restore(&cfg, &zip, "").unwrap();
        assert!(finish_pending_restore(&root).unwrap());
        assert_eq!(Config::load_or_init(&root).unwrap().hub_id, "from-an-old-version");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A restore prepared by an older version, which only checked that the
    /// settings were JSON, is not swapped in if the hub could not start.
    #[test]
    fn a_prepared_restore_with_bad_settings_is_discarded() {
        let root = temp_root();
        let cfg = Config::load_or_init(&root).unwrap();
        let db = Db::open(&cfg.db_path()).unwrap();
        db.set_setting("marker", "current").unwrap();
        let zip = create(&cfg, &db, &cfg.backups_dir(), false).unwrap();
        drop(db);
        stage_restore(&cfg, &zip, "").unwrap();
        std::fs::write(root.join(PENDING).join("hub.json"), br#"{"language": "sr"}"#).unwrap();
        let err = finish_pending_restore(&root).expect_err("not swapped in");
        assert!(err.contains("discarded"), "{err}");
        assert!(!restore_pending(&root));
        assert_eq!(Config::load_or_init(&root).unwrap().hub_id, cfg.hub_id, "the hub starts as before");
        let db = Db::open(&cfg.db_path()).unwrap();
        assert_eq!(db.get_setting("marker").unwrap().as_deref(), Some("current"));
        drop(db);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn not_a_backup_is_refused() {
        let root = temp_root();
        let cfg = Config::load_or_init(&root).unwrap();
        let bogus = root.join("bogus.zip");
        std::fs::write(&bogus, b"not a zip").unwrap();
        assert!(stage_restore(&cfg, &bogus, "").is_err());
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

        let root = temp_root();
        let cfg = Config::load_or_init(&root).unwrap();
        let db = Db::open(&cfg.db_path()).unwrap();
        db.set_setting("marker", "before").unwrap();
        let zip = create(&cfg, &db, &cfg.backups_dir(), false).unwrap();
        db.set_setting("marker", "current").unwrap();
        drop(db);
        stage_restore(&cfg, &zip, "").unwrap();

        // A leftover "household.new" with a file held open and not shared:
        // the cleanup cannot remove it, and the finished folder cannot be
        // renamed into place.
        let fresh = root.join("household.new");
        std::fs::create_dir_all(fresh.join("tls")).unwrap();
        let lock = std::fs::OpenOptions::new().write(true).create(true).truncate(true).share_mode(0).open(fresh.join("tls").join("lock")).unwrap();

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
        // The failure is reported (Err above) and the staged restore is still
        // whole, so the next start (the lock is gone) finishes it.
        assert!(restore_pending(&root));
        assert!(root.join(PENDING).join("household.db").exists());
        assert!(finish_pending_restore(&root).unwrap(), "the next start finishes the restore");
        assert!(!restore_pending(&root));
        let db = Db::open(&cfg.db_path()).unwrap();
        assert_eq!(db.get_setting("marker").unwrap().as_deref(), Some("before"));
        drop(db);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Entries that try to leave the unpacking folder are never written.
    #[test]
    fn a_backup_cannot_write_outside_the_pending_folder() {
        let base = temp_root();
        let root = base.join("root");
        let cfg = Config::load_or_init(&root).unwrap();
        let db = Db::open(&cfg.db_path()).unwrap();
        let good = create(&cfg, &db, &cfg.backups_dir(), false).unwrap();
        drop(db);

        // The real backup's files plus some hostile names.
        let build = |extra: &[&str]| -> PathBuf {
            let path = base.join(format!("evil-{}.zip", uuid::Uuid::new_v4()));
            let mut src = zip::ZipArchive::new(File::open(&good).unwrap()).unwrap();
            let mut out = zip::ZipWriter::new(File::create(&path).unwrap());
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
        let staged = stage_restore(&cfg, &evil, "");
        assert!(staged.is_ok(), "the hostile names are skipped, the rest restores: {staged:?}");
        assert_eq!(outside(&root), Vec::<PathBuf>::new(), "nothing written outside");
        let tls: Vec<String> = std::fs::read_dir(root.join(PENDING).join("tls")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        assert!(tls.iter().all(|n| n != "x" && !n.contains("..")), "{tls:?}");

        // "tls/.." and "tls/." name folders, not files: skipped like the rest.
        let evil = build(&["tls/..", "tls/."]);
        assert!(stage_restore(&cfg, &evil, "").is_ok());
        assert_eq!(outside(&root), Vec::<PathBuf>::new());
        assert!(!root.join(format!("{PENDING}.tmp")).exists(), "no half-unpacked folder is left");
        assert!(restore_pending(&root), "the restore waits");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn automatic_backups_keep_seven() {
        let root = temp_root();
        let cfg = Config::load_or_init(&root).unwrap();
        let db = Db::open(&cfg.db_path()).unwrap();
        for i in 0..9 {
            std::fs::write(cfg.backups_dir().join(format!("{AUTO_PREFIX}2020-01-0{i}-000000.zip")), b"x").unwrap();
        }
        // All are "old" by name but new by time: nothing is due, nothing pruned.
        auto_backup_if_due(&cfg, &db);
        assert_eq!(std::fs::read_dir(cfg.backups_dir()).unwrap().count(), 9);

        // Two days old by time: a new one is made and only the last seven stay.
        let old = std::time::SystemTime::now() - Duration::from_secs(2 * 24 * 3600);
        for e in std::fs::read_dir(cfg.backups_dir()).unwrap().flatten() {
            File::options().write(true).open(e.path()).unwrap().set_modified(old).unwrap();
        }
        auto_backup_if_due(&cfg, &db);
        let mut names: Vec<String> =
            std::fs::read_dir(cfg.backups_dir()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        names.sort();
        assert_eq!(names.len(), KEEP_AUTO, "{names:?}");
        assert_eq!(names[0], format!("{AUTO_PREFIX}2020-01-03-000000.zip"), "the oldest went");
        let newest = names.last().unwrap();
        assert!(!newest.starts_with(&format!("{AUTO_PREFIX}2020")), "today's is kept: {names:?}");
        assert!(read_manifest(&cfg.backups_dir().join(newest)).is_some());
        drop(db);
        let _ = std::fs::remove_dir_all(&root);
    }
}
