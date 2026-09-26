//! SQLite storage for the household database. One connection guarded by a
//! mutex is plenty for a household; WAL mode keeps reads cheap.

use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

pub struct Db {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub created_at: String,
    pub last_seen: Option<String>,
}

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
CREATE TABLE IF NOT EXISTS profiles (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    avatar TEXT,
    language TEXT NOT NULL DEFAULT 'en',
    accent TEXT NOT NULL DEFAULT 'green',
    password_hash TEXT,
    created_at TEXT NOT NULL
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
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA).context("applying schema")?;
        conn.execute_batch(crate::supplies::SCHEMA).context("applying supplies schema")?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        conn.execute_batch(crate::supplies::SCHEMA)?;
        Ok(Self { conn: Mutex::new(conn) })
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
        if let Some(d) = &dev {
            conn.execute("UPDATE devices SET last_seen = ?1 WHERE id = ?2", params![now, d.id])?;
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

/// RFC 3339 timestamp in UTC, second precision.
pub fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
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
}
