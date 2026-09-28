//! Saved conversations with the assistant. Each one belongs to the device
//! that had it, its owner: [`LAPTOP`] for the laptop itself, a paired
//! phone's device id for that phone. Only the owner sees, renames or
//! deletes it. A copy can be sent to another device of the household, where
//! it shows as a conversation of its own, marked with the sender's name.
//!
//! A turn is saved when the question is asked (as "pending", with the id of
//! the answer being written) and completed when the answer is finished, so
//! a conversation opened on another screen or after closing the app shows
//! the answer that is still being written.

use anyhow::{bail, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use crate::db::{now_rfc3339, Db};
use crate::translit::fold_loose;

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS conversations (
    id TEXT PRIMARY KEY,
    owner TEXT NOT NULL,
    title TEXT NOT NULL,
    from_owner TEXT,
    from_name TEXT,
    search TEXT NOT NULL DEFAULT '',
    touched INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS conversations_owner ON conversations(owner, touched);
CREATE TABLE IF NOT EXISTS conversation_turns (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    seq INTEGER NOT NULL,
    question TEXT NOT NULL,
    answer TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'pending',
    error TEXT,
    answer_id TEXT,
    sources TEXT NOT NULL DEFAULT '[]',
    details TEXT NOT NULL DEFAULT '{}',
    outcome TEXT,
    created_at TEXT NOT NULL,
    answered_at TEXT
);
CREATE INDEX IF NOT EXISTS conversation_turns_conversation ON conversation_turns(conversation_id, seq);
CREATE INDEX IF NOT EXISTS conversation_turns_answer ON conversation_turns(answer_id);
"#;

/// The owner of the laptop's own conversations. Device ids are UUIDs, and a
/// phone may not be named "laptop", so it never stands for a phone.
pub const LAPTOP: &str = "laptop";
/// How many conversations a device keeps; a new one beyond this pushes out
/// the one used longest ago.
pub const MAX_CONVERSATIONS: usize = 500;
/// How many questions one conversation holds; then a new one is started.
pub const MAX_TURNS: usize = 200;
pub const MAX_TITLE: usize = 80;
/// Titles made from the first question are about this long.
const AUTO_TITLE: usize = 48;
/// The longest question the assistant takes (as in the hub's `ask`).
pub const MAX_QUESTION: usize = 2000;
/// Longer answers are cut here when saved (answers are far shorter).
pub const MAX_ANSWER: usize = 20_000;
const MAX_ERROR: usize = 500;
/// The sources and the details of an answer, as JSON; more is not kept.
const MAX_SOURCES: usize = 16_000;
const MAX_DETAILS: usize = 8_000;
/// Searchable text kept per conversation: the title and the questions.
const SEARCH_CHARS: usize = 20_000;
/// Words of a search that must all be found.
const SEARCH_WORDS: usize = 6;

/// A conversation in the list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Summary {
    pub id: String,
    /// Empty until the first question gives it one (the app shows "New conversation").
    pub title: String,
    /// A copy sent from another device: who sent it ([`LAPTOP`] or a
    /// device id) and the sender's name at the time.
    pub from_owner: Option<String>,
    pub from_name: Option<String>,
    pub created_at: String,
    /// When a question was last asked in it (or it was renamed or sent here).
    pub updated_at: String,
    pub turns: usize,
}

/// One question and its answer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Turn {
    pub id: String,
    pub question: String,
    pub answer: String,
    /// "pending" while the answer is being written, then "done" or "failed".
    pub status: String,
    pub error: Option<String>,
    /// The assistant's id for the answer, to follow one still being written.
    pub answer_id: Option<String>,
    /// The answer's sources, as the assistant gave them (a JSON array).
    pub sources: serde_json::Value,
    /// The rest of what the app shows with an answer (a JSON object).
    pub details: serde_json::Value,
    /// What happened to a proposed supplies change: "done" or "canceled".
    pub outcome: Option<String>,
    pub created_at: String,
    pub answered_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Conversation {
    #[serde(flatten)]
    pub summary: Summary,
    pub turns: Vec<Turn>,
}

/// A finished answer, as it is saved.
#[derive(Debug, Clone, Default)]
pub struct Finished {
    pub failed: bool,
    pub answer: String,
    pub error: Option<String>,
    pub sources: serde_json::Value,
    pub details: serde_json::Value,
}

/// A short title from the first question: its first words.
pub fn title_from(question: &str) -> String {
    let mut out = String::new();
    for word in question.split_whitespace() {
        let len = out.chars().count() + usize::from(!out.is_empty()) + word.chars().count();
        if len > AUTO_TITLE {
            if out.is_empty() {
                out = word.chars().take(AUTO_TITLE).collect();
            }
            out.push('…');
            return out;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

/// A title as typed: on one line, trimmed, not too long; never empty.
fn clean_title(title: &str) -> Result<String> {
    let title: String = title.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(MAX_TITLE).collect();
    if title.is_empty() {
        bail!("name is required");
    }
    Ok(title)
}

fn clip(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((at, _)) => text[..at].to_string(),
        None => text.to_string(),
    }
}

/// `value` as JSON text, or `empty` when it is larger than `max` bytes.
fn json_within(value: &serde_json::Value, max: usize, empty: &str) -> String {
    let text = value.to_string();
    if text.len() > max || value.is_null() {
        empty.to_string()
    } else {
        text
    }
}

const SUMMARY: &str = "SELECT c.id, c.title, c.from_owner, c.from_name, c.created_at, c.updated_at,
        (SELECT COUNT(*) FROM conversation_turns t WHERE t.conversation_id = c.id)
    FROM conversations c";

fn summary_row(r: &Row<'_>) -> rusqlite::Result<Summary> {
    Ok(Summary {
        id: r.get(0)?,
        title: r.get(1)?,
        from_owner: r.get(2)?,
        from_name: r.get(3)?,
        created_at: r.get(4)?,
        updated_at: r.get(5)?,
        turns: r.get::<_, i64>(6)? as usize,
    })
}

fn turn_row(r: &Row<'_>) -> rusqlite::Result<Turn> {
    let json = |i: usize, empty: &str| -> rusqlite::Result<serde_json::Value> {
        let text: String = r.get(i)?;
        Ok(serde_json::from_str(&text).unwrap_or_else(|_| serde_json::from_str(empty).unwrap_or_default()))
    };
    Ok(Turn {
        id: r.get(0)?,
        question: r.get(1)?,
        answer: r.get(2)?,
        status: r.get(3)?,
        error: r.get(4)?,
        answer_id: r.get(5)?,
        sources: json(6, "[]")?,
        details: json(7, "{}")?,
        outcome: r.get(8)?,
        created_at: r.get(9)?,
        answered_at: r.get(10)?,
    })
}

fn summary(conn: &Connection, owner: &str, id: &str) -> Result<Option<Summary>> {
    Ok(conn.query_row(&format!("{SUMMARY} WHERE c.owner = ?1 AND c.id = ?2"), params![owner, id], summary_row).optional()?)
}

/// Mark the conversation as the one used last.
fn touch(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE conversations SET updated_at = ?2, touched = (SELECT coalesce(max(touched), 0) + 1 FROM conversations) WHERE id = ?1",
        params![id, now_rfc3339()],
    )?;
    Ok(())
}

/// Rebuild what a search looks in: the title and the questions, folded so
/// that "caj", "čaj" and "чај" find each other.
fn refresh_search(conn: &Connection, id: &str) -> Result<()> {
    let title: String = conn.query_row("SELECT title FROM conversations WHERE id = ?1", [id], |r| r.get(0))?;
    let mut text = fold_loose(&title);
    let mut stmt = conn.prepare("SELECT question FROM conversation_turns WHERE conversation_id = ?1 ORDER BY seq")?;
    let questions = stmt.query_map([id], |r| r.get::<_, String>(0))?;
    for q in questions {
        if text.chars().count() >= SEARCH_CHARS {
            break;
        }
        text.push('\n');
        text.push_str(&fold_loose(&q?));
    }
    conn.execute("UPDATE conversations SET search = ?2 WHERE id = ?1", params![id, clip(&text, SEARCH_CHARS)])?;
    Ok(())
}

fn insert_conversation(conn: &Connection, owner: &str, title: &str, from: Option<(&str, &str)>) -> Result<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO conversations (id, owner, title, from_owner, from_name, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        params![id, owner, title, from.map(|f| f.0), from.map(|f| f.1), now],
    )?;
    touch(conn, &id)?;
    prune(conn, owner)?;
    Ok(id)
}

/// Keep the newest [`MAX_CONVERSATIONS`] of an owner.
fn prune(conn: &Connection, owner: &str) -> Result<()> {
    let mut stmt = conn.prepare("SELECT id FROM conversations WHERE owner = ?1 ORDER BY touched DESC LIMIT -1 OFFSET ?2")?;
    let old = stmt.query_map(params![owner, MAX_CONVERSATIONS as i64], |r| r.get::<_, String>(0))?.collect::<std::result::Result<Vec<_>, _>>()?;
    for id in old {
        delete(conn, &id)?;
    }
    Ok(())
}

fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM conversation_turns WHERE conversation_id = ?1", [id])?;
    conn.execute("DELETE FROM conversations WHERE id = ?1", [id])?;
    Ok(())
}

