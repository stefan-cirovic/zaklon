//! SQLite storage for the household database. One connection guarded by a
//! mutex is plenty for a household; WAL mode keeps reads cheap.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};

/// Kept here too for callers that know it from before [`crate::dates`].
pub use crate::dates::now_rfc3339;

pub struct Db {
    conn: Mutex<Connection>,
    /// The database file; `None` for an in-memory database.
    path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub created_at: String,
    pub last_seen: Option<String>,
}

// Older databases also hold an empty `profiles` table from an early plan
// (id, name, avatar, language, accent, password_hash, created_at); nothing
// reads it. Profiles (docs/SPEC.md) must not assume `CREATE TABLE IF NOT
// EXISTS profiles` gives them their own shape there.
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS devices (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    platform TEXT NOT NULL,
    token_hash TEXT NOT NULL,
    created_at TEXT NOT NULL,
    last_seen TEXT
);
CREATE TABLE IF NOT EXISTS items (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    quantity REAL NOT NULL DEFAULT 0,
    unit TEXT NOT NULL DEFAULT 'pcs',
    category TEXT NOT NULL DEFAULT 'other',
    place TEXT,
    expiry TEXT,
    barcode TEXT,
    min_quantity REAL,
    notes TEXT,
    updated_at TEXT NOT NULL,
    updated_by TEXT,
    deleted INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS items_barcode ON items(barcode);
CREATE TABLE IF NOT EXISTS history (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    at TEXT NOT NULL,
    actor TEXT,
    entity TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    action TEXT NOT NULL,
    before_json TEXT,
    after_json TEXT
);
"#;

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA).context("applying schema")?;
        conn.execute_batch(crate::supplies::SCHEMA).context("applying supplies schema")?;
        conn.execute_batch(crate::memory::SCHEMA).context("applying memory schema")?;
        conn.execute_batch(crate::conversations::SCHEMA).context("applying conversations schema")?;
        crate::supplies::migrate(&conn).context("migrating supplies")?;
        Ok(Self { conn: Mutex::new(conn), path: Some(path.to_path_buf()) })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        conn.execute_batch(crate::supplies::SCHEMA)?;
        conn.execute_batch(crate::memory::SCHEMA)?;
        conn.execute_batch(crate::conversations::SCHEMA)?;
        crate::supplies::migrate(&conn)?;
        Ok(Self { conn: Mutex::new(conn), path: None })
    }

    /// A consistent copy of the whole database into a new file, safe while in
    /// use. It reads through a connection of its own, so requests keep using
    /// the database while a large copy is written (WAL lets readers and the
    /// writer work side by side).
    pub fn snapshot_to(&self, path: &Path) -> Result<()> {
        let _ = std::fs::remove_file(path);
        let target = path.to_string_lossy();
        match &self.path {
            Some(file) => {
                let conn = Connection::open(file).with_context(|| format!("opening {}", file.display()))?;
                conn.busy_timeout(std::time::Duration::from_secs(5))?;
                conn.execute("VACUUM INTO ?1", [target.as_ref()])?;
            }
            None => {
                self.lock().execute("VACUUM INTO ?1", [target.as_ref()])?;
            }
        }
        Ok(())
    }

    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    // ---- settings -------------------------------------------------------

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.lock();
        Ok(conn
            .query_row("SELECT value FROM settings WHERE key = ?1", params![key], |r| r.get(0))
            .optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Several settings at once: all of them are written, or none (for
    /// values that must always match, like a password and what it locks).
    pub fn set_settings(&self, pairs: &[(&str, &str)]) -> Result<()> {
        let conn = self.lock();
        let tx = conn.unchecked_transaction()?;
        for (key, value) in pairs {
            tx.execute(
                "INSERT INTO settings(key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// True once the household password has been set (first-run setup done).
    pub fn is_set_up(&self) -> Result<bool> {
        Ok(self.get_setting("household_password_hash")?.is_some())
    }

    // ---- devices --------------------------------------------------------

    pub fn insert_device(&self, dev: &Device, token_hash: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO devices(id, name, platform, token_hash, created_at, last_seen)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![dev.id, dev.name, dev.platform, token_hash, dev.created_at, dev.last_seen],
        )?;
        Ok(())
    }

    pub fn list_devices(&self) -> Result<Vec<Device>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, name, platform, created_at, last_seen FROM devices ORDER BY created_at",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Device {
                id: r.get(0)?,
                name: r.get(1)?,
                platform: r.get(2)?,
                created_at: r.get(3)?,
                last_seen: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Find the device that owns this token hash and stamp `last_seen`.
    pub fn device_by_token_hash(&self, token_hash: &str, now: &str) -> Result<Option<Device>> {
        let conn = self.lock();
        let dev = conn
            .query_row(
                "SELECT id, name, platform, created_at, last_seen FROM devices WHERE token_hash = ?1",
                params![token_hash],
                |r| {
                    Ok(Device {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        platform: r.get(2)?,
                        created_at: r.get(3)?,
                        last_seen: r.get(4)?,
                    })
                },
            )
            .optional()?;
        // Write "last seen" at most about once a minute, not on every request
        // (an article can pull dozens of images).
        if let Some(d) = &dev {
            let stale = d.last_seen.as_deref().is_none_or(|seen| {
                let fmt = &time::format_description::well_known::Rfc3339;
                match (time::OffsetDateTime::parse(seen, fmt), time::OffsetDateTime::parse(now, fmt)) {
                    (Ok(a), Ok(b)) => (b - a).whole_seconds() >= 60,
                    _ => true,
                }
            });
            if stale {
                conn.execute("UPDATE devices SET last_seen = ?1 WHERE id = ?2", params![now, d.id])?;
            }
        }
        Ok(dev)
    }

    pub fn rename_device(&self, id: &str, name: &str) -> Result<bool> {
        let conn = self.lock();
        Ok(conn.execute("UPDATE devices SET name = ?1 WHERE id = ?2", params![name, id])? > 0)
    }

    /// Remove a paired device, and with it its saved conversations.
    pub fn delete_device(&self, id: &str) -> Result<bool> {
        let conn = self.lock();
        let tx = conn.unchecked_transaction()?;
        let removed = tx.execute("DELETE FROM devices WHERE id = ?1", params![id])? > 0;
        if removed {
            crate::conversations::delete_owned(&tx, id)?;
        }
        tx.commit()?;
        Ok(removed)
    }

    pub fn count_devices(&self) -> Result<i64> {
        let conn = self.lock();
        Ok(conn.query_row("SELECT COUNT(*) FROM devices", [], |r| r.get(0))?)
    }

    // ---- about the database itself (Settings › About) -------------------

    /// The one-time data migrations done on this database, by name
    /// ("batches_v1"), each marked by a `migration_<name>` setting. The
    /// schema itself has no version number: its tables, indexes and columns
    /// are made on every start where they are missing (see [`Db::open`]).
    pub fn migrations_done(&self) -> Result<Vec<String>> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT substr(key, 11) FROM settings WHERE key LIKE 'migration\\_%' ESCAPE '\\' ORDER BY key")?;
        let names = stmt.query_map([], |r| r.get(0))?.collect::<std::result::Result<Vec<String>, _>>()?;
        Ok(names)
    }

    /// Bytes on disk: the database file and its write-ahead log (0 for a
    /// database in memory).
    pub fn size_on_disk(&self) -> u64 {
        let Some(path) = &self.path else { return 0 };
        let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        let mut wal = path.as_os_str().to_owned();
        wal.push("-wal");
        size(path) + size(Path::new(&wal))
    }
}

/// The version of SQLite built into Zaklon ("3.46.0").
pub fn sqlite_version() -> &'static str {
    rusqlite::version()
}

/// One setting read straight from the household database file at `path`,
/// which must exist. Nothing else runs on the file (no schema, no
/// migrations); it is opened for writing only so that SQLite can finish a
/// write the hub left halfway.
pub fn setting_in(path: &Path, key: &str) -> Result<Option<String>> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .with_context(|| format!("opening {}", path.display()))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(conn
        .query_row("SELECT value FROM settings WHERE key = ?1", params![key], |r| r.get(0))
        .optional()?)
}

