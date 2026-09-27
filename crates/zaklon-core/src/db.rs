//! SQLite storage for the household database. One connection guarded by a
//! mutex is plenty for a household; WAL mode keeps reads cheap.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
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
        crate::supplies::migrate(&conn).context("migrating supplies")?;
        Ok(Self { conn: Mutex::new(conn), path: Some(path.to_path_buf()) })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        conn.execute_batch(crate::supplies::SCHEMA)?;
        conn.execute_batch(crate::memory::SCHEMA)?;
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

    pub fn delete_device(&self, id: &str) -> Result<bool> {
        let conn = self.lock();
        Ok(conn.execute("DELETE FROM devices WHERE id = ?1", params![id])? > 0)
    }

    pub fn count_devices(&self) -> Result<i64> {
        let conn = self.lock();
        Ok(conn.query_row("SELECT COUNT(*) FROM devices", [], |r| r.get(0))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