/// Delete every conversation of `owner` (a removed phone).
pub(crate) fn delete_owned(conn: &Connection, owner: &str) -> Result<()> {
    conn.execute("DELETE FROM conversation_turns WHERE conversation_id IN (SELECT id FROM conversations WHERE owner = ?1)", [owner])?;
    conn.execute("DELETE FROM conversations WHERE owner = ?1", [owner])?;
    Ok(())
}

/// After a restore: conversations only of the laptop and of the phones
/// that may connect, and no turns without their conversation.
pub(crate) fn drop_orphans(conn: &Connection) -> Result<()> {
    conn.execute(
        "DELETE FROM main.conversations WHERE owner <> ?1 AND owner NOT IN (SELECT id FROM main.devices)",
        [LAPTOP],
    )?;
    conn.execute("DELETE FROM main.conversation_turns WHERE conversation_id NOT IN (SELECT id FROM main.conversations)", [])?;
    Ok(())
}

impl Db {
    /// The conversations of `owner`, the one used last first. With a
    /// `query`, only those whose title or questions hold all its words.
    pub fn list_conversations(&self, owner: &str, query: Option<&str>) -> Result<Vec<Summary>> {
        let words: Vec<String> = query
            .map(fold_loose)
            .unwrap_or_default()
            .split_whitespace()
            .take(SEARCH_WORDS)
            .map(str::to_string)
            .collect();
        let mut sql = format!("{SUMMARY} WHERE c.owner = ?1");
        for i in 0..words.len() {
            sql.push_str(&format!(" AND instr(c.search, ?{}) > 0", i + 2));
        }
        sql.push_str(" ORDER BY c.touched DESC");
        let conn = self.lock();
        let mut stmt = conn.prepare(&sql)?;
        let values: Vec<&str> = std::iter::once(owner).chain(words.iter().map(String::as_str)).collect();
        let rows = stmt.query_map(rusqlite::params_from_iter(values), summary_row)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// One conversation of `owner` with its turns; `None` when it is not theirs.
    pub fn get_conversation(&self, owner: &str, id: &str) -> Result<Option<Conversation>> {
        let conn = self.lock();
        let Some(summary) = summary(&conn, owner, id)? else {
            return Ok(None);
        };
        let mut stmt = conn.prepare(
            "SELECT id, question, answer, status, error, answer_id, sources, details, outcome, created_at, answered_at
             FROM conversation_turns WHERE conversation_id = ?1 ORDER BY seq",
        )?;
        let turns = stmt.query_map([id], turn_row)?.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(Some(Conversation { summary, turns }))
    }

    /// A new, empty conversation of `owner` (an empty title is set from the first question).
    pub fn create_conversation(&self, owner: &str, title: &str) -> Result<Summary> {
        let title = if title.trim().is_empty() { String::new() } else { clean_title(title)? };
        let conn = self.lock();
        let tx = conn.unchecked_transaction()?;
        let id = insert_conversation(&tx, owner, &title, None)?;
        refresh_search(&tx, &id)?;
        let s = summary(&tx, owner, &id)?.expect("just made");
        tx.commit()?;
        Ok(s)
    }

    /// Save a question as it is asked, in the conversation `conversation` of
    /// `owner`, or in a new one (titled from the question) when that is
    /// `None`. The turn waits as "pending" for [`Db::finish_turn`].
    pub fn add_turn(&self, owner: &str, conversation: Option<&str>, question: &str, answer_id: &str) -> Result<(Summary, Turn)> {
        let question = question.trim();
        if question.is_empty() {
            bail!("ask something first");
        }
        if question.chars().count() > MAX_QUESTION {
            bail!("the question is too long");
        }
        let conn = self.lock();
        let tx = conn.unchecked_transaction()?;
        let id = match conversation {
            Some(id) => {
                let Some(s) = summary(&tx, owner, id)? else {
                    bail!("no such conversation");
                };
                if s.turns >= MAX_TURNS {
                    bail!("the conversation is too long; start a new one");
                }
                if s.title.is_empty() {
                    tx.execute("UPDATE conversations SET title = ?2 WHERE id = ?1", params![id, title_from(question)])?;
                }
                id.to_string()
            }
            None => insert_conversation(&tx, owner, &title_from(question), None)?,
        };
        let now = now_rfc3339();
        let turn = Turn {
            id: uuid::Uuid::new_v4().to_string(),
            question: question.to_string(),
            answer: String::new(),
            status: "pending".into(),
            error: None,
            answer_id: Some(answer_id.to_string()),
            sources: serde_json::json!([]),
            details: serde_json::json!({}),
            outcome: None,
            created_at: now.clone(),
            answered_at: None,
        };
        tx.execute(
            "INSERT INTO conversation_turns (id, conversation_id, seq, question, status, answer_id, created_at)
             VALUES (?1, ?2, (SELECT coalesce(max(seq), 0) + 1 FROM conversation_turns WHERE conversation_id = ?2), ?3, 'pending', ?4, ?5)",
            params![turn.id, id, turn.question, answer_id, now],
        )?;
        touch(&tx, &id)?;
        refresh_search(&tx, &id)?;
        let s = summary(&tx, owner, &id)?.expect("just saved");
        tx.commit()?;
        Ok((s, turn))
    }

    /// Complete the turn waiting for the answer `answer_id`. False when no
    /// turn waits for it (it was finished already, or deleted).
    pub fn finish_turn(&self, answer_id: &str, done: &Finished) -> Result<bool> {
        let conn = self.lock();
        let n = conn.execute(
            "UPDATE conversation_turns SET status = ?2, answer = ?3, error = ?4, sources = ?5, details = ?6, answered_at = ?7
             WHERE answer_id = ?1 AND status = 'pending'",
            params![
                answer_id,
                if done.failed { "failed" } else { "done" },
                clip(&done.answer, MAX_ANSWER),
                done.error.as_deref().map(|e| clip(e, MAX_ERROR)),
                json_within(&done.sources, MAX_SOURCES, "[]"),
                json_within(&done.details, MAX_DETAILS, "{}"),
                now_rfc3339(),
            ],
        )?;
        Ok(n > 0)
    }

    /// Remember what the household decided about a proposed change in a turn.
    pub fn set_turn_outcome(&self, owner: &str, conversation: &str, turn: &str, outcome: &str) -> Result<bool> {
        if !matches!(outcome, "done" | "canceled") {
            bail!("the outcome must be done or canceled");
        }
        let conn = self.lock();
        let n = conn.execute(
            "UPDATE conversation_turns SET outcome = ?1
             WHERE id = ?2 AND conversation_id = ?3 AND conversation_id IN (SELECT id FROM conversations WHERE owner = ?4)",
            params![outcome, turn, conversation, owner],
        )?;
        Ok(n > 0)
    }

    pub fn rename_conversation(&self, owner: &str, id: &str, title: &str) -> Result<Option<Summary>> {
        let title = clean_title(title)?;
        let conn = self.lock();
        let tx = conn.unchecked_transaction()?;
        if tx.execute("UPDATE conversations SET title = ?3 WHERE id = ?1 AND owner = ?2", params![id, owner, title])? == 0 {
            return Ok(None);
        }
        refresh_search(&tx, id)?;
        let s = summary(&tx, owner, id)?;
        tx.commit()?;
        Ok(s)
    }

    pub fn delete_conversation(&self, owner: &str, id: &str) -> Result<bool> {
        let conn = self.lock();
        let tx = conn.unchecked_transaction()?;
        if summary(&tx, owner, id)?.is_none() {
            return Ok(false);
        }
        delete(&tx, id)?;
        tx.commit()?;
        Ok(true)
    }

    /// Send a copy of a conversation of `owner` to the device `to`, marked as
    /// coming from `from_name`. Only finished turns are copied (an answer
    /// still being written is saved in the original only). `None` when the
    /// conversation is not `owner`'s. The caller checks that `to` is a device
    /// of the household.
    pub fn send_conversation(&self, owner: &str, id: &str, to: &str, from_name: &str) -> Result<Option<Summary>> {
        let conn = self.lock();
        let tx = conn.unchecked_transaction()?;
        let Some(original) = summary(&tx, owner, id)? else {
            return Ok(None);
        };
        let copy = insert_conversation(&tx, to, &original.title, Some((owner, from_name)))?;
        tx.execute(
            "INSERT INTO conversation_turns
               (id, conversation_id, seq, question, answer, status, error, answer_id, sources, details, outcome, created_at, answered_at)
             SELECT lower(hex(randomblob(16))), ?2, seq, question, answer, status, error, NULL, sources, details, outcome, created_at, answered_at
             FROM conversation_turns WHERE conversation_id = ?1 AND status <> 'pending'",
            params![id, copy],
        )?;
        refresh_search(&tx, &copy)?;
        let s = summary(&tx, to, &copy)?;
        tx.commit()?;
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Device;

    fn done(text: &str) -> Finished {
        Finished { answer: text.into(), sources: serde_json::json!([{ "n": 1, "title": "Voda" }]), details: serde_json::json!({ "grounded": true }), ..Default::default() }
    }

    #[test]
    fn titles_come_from_the_first_question() {
        assert_eq!(title_from("  Kako da prečistim   vodu? "), "Kako da prečistim vodu?");
        let long = title_from("How do I purify water without a filter when the power is out for days?");
        assert!(long.ends_with('…') && long.chars().count() <= AUTO_TITLE + 1, "{long}");
        assert_eq!(long, "How do I purify water without a filter when the…");
        assert_eq!(title_from(&"x".repeat(100)).chars().count(), AUTO_TITLE + 1);
        assert!(clean_title("   ").is_err());
        assert_eq!(clean_title(" Voda \n za piće ").unwrap(), "Voda za piće");
        assert_eq!(clean_title(&"y".repeat(200)).unwrap().chars().count(), MAX_TITLE);
    }

    #[test]
    fn a_question_is_saved_and_its_answer_completes_it() {
        let db = Db::open_in_memory().unwrap();
        let (conv, turn) = db.add_turn(LAPTOP, None, "Kako da prečistim vodu?", "a1").unwrap();
        assert_eq!(conv.title, "Kako da prečistim vodu?");
        assert_eq!(conv.turns, 1);
        assert_eq!(turn.status, "pending");
        let (again, _) = db.add_turn(LAPTOP, Some(&conv.id), "A bez lonca?", "a2").unwrap();
        assert_eq!(again.id, conv.id);
        assert!(db.finish_turn("a1", &done("Prokuvaj je [1].")).unwrap());
        assert!(!db.finish_turn("a1", &done("again")).unwrap(), "a finished turn stays as it is");
        db.finish_turn("a2", &Finished { failed: true, error: Some("the AI engine is not installed".into()), ..Default::default() }).unwrap();
        let c = db.get_conversation(LAPTOP, &conv.id).unwrap().unwrap();
        assert_eq!(c.turns.len(), 2);
        assert_eq!((c.turns[0].status.as_str(), c.turns[0].answer.as_str()), ("done", "Prokuvaj je [1]."));
        assert_eq!(c.turns[0].sources[0]["title"], "Voda");
        assert_eq!(c.turns[0].details["grounded"], true);
        assert!(c.turns[0].answered_at.is_some());
        assert_eq!(c.turns[1].status, "failed");
        assert_eq!(c.turns[1].error.as_deref(), Some("the AI engine is not installed"));
        assert!(db.set_turn_outcome(LAPTOP, &conv.id, &c.turns[0].id, "done").unwrap());
        assert!(db.set_turn_outcome(LAPTOP, &conv.id, &c.turns[0].id, "maybe").is_err());
        assert_eq!(db.get_conversation(LAPTOP, &conv.id).unwrap().unwrap().turns[0].outcome.as_deref(), Some("done"));
    }

    #[test]
    fn each_device_sees_only_its_own() {
        let db = Db::open_in_memory().unwrap();
        let (mine, turn) = db.add_turn(LAPTOP, None, "Laptop's question", "a1").unwrap();
        let (theirs, _) = db.add_turn("phone-1", None, "Phone's question", "a2").unwrap();
        assert_eq!(db.list_conversations(LAPTOP, None).unwrap().iter().map(|c| &c.id).collect::<Vec<_>>(), [&mine.id]);
        assert_eq!(db.list_conversations("phone-1", None).unwrap().iter().map(|c| &c.id).collect::<Vec<_>>(), [&theirs.id]);
        assert!(db.get_conversation("phone-1", &mine.id).unwrap().is_none());
        assert!(db.add_turn("phone-1", Some(&mine.id), "sneaky", "a3").is_err());
        assert!(db.rename_conversation("phone-1", &mine.id, "mine now").unwrap().is_none());
        assert!(!db.delete_conversation("phone-1", &mine.id).unwrap());
        assert!(!db.set_turn_outcome("phone-1", &mine.id, &turn.id, "done").unwrap());
        assert!(db.send_conversation("phone-1", &mine.id, "phone-1", "Phone").unwrap().is_none());
        let c = db.get_conversation(LAPTOP, &mine.id).unwrap().unwrap();
        assert_eq!((c.summary.title.as_str(), c.turns.len()), ("Laptop's question", 1), "nothing changed");
    }

    #[test]
    fn rename_delete_and_the_newest_first() {
        let db = Db::open_in_memory().unwrap();
        let (a, _) = db.add_turn(LAPTOP, None, "First", "a1").unwrap();
        let (b, _) = db.add_turn(LAPTOP, None, "Second", "a2").unwrap();
        let order = |db: &Db| db.list_conversations(LAPTOP, None).unwrap().into_iter().map(|c| c.title).collect::<Vec<_>>();
        assert_eq!(order(&db), ["Second", "First"]);
        db.add_turn(LAPTOP, Some(&a.id), "More", "a3").unwrap();
        assert_eq!(order(&db), ["First", "Second"], "asking again brings it to the top");
        assert_eq!(db.rename_conversation(LAPTOP, &a.id, "  Water  ").unwrap().unwrap().title, "Water");
        assert!(db.rename_conversation(LAPTOP, &a.id, " ").is_err());
        assert!(db.delete_conversation(LAPTOP, &b.id).unwrap());
        assert!(!db.delete_conversation(LAPTOP, &b.id).unwrap());
        assert_eq!(order(&db), ["Water"]);
        let turns: i64 = db.lock().query_row("SELECT COUNT(*) FROM conversation_turns", [], |r| r.get(0)).unwrap();
        assert_eq!(turns, 2, "the deleted one's turn is gone");
        // An empty one gets its title from the first question.
        let empty = db.create_conversation(LAPTOP, "").unwrap();
        assert_eq!(empty.title, "");
        assert_eq!(db.add_turn(LAPTOP, Some(&empty.id), "Kiša i struja", "a4").unwrap().0.title, "Kiša i struja");
    }

    #[test]
    fn search_finds_titles_and_questions_with_or_without_diacritics() {
        let db = Db::open_in_memory().unwrap();
        let (tea, _) = db.add_turn(LAPTOP, None, "Koji čaj za stomak?", "a1").unwrap();
        db.add_turn(LAPTOP, Some(&tea.id), "A za grlo?", "a2").unwrap();
        let (water, _) = db.add_turn(LAPTOP, None, "Boiling water", "a3").unwrap();
        db.add_turn("phone-1", None, "Čaj od nane", "a4").unwrap();
        let found = |q: &str| db.list_conversations(LAPTOP, Some(q)).unwrap().into_iter().map(|c| c.id).collect::<Vec<_>>();
        assert_eq!(found("caj"), [tea.id.as_str()]);
        assert_eq!(found("ČAJ"), [tea.id.as_str()]);
        assert_eq!(found("чај"), [tea.id.as_str()]);
        assert_eq!(found("grlo caj"), [tea.id.as_str()], "every word, anywhere");
        assert_eq!(found("water"), [water.id.as_str()]);
        assert!(found("nane").is_empty(), "not another device's");
        assert_eq!(found("  ").len(), 2);
        db.rename_conversation(LAPTOP, &water.id, "Voda").unwrap();
        assert_eq!(found("voda"), [water.id]);
    }

    #[test]
    fn limits_hold() {
        let db = Db::open_in_memory().unwrap();
        let (conv, _) = db.add_turn(LAPTOP, None, "q", "a0").unwrap();
        for i in 1..MAX_TURNS {
            db.add_turn(LAPTOP, Some(&conv.id), "q", &format!("a{i}")).unwrap();
        }
        let err = db.add_turn(LAPTOP, Some(&conv.id), "q", "over").unwrap_err().to_string();
        assert!(err.contains("conversation is too long"), "{err}");
        assert!(db.add_turn(LAPTOP, None, &"x".repeat(MAX_QUESTION + 1), "a").is_err());
        assert!(db.add_turn(LAPTOP, None, "  ", "a").is_err());
        // A long answer is cut, oversized sources are dropped.
        db.add_turn(LAPTOP, None, "long", "long").unwrap();
        let huge = Finished { answer: "é".repeat(MAX_ANSWER + 10), sources: serde_json::json!(["s".repeat(MAX_SOURCES)]), ..Default::default() };
        db.finish_turn("long", &huge).unwrap();
        let c = &db.list_conversations(LAPTOP, Some("long")).unwrap()[0];
        let turn = &db.get_conversation(LAPTOP, &c.id).unwrap().unwrap().turns[0];
        assert_eq!(turn.answer.chars().count(), MAX_ANSWER);
        assert_eq!(turn.sources, serde_json::json!([]));
    }

    #[test]
    fn the_oldest_conversations_make_room() {
        let db = Db::open_in_memory().unwrap();
        let (first, _) = db.add_turn(LAPTOP, None, "first", "f").unwrap();
        for i in 1..MAX_CONVERSATIONS {
            db.create_conversation(LAPTOP, &format!("c{i}")).unwrap();
        }
        db.add_turn(LAPTOP, Some(&first.id), "used again", "g").unwrap();
        db.create_conversation("phone-1", "not counted with the laptop's").unwrap();
        let newest = db.create_conversation(LAPTOP, "one more").unwrap();
        let list = db.list_conversations(LAPTOP, None).unwrap();
        assert_eq!(list.len(), MAX_CONVERSATIONS);
        assert_eq!(list[0].id, newest.id);
        assert!(list.iter().any(|c| c.id == first.id), "used lately, so kept");
        assert!(!list.iter().any(|c| c.title == "c1"), "the one used longest ago made room");
        assert_eq!(db.list_conversations("phone-1", None).unwrap().len(), 1);
    }

    #[test]
    fn a_copy_goes_to_another_device_with_the_senders_name() {
        let db = Db::open_in_memory().unwrap();
        let (conv, _) = db.add_turn(LAPTOP, None, "Kako da prečistim vodu?", "a1").unwrap();
        db.finish_turn("a1", &done("Prokuvaj je.")).unwrap();
        db.add_turn(LAPTOP, Some(&conv.id), "Still being written", "a2").unwrap();
        let copy = db.send_conversation(LAPTOP, &conv.id, "phone-1", LAPTOP).unwrap().unwrap();
        assert_ne!(copy.id, conv.id);
        assert_eq!((copy.from_owner.as_deref(), copy.from_name.as_deref()), (Some(LAPTOP), Some(LAPTOP)));
        let c = db.get_conversation("phone-1", &copy.id).unwrap().unwrap();
        assert_eq!(c.summary.title, "Kako da prečistim vodu?");
        assert_eq!(c.turns.len(), 1, "only finished answers are copied");
        assert_eq!(c.turns[0].answer, "Prokuvaj je.");
        assert_eq!(c.turns[0].answer_id, None);
        // The original's pending answer finishes only the original.
        db.finish_turn("a2", &done("Done now.")).unwrap();
        assert_eq!(db.get_conversation("phone-1", &copy.id).unwrap().unwrap().turns.len(), 1);
        assert_eq!(db.get_conversation(LAPTOP, &conv.id).unwrap().unwrap().turns[1].answer, "Done now.");
        // The copy is the phone's own: renaming or deleting it leaves the original alone.
        db.rename_conversation("phone-1", &copy.id, "Voda").unwrap();
        assert!(db.delete_conversation("phone-1", &copy.id).unwrap());
        assert_eq!(db.get_conversation(LAPTOP, &conv.id).unwrap().unwrap().summary.title, "Kako da prečistim vodu?");
        assert!(db.list_conversations("phone-1", Some("vodu")).unwrap().is_empty());
    }

    #[test]
    fn a_removed_phone_takes_its_conversations_along() {
        let db = Db::open_in_memory().unwrap();
        let phone = Device { id: "phone-1".into(), name: "Ana".into(), platform: "android".into(), created_at: now_rfc3339(), last_seen: None };
        db.insert_device(&phone, "hash").unwrap();
        db.add_turn("phone-1", None, "Phone's", "a1").unwrap();
        let (sent, _) = db.add_turn("phone-1", None, "Sent to the laptop", "a2").unwrap();
        db.finish_turn("a2", &done("ok")).unwrap();
        db.send_conversation("phone-1", &sent.id, LAPTOP, "Ana").unwrap().unwrap();
        db.add_turn(LAPTOP, None, "Laptop's", "a3").unwrap();
        assert!(db.delete_device("phone-1").unwrap());
        assert!(db.list_conversations("phone-1", None).unwrap().is_empty());
        let turns: i64 = db.lock().query_row("SELECT COUNT(*) FROM conversation_turns WHERE question = 'Phone''s'", [], |r| r.get(0)).unwrap();
        assert_eq!(turns, 0);
        let laptop = db.list_conversations(LAPTOP, None).unwrap();
        assert_eq!(laptop.len(), 2, "what the laptop has, including the copy it got, stays");
        assert!(laptop.iter().any(|c| c.from_name.as_deref() == Some("Ana")));
    }
}