// ---- restoring a backup -------------------------------------------------

/// Where a restored database takes who may connect from (see [`restore_into`]).
#[derive(Debug, Clone, Copy)]
pub enum RestoreAccess<'a> {
    /// The paired devices, and the settings named in `settings`, come from
    /// the database at `current` (this hub's own); everything else comes
    /// from the backup.
    Keep { current: &'a Path, settings: &'a [&'a str] },
    /// Everything comes from the backup, the devices and every setting too.
    Take,
}

/// Check a household database that came from elsewhere (a backup) and bring
/// it up to date in place, so that [`restore_into`] can copy its rows.
///
/// Only tables and indexes are accepted. A trigger or a view is SQL chosen
/// by whoever wrote the file, run whenever a table is written or read, and a
/// household database never has one: a file with any, or a damaged file, is
/// refused before anything writes to it. Then the same schema and
/// migrations as [`Db::open`] run on it; with no triggers, they change
/// nothing but that file.
pub fn prepare_foreign(path: &Path) -> Result<()> {
    {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
            .with_context(|| format!("opening {}", path.display()))?;
        conn.pragma_update(None, "trusted_schema", "OFF")?;
        only_tables_and_indexes(&conn, "main")?;
        let check: String = conn.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?;
        if check != "ok" {
            bail!("the database is damaged: {check}");
        }
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .with_context(|| format!("opening {}", path.display()))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.pragma_update(None, "trusted_schema", "OFF")?;
    // A plain rollback journal, so that the file can be attached read-only.
    conn.pragma_update(None, "journal_mode", "DELETE")?;
    conn.execute_batch(SCHEMA).context("applying schema")?;
    conn.execute_batch(crate::supplies::SCHEMA).context("applying supplies schema")?;
    conn.execute_batch(crate::memory::SCHEMA).context("applying memory schema")?;
    conn.execute_batch(crate::conversations::SCHEMA).context("applying conversations schema")?;
    crate::supplies::migrate(&conn).context("migrating supplies")?;
    Ok(())
}

/// Refuses a database whose schema `schema` holds anything but tables and
/// indexes: no triggers, no views, no virtual tables.
fn only_tables_and_indexes(conn: &Connection, schema: &str) -> Result<()> {
    let mut stmt = conn.prepare(&format!("SELECT type, name, coalesce(sql, '') FROM {schema}.sqlite_master"))?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?;
    for row in rows {
        let (kind, name, sql) = row?;
        let is_virtual = sql.split_whitespace().nth(1).is_some_and(|w| w.eq_ignore_ascii_case("VIRTUAL"));
        if !(kind == "index" || (kind == "table" && !is_virtual)) {
            bail!("it holds a {kind} named {name:?}, which a household database never has");
        }
    }
    Ok(())
}

/// Build a restored household database in the file `target` (replaced if
/// it exists): this version's own schema, filled with the rows of the
/// backup's database at `backup` (prepared with [`prepare_foreign`]), with
/// who may connect as `access` says. All or nothing.
///
/// Nothing of the backup's schema is kept or run. The backup is attached
/// read-only, and its rows are copied into the tables this version made,
/// with the column lists spelled out (the columns both have; any other gets
/// its default). A trigger belongs to its own database and never fires on
/// a read, so even one that got past [`prepare_foreign`] changes nothing.
pub fn restore_into(target: &Path, backup: &Path, access: RestoreAccess<'_>) -> Result<()> {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut name = target.as_os_str().to_os_string();
        name.push(suffix);
        let _ = std::fs::remove_file(PathBuf::from(name));
    }
    // This version's schema, empty.
    drop(Db::open(target)?);
    let conn = Connection::open(target).with_context(|| format!("opening {}", target.display()))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.pragma_update(None, "trusted_schema", "OFF")?;
    // The rows go in as the backup has them, whatever order they come in.
    conn.pragma_update(None, "foreign_keys", "OFF")?;
    conn.execute("ATTACH DATABASE ?1 AS backup", params![read_only_uri(backup)?])
        .with_context(|| format!("opening {}", backup.display()))?;
    if let RestoreAccess::Keep { current, .. } = access {
        // This hub's own database, as it is; only read.
        conn.execute("ATTACH DATABASE ?1 AS current", params![current.to_string_lossy()])
            .with_context(|| format!("opening {}", current.display()))?;
    }
    let tx = conn.unchecked_transaction()?;
    let mut stmt = tx.prepare("SELECT name FROM main.sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite%'")?;
    let tables = stmt.query_map([], |r| r.get(0))?.collect::<std::result::Result<Vec<String>, _>>()?;
    drop(stmt);
    for table in &tables {
        tx.execute(&format!("DELETE FROM main.{}", quoted(table)), [])?;
        match (table.as_str(), access) {
            ("devices", RestoreAccess::Keep { .. }) => copy_rows(&tx, table, "current", "", &[])?,
            ("settings", RestoreAccess::Keep { settings, .. }) => {
                let keys = (1..=settings.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(", ");
                copy_rows(&tx, table, "backup", &format!("WHERE key NOT IN ({keys})"), settings)?;
                copy_rows(&tx, table, "current", &format!("WHERE key IN ({keys})"), settings)?;
            }
            _ => copy_rows(&tx, table, "backup", "", &[])?,
        }
    }
    // Conversations belong to a device: those of devices that may not
    // connect after this restore go.
    crate::conversations::drop_orphans(&tx)?;
    tx.commit()?;
    Ok(())
}

/// Copy the rows of `table` from the attached database `from` into `main`:
/// the columns both have, named one by one (the names are this version's).
/// A table `from` does not have copies nothing.
fn copy_rows(conn: &Connection, table: &str, from: &str, filter: &str, values: &[&str]) -> Result<()> {
    let theirs = columns(conn, from, table)?;
    let shared: Vec<String> = columns(conn, "main", table)?
        .into_iter()
        .filter(|c| theirs.iter().any(|t| t.eq_ignore_ascii_case(c)))
        .map(|c| quoted(&c))
        .collect();
    if shared.is_empty() {
        return Ok(());
    }
    let (list, t) = (shared.join(", "), quoted(table));
    conn.execute(&format!("INSERT INTO main.{t} ({list}) SELECT {list} FROM {from}.{t} {filter}"), rusqlite::params_from_iter(values))
        .with_context(|| format!("copying {table}"))?;
    Ok(())
}

fn columns(conn: &Connection, schema: &str, table: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT name FROM pragma_table_info(?1, ?2)")?;
    let names = stmt.query_map(params![table, schema], |r| r.get(0))?.collect::<std::result::Result<Vec<String>, _>>()?;
    Ok(names)
}

fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// `path` as an SQLite URI that opens the file read-only.
fn read_only_uri(path: &Path) -> Result<String> {
    let text = std::path::absolute(path)?.to_string_lossy().replace('\\', "/");
    // "file:///C:/..." on Windows, "file:///home/..." elsewhere.
    let mut uri = String::from(if text.starts_with('/') { "file://" } else { "file:///" });
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/:".contains(&b) {
            uri.push(b as char);
        } else {
            uri.push_str(&format!("%{b:02X}"));
        }
    }
    uri.push_str("?mode=ro");
    Ok(uri)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_database_tells_its_migrations_size_and_sqlite() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.migrations_done().unwrap(), ["batches_v1"]);
        // Only settings named migration_<name> count ("migrationX" is not one).
        db.set_setting("migrationX", "1").unwrap();
        db.set_setting("migration_aaa", "1").unwrap();
        assert_eq!(db.migrations_done().unwrap(), ["aaa", "batches_v1"]);
        assert_eq!(db.size_on_disk(), 0);
        let dir = std::env::temp_dir().join(format!("zaklon-db-size-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = Db::open(&dir.join("household.db")).unwrap();
        assert!(file.size_on_disk() > 0);
        drop(file);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(sqlite_version().starts_with("3."), "{}", sqlite_version());
    }

    #[test]
    fn devices_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        assert!(!db.is_set_up().unwrap());
        let dev = Device {
            id: "d1".into(),
            name: "Phone".into(),
            platform: "android".into(),
            created_at: now_rfc3339(),
            last_seen: None,
        };
        db.insert_device(&dev, "hash").unwrap();
        assert_eq!(db.count_devices().unwrap(), 1);
        let found = db.device_by_token_hash("hash", "2026-01-01T00:00:00Z").unwrap().unwrap();
        assert_eq!(found.id, "d1");
        assert!(db.delete_device("d1").unwrap());
        assert_eq!(db.count_devices().unwrap(), 0);
    }

    #[test]
    fn settings_written_together() {
        let db = Db::open_in_memory().unwrap();
        db.set_settings(&[("a", "1"), ("b", "2")]).unwrap();
        db.set_settings(&[("b", "3")]).unwrap();
        assert_eq!(db.get_setting("a").unwrap().as_deref(), Some("1"));
        assert_eq!(db.get_setting("b").unwrap().as_deref(), Some("3"));
    }

    fn device(id: &str) -> Device {
        Device { id: id.into(), name: id.into(), platform: "android".into(), created_at: now_rfc3339(), last_seen: None }
    }

    fn device_ids(db: &Db) -> Vec<String> {
        db.list_devices().unwrap().into_iter().map(|d| d.id).collect()
    }

    /// A folder in the system's temporary folder, with a Serbian name (the
    /// data folder often lives under such a Windows account).
    fn temp_folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zaklon-{name}-Đorđe-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_restore_keeps_or_takes_who_may_connect() {
        let dir = temp_folder("restore");
        let backup = dir.join("backup.db");
        let db = Db::open(&backup).unwrap();
        db.insert_device(&device("in-backup"), "in-backup-hash").unwrap();
        db.set_settings(&[("secret", "theirs"), ("plain", "theirs")]).unwrap();
        db.add_note("from the backup", "Ana").unwrap();
        drop(db);
        let current = dir.join("current.db");
        let db = Db::open(&current).unwrap();
        db.insert_device(&device("paired-now"), "paired-now-hash").unwrap();
        db.set_settings(&[("secret", "ours"), ("plain", "ours"), ("missing", "ours")]).unwrap();
        db.add_note("current", "Ana").unwrap();
        drop(db);
        prepare_foreign(&backup).unwrap();

        let kept = dir.join("kept.db");
        restore_into(&kept, &backup, RestoreAccess::Keep { current: &current, settings: &["secret", "missing"] }).unwrap();
        let db = Db::open(&kept).unwrap();
        assert_eq!(device_ids(&db), ["paired-now"]);
        assert!(db.device_by_token_hash("in-backup-hash", &now_rfc3339()).unwrap().is_none());
        assert_eq!(db.get_setting("secret").unwrap().as_deref(), Some("ours"));
        assert_eq!(db.get_setting("missing").unwrap().as_deref(), Some("ours"));
        assert_eq!(db.get_setting("plain").unwrap().as_deref(), Some("theirs"), "the other settings come from the backup");
        let notes: Vec<String> = db.list_notes().unwrap().into_iter().map(|n| n.text).collect();
        assert_eq!(notes, ["from the backup"]);
        drop(db);

        let taken = dir.join("taken.db");
        restore_into(&taken, &backup, RestoreAccess::Take).unwrap();
        let db = Db::open(&taken).unwrap();
        assert_eq!(device_ids(&db), ["in-backup"]);
        assert_eq!(db.get_setting("secret").unwrap().as_deref(), Some("theirs"));
        assert_eq!(db.get_setting("missing").unwrap(), None, "what the backup lacks is not there");
        drop(db);
        // The current database was only read.
        let db = Db::open(&current).unwrap();
        assert_eq!(device_ids(&db), ["paired-now"]);
        assert_eq!(db.list_notes().unwrap().len(), 1);
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Conversations come back with the backup, but only those of the
    /// laptop and of the phones that may connect after the restore.
    #[test]
    fn a_restore_brings_back_the_conversations_of_devices_that_may_connect() {
        use crate::conversations::LAPTOP;
        let dir = temp_folder("restore-conversations");
        let backup = dir.join("backup.db");
        let db = Db::open(&backup).unwrap();
        db.insert_device(&device("still-paired"), "still-paired-hash").unwrap();
        db.insert_device(&device("gone-since"), "gone-since-hash").unwrap();
        db.add_turn(LAPTOP, None, "The laptop's", "a1").unwrap();
        db.add_turn("still-paired", None, "Still paired", "a2").unwrap();
        db.add_turn("gone-since", None, "Gone since", "a3").unwrap();
        drop(db);
        let current = dir.join("current.db");
        let db = Db::open(&current).unwrap();
        db.insert_device(&device("still-paired"), "still-paired-hash").unwrap();
        db.add_turn(LAPTOP, None, "Asked after the backup", "a4").unwrap();
        drop(db);
        prepare_foreign(&backup).unwrap();
        let titles = |db: &Db, owner: &str| db.list_conversations(owner, None).unwrap().into_iter().map(|c| c.title).collect::<Vec<_>>();

        let kept = dir.join("kept.db");
        restore_into(&kept, &backup, RestoreAccess::Keep { current: &current, settings: &["household_password_hash"] }).unwrap();
        let db = Db::open(&kept).unwrap();
        assert_eq!(titles(&db, LAPTOP), ["The laptop's"]);
        assert_eq!(titles(&db, "still-paired"), ["Still paired"]);
        assert!(titles(&db, "gone-since").is_empty(), "that phone may not connect, so its conversations are not kept");
        let turns: i64 = db.lock().query_row("SELECT COUNT(*) FROM conversation_turns", [], |r| r.get(0)).unwrap();
        assert_eq!(turns, 2, "no turns are left without their conversation");
        drop(db);

        let taken = dir.join("taken.db");
        restore_into(&taken, &backup, RestoreAccess::Take).unwrap();
        let db = Db::open(&taken).unwrap();
        assert_eq!(titles(&db, "gone-since"), ["Gone since"], "the backup's phones come with it");
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A backup's database from before a column or a table existed still
    /// restores; what it lacks gets its default.
    #[test]
    fn an_older_backup_database_restores() {
        let dir = temp_folder("restore-old");
        let backup = dir.join("backup.db");
        let conn = Connection::open(&backup).unwrap();
        conn.execute_batch(
            "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE devices (id TEXT PRIMARY KEY, name TEXT NOT NULL, platform TEXT NOT NULL, token_hash TEXT NOT NULL, created_at TEXT NOT NULL, last_seen TEXT);
             CREATE TABLE items (id TEXT PRIMARY KEY, name TEXT NOT NULL, quantity REAL NOT NULL DEFAULT 0,
               unit TEXT NOT NULL DEFAULT 'pcs', category TEXT NOT NULL DEFAULT 'other', place TEXT, expiry TEXT,
               barcode TEXT, min_quantity REAL, notes TEXT, updated_at TEXT NOT NULL, updated_by TEXT, deleted INTEGER NOT NULL DEFAULT 0);
             INSERT INTO items (id, name, quantity, expiry, updated_at) VALUES ('a', 'Brašno', 2, '2027-01-31', 'x');
             CREATE TABLE shopping (id TEXT PRIMARY KEY, item_id TEXT, text TEXT NOT NULL, quantity REAL, unit TEXT,
               done INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, updated_by TEXT);
             INSERT INTO shopping (id, text, done, created_at, updated_at) VALUES ('s1', 'Hleb', 1, 'x', 'x');",
        )
        .unwrap();
        drop(conn);
        prepare_foreign(&backup).unwrap();
        let target = dir.join("restored.db");
        restore_into(&target, &backup, RestoreAccess::Take).unwrap();
        let db = Db::open(&target).unwrap();
        let item = db.get_item("a").unwrap().unwrap();
        assert_eq!(item.batches.len(), 1, "migrated as an old database is");
        assert_eq!(db.to_put_away().unwrap()[0].text, "Hleb");
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The security review's proof of concept: triggers in a backup's
    /// database that add a phone and replace the password when the hub's
    /// own phones and password are written into it, and bring that phone
    /// back when it is removed.
    const HOSTILE: &str = "
        CREATE TRIGGER inject AFTER INSERT ON settings WHEN NEW.key = 'household_password_hash'
        BEGIN
          INSERT OR IGNORE INTO devices VALUES ('evil', 'Phone', 'android', 'ATTACKER-TOKEN-HASH', 't', NULL);
          UPDATE settings SET value = 'ATTACKER-HASH' WHERE key = 'household_password_hash';
        END;
        CREATE TRIGGER sticky AFTER DELETE ON devices WHEN OLD.id = 'evil'
        BEGIN
          INSERT OR IGNORE INTO devices VALUES ('evil', 'Phone', 'android', 'ATTACKER-TOKEN-HASH', 't', NULL);
        END;
        CREATE TRIGGER on_devices AFTER INSERT ON devices
        BEGIN
          INSERT OR IGNORE INTO devices VALUES ('evil', 'Phone', 'android', 'ATTACKER-TOKEN-HASH', 't', NULL);
        END;";

    #[test]
    fn a_backup_database_with_triggers_is_refused_and_its_triggers_never_run() {
        let dir = temp_folder("restore-hostile");
        let backup = dir.join("backup.db");
        let db = Db::open(&backup).unwrap();
        db.insert_device(&device("in-backup"), "in-backup-hash").unwrap();
        db.set_setting("household_password_hash", "BACKUP-HASH").unwrap();
        drop(db);
        let conn = Connection::open(&backup).unwrap();
        conn.execute_batch(HOSTILE).unwrap();
        // As `prepare_foreign` would have left it, had it let the file through.
        conn.pragma_update(None, "journal_mode", "DELETE").unwrap();
        drop(conn);
        let err = prepare_foreign(&backup).unwrap_err().to_string();
        assert!(err.contains("trigger"), "{err}");

        let current = dir.join("current.db");
        let db = Db::open(&current).unwrap();
        db.insert_device(&device("paired-now"), "paired-now-hash").unwrap();
        db.set_setting("household_password_hash", "OUR-HASH").unwrap();
        drop(db);
        // Even past the check, a restore only reads the backup: its triggers
        // never run, and none is kept.
        let target = dir.join("restored.db");
        restore_into(&target, &backup, RestoreAccess::Keep { current: &current, settings: &["household_password_hash"] }).unwrap();
        let db = Db::open(&target).unwrap();
        assert_eq!(device_ids(&db), ["paired-now"]);
        assert_eq!(db.get_setting("household_password_hash").unwrap().as_deref(), Some("OUR-HASH"));
        assert!(db.device_by_token_hash("ATTACKER-TOKEN-HASH", &now_rfc3339()).unwrap().is_none());
        let code: i64 = db.lock().query_row("SELECT count(*) FROM sqlite_master WHERE type IN ('trigger', 'view')", [], |r| r.get(0)).unwrap();
        assert_eq!(code, 0);
        // A phone removed stays removed.
        assert!(db.delete_device("paired-now").unwrap());
        assert_eq!(db.count_devices().unwrap(), 0);
        drop(db);
        // The same for a restore that takes the backup's phones: its rows, not its code.
        let taken = dir.join("taken.db");
        restore_into(&taken, &backup, RestoreAccess::Take).unwrap();
        let db = Db::open(&taken).unwrap();
        assert_eq!(device_ids(&db), ["in-backup"]);
        assert_eq!(db.get_setting("household_password_hash").unwrap().as_deref(), Some("BACKUP-HASH"));
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_backup_database_with_a_view_is_refused() {
        let dir = temp_folder("restore-view");
        let backup = dir.join("backup.db");
        drop(Db::open(&backup).unwrap());
        let conn = Connection::open(&backup).unwrap();
        conn.execute_batch("CREATE VIEW everyone AS SELECT id FROM devices;").unwrap();
        drop(conn);
        let err = prepare_foreign(&backup).unwrap_err().to_string();
        assert!(err.contains("view"), "{err}");
        // Not a database at all.
        std::fs::write(dir.join("junk.db"), b"not a database, only some text that is long enough").unwrap();
        assert!(prepare_foreign(&dir.join("junk.db")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_setting_is_read_from_a_database_file() {
        let dir = temp_folder("setting-in");
        let db = Db::open(&dir.join("household.db")).unwrap();
        db.set_setting("household_password_hash", "hash").unwrap();
        assert_eq!(setting_in(&dir.join("household.db"), "household_password_hash").unwrap().as_deref(), Some("hash"));
        assert_eq!(setting_in(&dir.join("household.db"), "other").unwrap(), None);
        drop(db);
        assert!(setting_in(&dir.join("missing.db"), "household_password_hash").is_err());
        assert!(!dir.join("missing.db").exists(), "nothing is created");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_snapshot_does_not_wait_for_the_shared_connection() {
        let dir = std::env::temp_dir().join(format!("zaklon-snapshot-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::open(&dir.join("household.db")).unwrap();
        db.set_setting("marker", "kept").unwrap();
        // A request busy with the database while a backup copies it.
        let busy = db.lock();
        db.snapshot_to(&dir.join("copy.db")).unwrap();
        drop(busy);
        let copy = Db::open(&dir.join("copy.db")).unwrap();
        assert_eq!(copy.get_setting("marker").unwrap().as_deref(), Some("kept"));
        drop((db, copy));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
