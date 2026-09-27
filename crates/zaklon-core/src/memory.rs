//! What the household asked the assistant to remember ("Ana is allergic to
//! penicillin"). Shared by the household, like the supplies, and visible to
//! everyone in the Assistant screen. The assistant reads the notes that
//! matter for a question.

use anyhow::{bail, Result};
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::Db;

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS memory (
    id TEXT PRIMARY KEY,
    text TEXT NOT NULL,
    created_at TEXT NOT NULL,
    created_by TEXT
);
"#;

/// Longest note, and how many the household can keep.
pub const MAX_TEXT: usize = 300;
pub const MAX_NOTES: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Note {
    pub id: String,
    pub text: String,
    pub created_at: String,
    pub created_by: Option<String>,
}

impl Db {
    pub fn list_notes(&self) -> Result<Vec<Note>> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT id, text, created_at, created_by FROM memory ORDER BY created_at DESC")?;
        let rows = stmt.query_map([], |r| Ok(Note { id: r.get(0)?, text: r.get(1)?, created_at: r.get(2)?, created_by: r.get(3)? }))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn add_note(&self, text: &str, actor: &str) -> Result<Note> {
        let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            bail!("text is required");
        }
        if text.chars().count() > MAX_TEXT {
            bail!("the note is too long");
        }
        let conn = self.lock();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM memory", [], |r| r.get(0))?;
        if count as usize >= MAX_NOTES {
            bail!("the assistant remembers too much already; delete some notes first");
        }
        let note = Note { id: uuid::Uuid::new_v4().to_string(), text, created_at: crate::db::now_rfc3339(), created_by: Some(actor.to_string()) };
        conn.execute(
            "INSERT INTO memory (id, text, created_at, created_by) VALUES (?1, ?2, ?3, ?4)",
            params![note.id, note.text, note.created_at, note.created_by],
        )?;
        Ok(note)
    }

    pub fn delete_note(&self, id: &str, actor: &str) -> Result<bool> {
        let conn = self.lock();
        let n = conn.execute("DELETE FROM memory WHERE id = ?1", [id])?;
        if n > 0 {
            tracing::info!(by = %actor, "assistant note deleted");
        }
        Ok(n > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_are_kept_and_deleted() {
        let db = Db::open_in_memory().unwrap();
        let n = db.add_note("  Ana je   alergična na penicilin ", "laptop").unwrap();
        assert_eq!(n.text, "Ana je alergična na penicilin");
        assert_eq!(db.list_notes().unwrap().len(), 1);
        assert!(db.add_note("   ", "laptop").is_err());
        assert!(db.add_note(&"x".repeat(MAX_TEXT + 1), "laptop").is_err());
        assert!(db.delete_note(&n.id, "laptop").unwrap());
        assert!(!db.delete_note(&n.id, "laptop").unwrap());
        assert!(db.list_notes().unwrap().is_empty());
    }
}
