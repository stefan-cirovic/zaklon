//! Household supplies: items with batches (each batch has its own quantity
//! and expiry date), places, remembered barcodes, the shopping list with
//! "bought" items waiting to be put away, and the change history. Everything
//! here is shared by the whole household.
//!
//! An item's `quantity` and `expiry` are kept up to date from its batches:
//! the total and the earliest expiry of what is still there. Using an item
//! takes from the batch that expires first.

use anyhow::{bail, Result};
use rusqlite::{params, Connection, OptionalExtension, Row, Transaction};
use serde::{Deserialize, Serialize};

use crate::dates::{is_date, now_rfc3339, plus_days};
use crate::db::Db;

/// Kept here too for callers that know it from before [`crate::dates`].
pub use crate::dates::today;

pub const CATEGORIES: &[&str] = &["food", "drink", "medicine", "hygiene", "equipment", "fuel", "other"];
pub const UNITS: &[&str] = &["pcs", "kg", "g", "l", "ml", "pack"];
/// Built-in places: the id stored in an item's `place`, then the name in
/// English and in Serbian (the same names the interface shows).
pub const PRESET_PLACES: &[(&str, &str, &str)] = &[
    ("pantry", "Pantry", "Ostava"),
    ("fridge", "Fridge", "Frižider"),
    ("freezer", "Freezer", "Zamrzivač"),
    ("medicine_cabinet", "Medicine cabinet", "Kućna apoteka"),
    ("garage", "Garage", "Garaža"),
    ("basement", "Basement", "Podrum"),
];

/// Items expiring within this many days show up as "expiring soon".
pub const EXPIRING_DAYS: i64 = 30;
/// Largest quantity we accept; anything bigger is a typo or an attack.
pub const MAX_QUANTITY: f64 = 1_000_000_000.0;

pub(crate) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS places (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS barcodes (
    barcode TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    unit TEXT,
    category TEXT,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS shopping (
    id TEXT PRIMARY KEY,
    item_id TEXT,
    text TEXT NOT NULL,
    quantity REAL,
    unit TEXT,
    done INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    updated_by TEXT
);
CREATE TABLE IF NOT EXISTS batches (
    id TEXT PRIMARY KEY,
    item_id TEXT NOT NULL,
    quantity REAL NOT NULL,
    expiry TEXT,
    added_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS batches_item ON batches(item_id);
CREATE INDEX IF NOT EXISTS items_expiry ON items(expiry);
"#;

/// Bring an existing database up to date. Safe to run on every start.
pub(crate) fn migrate(conn: &Connection) -> Result<()> {
    // Shopping list: "status" replaced the old done flag.
    let has_status: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('shopping') WHERE name = 'status'")?
        .exists([])?;
    if !has_status {
        conn.execute_batch(
            "ALTER TABLE shopping ADD COLUMN status TEXT NOT NULL DEFAULT 'open';
             UPDATE shopping SET status = 'bought' WHERE done = 1;",
        )?;
    }
    // Older versions marked an entry "putting" while putting it away, and a
    // crash halfway hid it for good. Show it again with the things to put
    // away, where the household can put it away or remove it.
    conn.execute("UPDATE shopping SET status = 'bought' WHERE status = 'putting'", [])?;
    // Batches: items from before batches existed get one batch each.
    let done: bool = conn
        .prepare("SELECT 1 FROM settings WHERE key = 'migration_batches_v1'")?
        .exists([])?;
    if !done {
        conn.execute_batch(
            "INSERT INTO batches (id, item_id, quantity, expiry, added_at)
               SELECT lower(hex(randomblob(16))), id, quantity, expiry, updated_at
               FROM items
               WHERE deleted = 0 AND quantity > 0 AND id NOT IN (SELECT item_id FROM batches);
             INSERT OR REPLACE INTO settings (key, value) VALUES ('migration_batches_v1', '1');",
        )?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Batch {
    pub id: String,
    pub quantity: f64,
    /// ISO date "YYYY-MM-DD".
    pub expiry: Option<String>,
    pub added_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Item {
    pub id: String,
    pub name: String,
    /// Total of all batches.
    pub quantity: f64,
    pub unit: String,
    pub category: String,
    pub place: Option<String>,
    /// Earliest expiry among batches still in stock.
    pub expiry: Option<String>,
    pub barcode: Option<String>,
    pub min_quantity: Option<f64>,
    pub notes: Option<String>,
    pub updated_at: String,
    pub updated_by: Option<String>,
    /// Batches, the one that expires first first.
    #[serde(default)]
    pub batches: Vec<Batch>,
}

/// Fields a caller may set when creating or editing an item. For edits,
/// `None` leaves a field unchanged; `Some(None)`/empty string clears it.
/// On edits, `quantity` sets the new total (the difference is added as a
/// batch without expiry, or taken from the batches that expire first), and
/// `expiry` sets the date of the only batch (items with several batches are
/// edited batch by batch).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ItemInput {
    pub name: Option<String>,
    pub quantity: Option<f64>,
    pub unit: Option<String>,
    pub category: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub place: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub expiry: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub barcode: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub min_quantity: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub notes: Option<Option<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BatchInput {
    pub quantity: Option<f64>,
    #[serde(default, deserialize_with = "double_option")]
    pub expiry: Option<Option<String>>,
}

/// Putting a bought thing away: add it to an existing item, or make a new one.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PutAwayInput {
    pub quantity: f64,
    #[serde(default)]
    pub expiry: Option<String>,
    #[serde(default)]
    pub place: Option<String>,
    /// Existing item to add to; when absent a new item is created.
    #[serde(default)]
    pub item_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub barcode: Option<String>,
}

fn double_option<'de, T, D>(d: D) -> std::result::Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

#[derive(Debug, Clone, Serialize)]
pub struct Place {
    pub id: String,
    pub name: String,
    /// True for the built-in places (their names are translated by the interface).
    pub preset: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownBarcode {
    pub barcode: String,
    pub name: String,
    pub unit: Option<String>,
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShoppingEntry {
    pub id: String,
    pub item_id: Option<String>,
    pub text: String,
    pub quantity: Option<f64>,
    pub unit: Option<String>,
    /// "open" (to buy) or "bought" (waiting to be put away).
    pub status: String,
    /// "manual", or "running_low" for entries computed from items below their minimum.
    pub source: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct HistoryEntry {
    pub seq: i64,
    pub at: String,
    pub actor: Option<String>,
    pub entity: String,
    pub entity_id: String,
    pub action: String,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub total_items: i64,
    pub expired: Vec<Item>,
    pub expiring_soon: Vec<Item>,
    pub running_low: Vec<Item>,
    /// Bought things not yet put away.
    pub to_put_away: i64,
}

fn clean(s: Option<String>) -> Option<String> {
    s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// Trimmed, non-empty, at most `max` characters.
fn clean_max(s: Option<String>, max: usize) -> Option<String> {
    clean(s).map(|v| v.chars().take(max).collect())
}

fn clamp_qty(q: f64) -> f64 {
    if !q.is_finite() {
        return 0.0;
    }
    (q.clamp(0.0, MAX_QUANTITY) * 1000.0).round() / 1000.0
}

fn checked_date(d: Option<String>) -> Result<Option<String>> {
    let d = clean(d);
    if let Some(v) = &d {
        if !is_date(v) {
            bail!("expiry must be a date like 2027-03-31");
        }
    }
    Ok(d)
}

fn row_item(r: &Row) -> rusqlite::Result<Item> {
    Ok(Item {
        id: r.get(0)?,
        name: r.get(1)?,
        quantity: r.get(2)?,
        unit: r.get(3)?,
        category: r.get(4)?,
        place: r.get(5)?,
        expiry: r.get(6)?,
        barcode: r.get(7)?,
        min_quantity: r.get(8)?,
        notes: r.get(9)?,
        updated_at: r.get(10)?,
        updated_by: r.get(11)?,
        batches: Vec::new(),
    })
}

const ITEM_COLS: &str =
    "id, name, quantity, unit, category, place, expiry, barcode, min_quantity, notes, updated_at, updated_by";

/// Batches of an item, the one that expires first first (no date last).
const BATCH_ORDER: &str = "ORDER BY (expiry IS NULL), expiry, added_at";

// ---- helpers that run inside one transaction ---------------------------------

fn load_batches(conn: &Connection, item_id: &str) -> Result<Vec<Batch>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT id, quantity, expiry, added_at FROM batches WHERE item_id = ?1 {BATCH_ORDER}"
    ))?;
    let rows = stmt.query_map(params![item_id], |r| {
        Ok(Batch { id: r.get(0)?, quantity: r.get(1)?, expiry: r.get(2)?, added_at: r.get(3)? })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

fn load_item(conn: &Connection, id: &str) -> Result<Option<Item>> {
    let item = conn
        .query_row(&format!("SELECT {ITEM_COLS} FROM items WHERE id = ?1 AND deleted = 0"), params![id], row_item)
        .optional()?;
    match item {
        Some(mut i) => {
            i.batches = load_batches(conn, id)?;
            Ok(Some(i))
        }
        None => Ok(None),
    }
}

/// Recompute an item's total and earliest expiry from its batches.
fn resync(tx: &Transaction, item_id: &str, actor: &str) -> Result<()> {
    tx.execute("DELETE FROM batches WHERE item_id = ?1 AND quantity <= 0", params![item_id])?;
    let (total, earliest): (f64, Option<String>) = tx.query_row(
        "SELECT COALESCE(SUM(quantity), 0), MIN(expiry) FROM batches WHERE item_id = ?1",
        params![item_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    tx.execute(
        "UPDATE items SET quantity = ?2, expiry = ?3, updated_at = ?4, updated_by = ?5 WHERE id = ?1",
        params![item_id, clamp_qty(total), earliest, now_rfc3339(), actor],
    )?;
    Ok(())
}

fn add_batch_tx(tx: &Transaction, item_id: &str, quantity: f64, expiry: Option<String>) -> Result<()> {
    let q = clamp_qty(quantity);
    if q <= 0.0 {
        return Ok(());
    }
    // Same date (or both without one): top up the existing batch.
    let existing: Option<String> = tx
        .query_row(
            "SELECT id FROM batches WHERE item_id = ?1 AND expiry IS ?2 LIMIT 1",
            params![item_id, expiry],
            |r| r.get(0),
        )
        .optional()?;
    match existing {
        Some(bid) => {
            tx.execute("UPDATE batches SET quantity = MIN(quantity + ?2, ?3) WHERE id = ?1", params![bid, q, MAX_QUANTITY])?;
        }
        None => {
            tx.execute(
                "INSERT INTO batches (id, item_id, quantity, expiry, added_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![uuid::Uuid::new_v4().to_string(), item_id, q, expiry, now_rfc3339()],
            )?;
        }
    }
    Ok(())
}

/// Take `amount` from the batches that expire first.
fn consume_tx(tx: &Transaction, item_id: &str, amount: f64) -> Result<()> {
    let mut left = amount;
    for b in load_batches(tx, item_id)? {
        if left <= 0.0 {
            break;
        }
        let take = b.quantity.min(left);
        tx.execute("UPDATE batches SET quantity = ?2 WHERE id = ?1", params![b.id, clamp_qty(b.quantity - take)])?;
        left -= take;
    }
    Ok(())
}

fn history_tx<T: Serialize>(
    tx: &Transaction,
    entity: &str,
    entity_id: &str,
    action: &str,
    actor: &str,
    before: Option<&T>,
    after: Option<&T>,
) -> Result<()> {
    tx.execute(
        "INSERT INTO history (at, actor, entity, entity_id, action, before_json, after_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            now_rfc3339(),
            actor,
            entity,
            entity_id,
            action,
            before.map(serde_json::to_string).transpose()?,
            after.map(serde_json::to_string).transpose()?
        ],
    )?;
    Ok(())
}

fn remember_barcode_tx(conn: &Connection, barcode: &str, name: &str, unit: Option<&str>, category: Option<&str>) -> Result<()> {
    conn.execute(
        "INSERT INTO barcodes (barcode, name, unit, category, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(barcode) DO UPDATE SET name=excluded.name, unit=excluded.unit,
           category=excluded.category, updated_at=excluded.updated_at",
        params![barcode.trim(), name, unit, category, now_rfc3339()],
    )?;
    Ok(())
}

fn create_item_tx(tx: &Transaction, input: ItemInput, actor: &str) -> Result<Item> {
    let name = clean(input.name).ok_or_else(|| anyhow::anyhow!("name is required"))?;
    let unit = clean(input.unit).unwrap_or_else(|| "pcs".into());
    let category = clean(input.category).unwrap_or_else(|| "other".into());
    if !CATEGORIES.contains(&category.as_str()) {
        bail!("unknown category");
    }
    let expiry = checked_date(input.expiry.flatten())?;
    let quantity = clamp_qty(input.quantity.unwrap_or(1.0));
    let id = uuid::Uuid::new_v4().to_string();
    let name: String = name.chars().take(120).collect();
    let unit: String = unit.chars().take(20).collect();
    let place = clean_max(input.place.flatten(), 60);
    let barcode = clean_max(input.barcode.flatten(), 64);
    let min_quantity = input.min_quantity.flatten().filter(|m| m.is_finite() && *m >= 0.0).map(clamp_qty);
    let notes = clean_max(input.notes.flatten(), 500);

    tx.execute(
        &format!("INSERT INTO items ({ITEM_COLS}, deleted) VALUES (?1, ?2, 0, ?3, ?4, ?5, NULL, ?6, ?7, ?8, ?9, ?10, 0)"),
        params![id, name, unit, category, place, barcode, min_quantity, notes, now_rfc3339(), actor],
    )?;
    add_batch_tx(tx, &id, quantity, expiry)?;
    resync(tx, &id, actor)?;
    if let Some(code) = &barcode {
        remember_barcode_tx(tx, code, &name, Some(&unit), Some(&category))?;
    }
    let item = load_item(tx, &id)?.ok_or_else(|| anyhow::anyhow!("item vanished"))?;
    history_tx(tx, "item", &id, "create", actor, None, Some(&item))?;
    Ok(item)
}

fn put_away_tx(
    tx: &Transaction,
    input: PutAwayInput,
    actor: &str,
    entry_item: Option<String>,
    text: String,
    entry_unit: Option<String>,
) -> Result<Item> {
    let quantity = clamp_qty(input.quantity);
    if quantity <= 0.0 {
        bail!("quantity must be more than zero");
    }
    let expiry = checked_date(input.expiry)?;
    let existing = match input.item_id.or(entry_item) {
        Some(id) => load_item(tx, &id)?,
        None => None,
    };
    let Some(before) = existing else {
        return create_item_tx(
            tx,
            ItemInput {
                name: Some(input.name.unwrap_or(text)),
                quantity: Some(quantity),
                unit: input.unit.or(entry_unit),
                category: input.category.or(Some("food".into())),
                place: Some(input.place),
                expiry: Some(expiry),
                barcode: Some(input.barcode),
                ..Default::default()
            },
            actor,
        );
    };
    add_batch_tx(tx, &before.id, quantity, expiry)?;
    let place = clean_max(input.place, 60);
    if place.is_some() && place != before.place {
        tx.execute("UPDATE items SET place = ?2 WHERE id = ?1", params![before.id, place])?;
    }
    resync(tx, &before.id, actor)?;
    let after = load_item(tx, &before.id)?.ok_or_else(|| anyhow::anyhow!("item vanished"))?;
    history_tx(tx, "item", &before.id, "add", actor, Some(&before), Some(&after))?;
    Ok(after)
}

/// The name people read for a place stored on an item: a built-in place in
/// `language` ("sr" or "en"), a household place by its name. Anything else
/// (a name written by hand) is shown as it is.
pub fn place_name(places: &[Place], stored: &str, language: &str) -> String {
    if let Some((_, en, sr)) = PRESET_PLACES.iter().find(|(id, _, _)| *id == stored) {
        return if language == "sr" { sr } else { en }.to_string();
    }
    places.iter().find(|p| p.id == stored).map_or_else(|| stored.to_string(), |p| p.name.clone())
}

impl Db {
    // ---- items ----------------------------------------------------------

    pub fn list_items(&self) -> Result<Vec<Item>> {
        let conn = self.lock();
        let mut items: Vec<Item> = {
            let mut stmt = conn.prepare(&format!(
                "SELECT {ITEM_COLS} FROM items WHERE deleted = 0 ORDER BY name COLLATE NOCASE"
            ))?;
            let rows = stmt.query_map([], row_item)?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        let mut stmt = conn.prepare(&format!(
            "SELECT item_id, id, quantity, expiry, added_at FROM batches {BATCH_ORDER}"
        ))?;
        let mut by_item: std::collections::HashMap<String, Vec<Batch>> = std::collections::HashMap::new();
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, Batch { id: r.get(1)?, quantity: r.get(2)?, expiry: r.get(3)?, added_at: r.get(4)? }))
        })?;
        for row in rows {
            let (item_id, b) = row?;
            by_item.entry(item_id).or_default().push(b);
        }
        for i in &mut items {
            i.batches = by_item.remove(&i.id).unwrap_or_default();
        }
        Ok(items)
    }

    /// The items with each place given as the name people read, in
    /// `language`, rather than the id the app stores. For the assistant and
    /// anything else that shows items outside the supplies screen.
    pub fn list_items_for_reading(&self, language: &str) -> Result<Vec<Item>> {
        let places = self.list_places()?;
        let mut items = self.list_items()?;
        for i in &mut items {
            i.place = i.place.take().map(|p| place_name(&places, &p, language));
        }
        Ok(items)
    }

    pub fn get_item(&self, id: &str) -> Result<Option<Item>> {
        let conn = self.lock();
        load_item(&conn, id)
    }

    pub fn find_item_by_barcode(&self, barcode: &str) -> Result<Option<Item>> {
        let conn = self.lock();
        let id: Option<String> = conn
            .query_row(
                "SELECT id FROM items WHERE barcode = ?1 AND deleted = 0 ORDER BY updated_at DESC LIMIT 1",
                params![barcode],
                |r| r.get(0),
            )
            .optional()?;
        match id {
            Some(id) => load_item(&conn, &id),
            None => Ok(None),
        }
    }

    pub fn create_item(&self, input: ItemInput, actor: &str) -> Result<Item> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let item = create_item_tx(&tx, input, actor)?;
        tx.commit()?;
        Ok(item)
    }

    pub fn update_item(&self, id: &str, input: ItemInput, actor: &str) -> Result<Option<Item>> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let Some(before) = load_item(&tx, id)? else { return Ok(None) };
        let mut after = before.clone();
        if let Some(n) = clean(input.name) {
            after.name = n.chars().take(120).collect();
        }
        if let Some(u) = clean(input.unit) {
            after.unit = u.chars().take(20).collect();
        }
        if let Some(c) = clean(input.category) {
            if !CATEGORIES.contains(&c.as_str()) {
                bail!("unknown category");
            }
            after.category = c;
        }
        if let Some(p) = input.place {
            after.place = clean_max(p, 60);
        }
        if let Some(b) = input.barcode {
            after.barcode = clean_max(b, 64);
        }
        if let Some(m) = input.min_quantity {
            after.min_quantity = m.filter(|v| v.is_finite() && *v >= 0.0).map(clamp_qty);
        }
        if let Some(n) = input.notes {
            after.notes = clean_max(n, 500);
        }
        tx.execute(
            "UPDATE items SET name=?2, unit=?3, category=?4, place=?5, barcode=?6, min_quantity=?7, notes=?8 WHERE id=?1",
            params![id, after.name, after.unit, after.category, after.place, after.barcode, after.min_quantity, after.notes],
        )?;
        // Quantity and expiry act on batches.
        let new_expiry = match input.expiry {
            Some(e) => Some(checked_date(e)?),
            None => None,
        };
        if let Some(e) = new_expiry.clone() {
            match before.batches.len() {
                0 => {}
                1 => {
                    tx.execute("UPDATE batches SET expiry = ?2 WHERE id = ?1", params![before.batches[0].id, e])?;
                }
                _ => {
                    if e != before.expiry {
                        bail!("this item has several batches; change the date of a batch instead");
                    }
                }
            }
        }
        if let Some(q) = input.quantity {
            let target = clamp_qty(q);
            let diff = target - before.quantity;
            if diff > 0.0 {
                // A date given in the same save belongs to the added amount too
                // (restocking an item that had run out, or one with a single batch).
                let dated = if before.batches.len() <= 1 { new_expiry.clone().flatten() } else { None };
                add_batch_tx(&tx, id, diff, dated)?;
            } else if diff < 0.0 {
                consume_tx(&tx, id, -diff)?;
            }
        }
        resync(&tx, id, actor)?;
        let after = load_item(&tx, id)?.ok_or_else(|| anyhow::anyhow!("item vanished"))?;
        if let Some(code) = &after.barcode {
            remember_barcode_tx(&tx, code, &after.name, Some(&after.unit), Some(&after.category))?;
        }
        let same = after.name == before.name
            && after.quantity == before.quantity
            && after.unit == before.unit
            && after.category == before.category
            && after.place == before.place
            && after.expiry == before.expiry
            && after.barcode == before.barcode
            && after.min_quantity == before.min_quantity
            && after.notes == before.notes
            && after.batches == before.batches;
        if !same {
            history_tx(&tx, "item", id, "update", actor, Some(&before), Some(&after))?;
        }
        tx.commit()?;
        Ok(Some(after))
    }

    /// Add to or take from the quantity; never below zero. Adding goes to the
    /// batch without a date; using takes from the batch that expires first.
    /// Everything happens in one transaction, so simultaneous taps from two
    /// phones are both counted.
    pub fn adjust_item(&self, id: &str, delta: f64, actor: &str) -> Result<Option<Item>> {
        if !delta.is_finite() {
            bail!("delta must be a number");
        }
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let Some(before) = load_item(&tx, id)? else { return Ok(None) };
        if delta > 0.0 {
            add_batch_tx(&tx, id, delta, None)?;
        } else if delta < 0.0 {
            consume_tx(&tx, id, -delta)?;
        }
        resync(&tx, id, actor)?;
        let after = load_item(&tx, id)?.ok_or_else(|| anyhow::anyhow!("item vanished"))?;
        let action = if delta < 0.0 { "consume" } else { "add" };
        history_tx(&tx, "item", id, action, actor, Some(&before), Some(&after))?;
        tx.commit()?;
        Ok(Some(after))
    }

    /// Soft delete: the row stays for history and sync, hidden from lists.
    pub fn delete_item(&self, id: &str, actor: &str) -> Result<bool> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let Some(before) = load_item(&tx, id)? else { return Ok(false) };
        tx.execute("UPDATE items SET deleted=1, updated_at=?2, updated_by=?3 WHERE id=?1", params![id, now_rfc3339(), actor])?;
        tx.execute("DELETE FROM batches WHERE item_id = ?1", params![id])?;
        tx.execute("DELETE FROM shopping WHERE item_id = ?1", params![id])?;
        history_tx::<Item>(&tx, "item", id, "delete", actor, Some(&before), None)?;
        tx.commit()?;
        Ok(true)
    }

    // ---- batches --------------------------------------------------------

    pub fn add_batch(&self, item_id: &str, input: BatchInput, actor: &str) -> Result<Option<Item>> {
        let quantity = clamp_qty(input.quantity.unwrap_or(0.0));
        if quantity <= 0.0 {
            bail!("quantity must be more than zero");
        }
        let expiry = checked_date(input.expiry.flatten())?;
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let Some(before) = load_item(&tx, item_id)? else { return Ok(None) };
        add_batch_tx(&tx, item_id, quantity, expiry)?;
        resync(&tx, item_id, actor)?;
        let after = load_item(&tx, item_id)?.ok_or_else(|| anyhow::anyhow!("item vanished"))?;
        history_tx(&tx, "item", item_id, "add", actor, Some(&before), Some(&after))?;
        tx.commit()?;
        Ok(Some(after))
    }

    /// Change a batch's quantity or date (quantity 0 removes it).
    pub fn update_batch(&self, batch_id: &str, input: BatchInput, actor: &str) -> Result<Option<Item>> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let item_id: Option<String> =
            tx.query_row("SELECT item_id FROM batches WHERE id = ?1", params![batch_id], |r| r.get(0)).optional()?;
        let Some(item_id) = item_id else { return Ok(None) };
        let Some(before) = load_item(&tx, &item_id)? else { return Ok(None) };
        if let Some(q) = input.quantity {
            tx.execute("UPDATE batches SET quantity = ?2 WHERE id = ?1", params![batch_id, clamp_qty(q)])?;
        }
        if let Some(e) = input.expiry {
            let e = checked_date(e)?;
            tx.execute("UPDATE batches SET expiry = ?2 WHERE id = ?1", params![batch_id, e])?;
        }
        resync(&tx, &item_id, actor)?;
        let after = load_item(&tx, &item_id)?.ok_or_else(|| anyhow::anyhow!("item vanished"))?;
        history_tx(&tx, "item", &item_id, "update", actor, Some(&before), Some(&after))?;
        tx.commit()?;
        Ok(Some(after))
    }

    pub fn delete_batch(&self, batch_id: &str, actor: &str) -> Result<Option<Item>> {
        self.update_batch(batch_id, BatchInput { quantity: Some(0.0), expiry: None }, actor)
    }

    // ---- summary --------------------------------------------------------

    pub fn supplies_summary(&self) -> Result<Summary> {
        let items = self.list_items()?;
        let today = today();
        let soon = plus_days(&today, EXPIRING_DAYS);
        let mut expired: Vec<Item> = Vec::new();
        let mut expiring: Vec<Item> = Vec::new();
        for i in &items {
            if let Some(e) = &i.expiry {
                if i.quantity > 0.0 {
                    if e.as_str() < today.as_str() {
                        expired.push(i.clone());
                    } else if e.as_str() <= soon.as_str() {
                        expiring.push(i.clone());
                    }
                }
            }
        }
        expired.sort_by(|a, b| a.expiry.cmp(&b.expiry));
        expiring.sort_by(|a, b| a.expiry.cmp(&b.expiry));
        let running_low: Vec<Item> =
            items.iter().filter(|i| i.min_quantity.is_some_and(|m| i.quantity < m)).cloned().collect();
        let to_put_away: i64 = {
            let conn = self.lock();
            conn.query_row("SELECT COUNT(*) FROM shopping WHERE status = 'bought'", [], |r| r.get(0))?
        };
        Ok(Summary { total_items: items.len() as i64, expired, expiring_soon: expiring, running_low, to_put_away })
    }

    // ---- places ---------------------------------------------------------

    pub fn list_places(&self) -> Result<Vec<Place>> {
        let mut out: Vec<Place> =
            PRESET_PLACES.iter().map(|(id, _, _)| Place { id: id.to_string(), name: id.to_string(), preset: true }).collect();
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT id, name FROM places ORDER BY name COLLATE NOCASE")?;
        let rows = stmt.query_map([], |r| Ok(Place { id: r.get(0)?, name: r.get(1)?, preset: false }))?;
        for p in rows {
            out.push(p?);
        }
        Ok(out)
    }

    pub fn add_place(&self, name: &str) -> Result<Place> {
        let name: String = name.trim().chars().take(40).collect();
        if name.is_empty() {
            bail!("name is required");
        }
        let conn = self.lock();
        // Adding a name that already exists (also at the same moment from
        // another connection) returns the existing place instead of failing.
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO places (id, name, created_at) VALUES (?1, ?2, ?3) ON CONFLICT DO NOTHING",
            params![id, name, now_rfc3339()],
        )?;
        Ok(conn.query_row("SELECT id, name FROM places WHERE name = ?1 COLLATE NOCASE", params![name], |r| {
            Ok(Place { id: r.get(0)?, name: r.get(1)?, preset: false })
        })?)
    }

    /// Remove a household place. Items kept there no longer say where they
    /// are, rather than pointing at a place that does not exist.
    pub fn delete_place(&self, id: &str, actor: &str) -> Result<bool> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        if tx.execute("DELETE FROM places WHERE id = ?1", params![id])? == 0 {
            return Ok(false);
        }
        let ids: Vec<String> = tx
            .prepare("SELECT id FROM items WHERE place = ?1 AND deleted = 0")?
            .query_map(params![id], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        for item_id in ids {
            let before = load_item(&tx, &item_id)?;
            tx.execute(
                "UPDATE items SET place = NULL, updated_at = ?2, updated_by = ?3 WHERE id = ?1",
                params![item_id, now_rfc3339(), actor],
            )?;
            let after = load_item(&tx, &item_id)?;
            history_tx(&tx, "item", &item_id, "update", actor, before.as_ref(), after.as_ref())?;
        }
        tx.commit()?;
        Ok(true)
    }

    // ---- barcodes -------------------------------------------------------

    pub fn lookup_barcode(&self, barcode: &str) -> Result<Option<KnownBarcode>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                "SELECT barcode, name, unit, category FROM barcodes WHERE barcode = ?1",
                params![barcode.trim()],
                |r| Ok(KnownBarcode { barcode: r.get(0)?, name: r.get(1)?, unit: r.get(2)?, category: r.get(3)? }),
            )
            .optional()?)
    }

    // ---- shopping list --------------------------------------------------

    fn shopping_rows(&self, status: &str) -> Result<Vec<ShoppingEntry>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, item_id, text, quantity, unit, status FROM shopping WHERE status = ?1 ORDER BY created_at",
        )?;
        let rows = stmt.query_map(params![status], |r| {
            Ok(ShoppingEntry {
                id: r.get(0)?,
                item_id: r.get(1)?,
                text: r.get(2)?,
                quantity: r.get(3)?,
                unit: r.get(4)?,
                status: r.get(5)?,
                source: "manual",
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Things to buy: entries added by hand, plus one entry per running-low
    /// item that is not already on the list, bought, or dismissed.
    pub fn shopping_list(&self) -> Result<Vec<ShoppingEntry>> {
        let mut out = self.shopping_rows("open")?;
        let running_low = self.supplies_summary()?.running_low;
        let handled: Vec<String> = {
            let conn = self.lock();
            // A dismissed item comes back once it has been restocked and runs low again.
            let low_ids: Vec<&str> = running_low.iter().map(|i| i.id.as_str()).collect();
            let mut stmt = conn.prepare("SELECT id, item_id FROM shopping WHERE status = 'dismissed'")?;
            let dismissed: Vec<(String, Option<String>)> =
                stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<std::result::Result<_, _>>()?;
            for (sid, iid) in dismissed {
                if iid.as_deref().is_none_or(|i| !low_ids.contains(&i)) {
                    conn.execute("DELETE FROM shopping WHERE id = ?1", params![sid])?;
                }
            }
            let mut stmt = conn.prepare("SELECT item_id FROM shopping WHERE item_id IS NOT NULL")?;
            let ids = stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<std::result::Result<Vec<_>, _>>()?;
            ids
        };
        for item in running_low {
            if handled.contains(&item.id) {
                continue;
            }
            let missing = item.min_quantity.unwrap_or(0.0) - item.quantity;
            out.push(ShoppingEntry {
                id: format!("low:{}", item.id),
                item_id: Some(item.id.clone()),
                text: item.name.clone(),
                quantity: Some((missing * 1000.0).round() / 1000.0),
                unit: Some(item.unit.clone()),
                status: "open".into(),
                source: "running_low",
            });
        }
        Ok(out)
    }

    /// Bought things waiting to be put away.
    pub fn to_put_away(&self) -> Result<Vec<ShoppingEntry>> {
        self.shopping_rows("bought")
    }

    pub fn add_shopping(
        &self,
        text: &str,
        quantity: Option<f64>,
        unit: Option<String>,
        item_id: Option<String>,
        actor: &str,
    ) -> Result<ShoppingEntry> {
        self.insert_shopping(text, quantity, unit, item_id, "open", actor)
    }

    fn insert_shopping(
        &self,
        text: &str,
        quantity: Option<f64>,
        unit: Option<String>,
        item_id: Option<String>,
        status: &str,
        actor: &str,
    ) -> Result<ShoppingEntry> {
        let text: String = text.trim().chars().take(120).collect();
        if text.is_empty() {
            bail!("text is required");
        }
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_rfc3339();
        let unit = clean_max(unit, 20);
        let quantity = quantity.filter(|q| q.is_finite()).map(clamp_qty);
        let conn = self.lock();
        conn.execute(
            "INSERT INTO shopping (id, item_id, text, quantity, unit, done, status, created_at, updated_at, updated_by)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7, ?7, ?8)",
            params![id, item_id, text, quantity, unit, status, now, actor],
        )?;
        Ok(ShoppingEntry { id, item_id, text, quantity, unit, status: status.into(), source: "manual" })
    }

    /// Resolve an id from the list: a stored entry, or a computed "low:<item>" one.
    fn low_item(&self, id: &str) -> Result<Option<Item>> {
        match id.strip_prefix("low:") {
            Some(item_id) => self.get_item(item_id),
            None => Ok(None),
        }
    }

    /// "Bought": the entry moves to the things to put away.
    pub fn mark_bought(&self, id: &str, actor: &str) -> Result<bool> {
        if let Some(item) = self.low_item(id)? {
            let missing = (item.min_quantity.unwrap_or(0.0) - item.quantity).max(0.0);
            // Check and insert under one lock: a repeated "bought" (a retried
            // request, two phones) must not put the same thing away twice.
            let conn = self.lock();
            let already: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM shopping WHERE item_id = ?1 AND status IN ('open', 'bought'))",
                    params![item.id],
                    |r| r.get(0),
                )?;
            if already {
                return Ok(false);
            }
            let now = now_rfc3339();
            let quantity = Some(missing).filter(|q| *q > 0.0).map(clamp_qty);
            conn.execute(
                "INSERT INTO shopping (id, item_id, text, quantity, unit, done, status, created_at, updated_at, updated_by)
                 VALUES (?1, ?2, ?3, ?4, ?5, 0, 'bought', ?6, ?6, ?7)",
                params![uuid::Uuid::new_v4().to_string(), item.id, item.name, quantity, item.unit, now, actor],
            )?;
            return Ok(true);
        }
        let conn = self.lock();
        Ok(conn.execute(
            "UPDATE shopping SET status = 'bought', updated_at = ?2, updated_by = ?3 WHERE id = ?1 AND status = 'open'",
            params![id, now_rfc3339(), actor],
        )? > 0)
    }

    /// "Delete" on the shopping list: not bought. A running-low entry stays
    /// away until the item has been restocked and runs low again.
    pub fn dismiss(&self, id: &str, actor: &str) -> Result<bool> {
        if let Some(item) = self.low_item(id)? {
            self.insert_shopping(&item.name, None, None, Some(item.id.clone()), "dismissed", actor)?;
            return Ok(true);
        }
        let conn = self.lock();
        Ok(conn.execute("DELETE FROM shopping WHERE id = ?1 AND status IN ('open', 'bought')", params![id])? > 0)
    }

    /// Put a bought thing away: add a batch to the item (or create the item)
    /// and remove the entry, all in one transaction. A double tap or two
    /// phones add it once, and a failure halfway stores nothing and leaves
    /// the entry waiting, so trying again cannot add it twice. Returns the
    /// item, or `None` when the entry is not (or no longer) waiting.
    pub fn put_away(&self, id: &str, input: PutAwayInput, actor: &str) -> Result<Option<Item>> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        // Claimed and removed in one step; a rollback puts it back.
        let entry: Option<(Option<String>, String, Option<String>)> = tx
            .query_row(
                "DELETE FROM shopping WHERE id = ?1 AND status = 'bought' RETURNING item_id, text, unit",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((entry_item, text, entry_unit)) = entry else { return Ok(None) };
        let item = put_away_tx(&tx, input, actor, entry_item, text, entry_unit)?;
        tx.commit()?;
        Ok(Some(item))
    }

    // ---- history --------------------------------------------------------

    pub fn history(&self, limit: usize) -> Result<Vec<HistoryEntry>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT seq, at, actor, entity, entity_id, action, before_json, after_json
             FROM history ORDER BY seq DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |r| {
            let parse = |s: Option<String>| s.and_then(|t| serde_json::from_str(&t).ok());
            Ok(HistoryEntry {
                seq: r.get(0)?,
                at: r.get(1)?,
                actor: r.get(2)?,
                entity: r.get(3)?,
                entity_id: r.get(4)?,
                action: r.get(5)?,
                before: parse(r.get(6)?),
                after: parse(r.get(7)?),
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(name: &str) -> ItemInput {
        ItemInput { name: Some(name.into()), ..Default::default() }
    }

    #[test]
    fn create_adjust_delete_with_history() {
        let db = Db::open_in_memory().unwrap();
        let mut i = input("Brašno");
        i.quantity = Some(2.0);
        i.unit = Some("kg".into());
        i.category = Some("food".into());
        i.min_quantity = Some(Some(1.0));
        let item = db.create_item(i, "laptop").unwrap();
        assert_eq!(item.quantity, 2.0);
        assert_eq!(item.batches.len(), 1);

        let item = db.adjust_item(&item.id, -1.5, "phone").unwrap().unwrap();
        assert_eq!(item.quantity, 0.5);
        let item = db.adjust_item(&item.id, -5.0, "phone").unwrap().unwrap();
        assert_eq!(item.quantity, 0.0, "never below zero");
        assert!(item.batches.is_empty(), "empty batches disappear");

        let s = db.supplies_summary().unwrap();
        assert_eq!(s.running_low.len(), 1);
        let list = db.shopping_list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].source, "running_low");
        assert_eq!(list[0].quantity, Some(1.0));

        assert!(db.delete_item(&item.id, "laptop").unwrap());
        assert!(db.list_items().unwrap().is_empty());
        let h = db.history(10).unwrap();
        assert_eq!(h.iter().map(|e| e.action.as_str()).collect::<Vec<_>>(), vec!["delete", "consume", "consume", "create"]);
        assert_eq!(h[1].actor.as_deref(), Some("phone"));
    }

    #[test]
    fn using_takes_the_batch_that_expires_first() {
        let db = Db::open_in_memory().unwrap();
        let t = today();
        let mut i = input("Mleko");
        i.quantity = Some(1.0);
        i.unit = Some("l".into());
        i.expiry = Some(Some(plus_days(&t, 20)));
        let item = db.create_item(i, "laptop").unwrap();
        // Two more, expiring sooner; and one without a date.
        let item = db
            .add_batch(&item.id, BatchInput { quantity: Some(2.0), expiry: Some(Some(plus_days(&t, 3))) }, "laptop")
            .unwrap()
            .unwrap();
        let item = db.adjust_item(&item.id, 1.0, "laptop").unwrap().unwrap();
        assert_eq!(item.quantity, 4.0);
        assert_eq!(item.batches.len(), 3);
        assert_eq!(item.expiry.as_deref(), Some(plus_days(&t, 3).as_str()), "earliest expiry shown");
        assert_eq!(item.batches[0].expiry.as_deref(), Some(plus_days(&t, 3).as_str()));
        assert_eq!(item.batches[2].expiry, None, "undated batch last");

        // Using 2.5: the 3-day batch (2) goes, then 0.5 of the 20-day batch.
        let item = db.adjust_item(&item.id, -2.5, "phone").unwrap().unwrap();
        assert_eq!(item.quantity, 1.5);
        assert_eq!(item.batches.len(), 2);
        assert_eq!(item.batches[0].quantity, 0.5);
        assert_eq!(item.expiry.as_deref(), Some(plus_days(&t, 20).as_str()));
    }

    #[test]
    fn batches_can_be_edited_and_removed() {
        let db = Db::open_in_memory().unwrap();
        let mut i = input("Pasulj");
        i.quantity = Some(3.0);
        let item = db.create_item(i, "laptop").unwrap();
        let bid = item.batches[0].id.clone();
        let item = db
            .update_batch(&bid, BatchInput { quantity: Some(5.0), expiry: Some(Some("2028-01-31".into())) }, "laptop")
            .unwrap()
            .unwrap();
        assert_eq!(item.quantity, 5.0);
        assert_eq!(item.expiry.as_deref(), Some("2028-01-31"));
        let bad = BatchInput { quantity: None, expiry: Some(Some("2028-02-30".into())) };
        assert!(db.update_batch(&bid, bad, "laptop").is_err());
        let item = db.delete_batch(&bid, "laptop").unwrap().unwrap();
        assert_eq!(item.quantity, 0.0);
        assert!(item.batches.is_empty());
    }

    #[test]
    fn shopping_bought_put_away_and_dismiss() {
        let db = Db::open_in_memory().unwrap();
        let mut i = input("Brašno");
        i.quantity = Some(1.0);
        i.unit = Some("kg".into());
        i.min_quantity = Some(Some(3.0));
        let flour = db.create_item(i, "laptop").unwrap();

        // Running low shows on the list; "bought" moves it to put away.
        let list = db.shopping_list().unwrap();
        assert_eq!(list[0].id, format!("low:{}", flour.id));
        assert!(db.mark_bought(&list[0].id, "phone").unwrap());
        assert!(db.shopping_list().unwrap().is_empty(), "no longer on the list");
        let away = db.to_put_away().unwrap();
        assert_eq!(away.len(), 1);
        assert_eq!(db.supplies_summary().unwrap().to_put_away, 1);

        // Put away 5 kg with a date: added to flour as a batch.
        let item = db
            .put_away(&away[0].id, PutAwayInput { quantity: 5.0, expiry: Some("2027-06-30".into()), place: Some("pantry".into()), ..Default::default() }, "laptop")
            .unwrap()
            .unwrap();
        assert_eq!(item.id, flour.id);
        assert_eq!(item.quantity, 6.0);
        assert_eq!(item.place.as_deref(), Some("pantry"));
        assert!(db.to_put_away().unwrap().is_empty());
        assert!(db.shopping_list().unwrap().is_empty(), "not low any more");

        // A hand-written entry becomes a new item when put away.
        let bread = db.add_shopping("Hleb", Some(2.0), Some("pcs".into()), None, "phone").unwrap();
        assert!(db.mark_bought(&bread.id, "phone").unwrap());
        let new_item = db.put_away(&bread.id, PutAwayInput { quantity: 2.0, ..Default::default() }, "laptop").unwrap().unwrap();
        assert_eq!(new_item.name, "Hleb");
        assert_eq!(new_item.quantity, 2.0);

        // "Delete" on something not bought: gone; a running-low item stays
        // away until it has been restocked and runs low again.
        let milk = db.add_shopping("Mleko", None, None, None, "phone").unwrap();
        assert!(db.dismiss(&milk.id, "phone").unwrap());
        db.adjust_item(&flour.id, -4.0, "phone").unwrap();
        let low_id = format!("low:{}", flour.id);
        assert!(db.shopping_list().unwrap().iter().any(|e| e.id == low_id));
        assert!(db.dismiss(&low_id, "phone").unwrap());
        assert!(db.shopping_list().unwrap().is_empty(), "dismissed while still low");
        db.adjust_item(&flour.id, 5.0, "phone").unwrap();
        db.shopping_list().unwrap();
        db.adjust_item(&flour.id, -5.0, "phone").unwrap();
        assert!(db.shopping_list().unwrap().iter().any(|e| e.id == low_id), "back after restock + running low again");
    }

    #[test]
    fn concurrent_adjustments_all_count() {
        let db = std::sync::Arc::new(Db::open_in_memory().unwrap());
        let mut i = input("Voda");
        i.quantity = Some(100.0);
        let id = db.create_item(i, "laptop").unwrap().id;
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let (db, id) = (db.clone(), id.clone());
                std::thread::spawn(move || {
                    for _ in 0..10 {
                        db.adjust_item(&id, -1.0, "phone").unwrap();
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(db.get_item(&id).unwrap().unwrap().quantity, 20.0);
        assert_eq!(db.history(200).unwrap().iter().filter(|h| h.action == "consume").count(), 80);
    }

    #[test]
    fn limits() {
        let db = Db::open_in_memory().unwrap();
        let mut i = input("x");
        i.quantity = Some(1e308);
        i.notes = Some(Some("n".repeat(5000)));
        let item = db.create_item(i, "laptop").unwrap();
        assert_eq!(item.quantity, MAX_QUANTITY);
        assert_eq!(item.notes.unwrap().chars().count(), 500);
        let item = db.adjust_item(&item.id, 1e308, "laptop").unwrap().unwrap();
        assert!(item.quantity.is_finite());
        let bad = ItemInput { expiry: Some(Some("2027-02-31".into())), ..Default::default() };
        assert!(db.update_item(&item.id, bad, "laptop").is_err(), "impossible date refused");
    }

    #[test]
    fn edit_and_clear_fields() {
        let db = Db::open_in_memory().unwrap();
        let mut i = input("Mleko");
        i.expiry = Some(Some("2026-10-01".into()));
        i.place = Some(Some("fridge".into()));
        let item = db.create_item(i, "laptop").unwrap();
        let edit = ItemInput { expiry: Some(None), place: Some(Some("pantry".into())), ..Default::default() };
        let item = db.update_item(&item.id, edit, "laptop").unwrap().unwrap();
        assert_eq!(item.expiry, None);
        assert_eq!(item.place.as_deref(), Some("pantry"));
        let edit = ItemInput { quantity: Some(4.0), ..Default::default() };
        assert_eq!(db.update_item(&item.id, edit, "laptop").unwrap().unwrap().quantity, 4.0);
        let bad = ItemInput { expiry: Some(Some("31.12.2026".into())), ..Default::default() };
        assert!(db.update_item(&item.id, bad, "laptop").is_err());
    }

    #[test]
    fn expiry_buckets() {
        let db = Db::open_in_memory().unwrap();
        let t = today();
        for (name, date) in [("old", plus_days(&t, -1)), ("soon", plus_days(&t, 5)), ("later", plus_days(&t, 90))] {
            let mut i = input(name);
            i.expiry = Some(Some(date));
            db.create_item(i, "laptop").unwrap();
        }
        let s = db.supplies_summary().unwrap();
        assert_eq!(s.expired.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), vec!["old"]);
        assert_eq!(s.expiring_soon.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), vec!["soon"]);
    }

    #[test]
    fn barcodes_are_remembered() {
        let db = Db::open_in_memory().unwrap();
        let mut i = input("Pasulj Podravka");
        i.barcode = Some(Some("3850104046254".into()));
        i.unit = Some("pack".into());
        db.create_item(i, "laptop").unwrap();
        let k = db.lookup_barcode("3850104046254").unwrap().unwrap();
        assert_eq!(k.name, "Pasulj Podravka");
        assert_eq!(k.unit.as_deref(), Some("pack"));
        assert!(db.lookup_barcode("000").unwrap().is_none());
        assert_eq!(db.find_item_by_barcode("3850104046254").unwrap().unwrap().batches.len(), 1);
    }

    #[test]
    fn places() {
        let db = Db::open_in_memory().unwrap();
        let p = db.add_place("Vikendica").unwrap();
        assert_eq!(db.add_place("vikendica").unwrap().id, p.id, "no duplicates");
        assert!(db.list_places().unwrap().iter().any(|x| x.name == "Vikendica" && !x.preset));
    }

    #[test]
    fn same_new_place_added_at_once_is_one_place() {
        // Two connections to one file, adding the same new name at the same time.
        let dir = std::env::temp_dir().join(format!("zaklon-places-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("household.db");
        let a = Db::open(&path).unwrap();
        let b = Db::open(&path).unwrap();
        let start = std::sync::Barrier::new(2);
        let (pa, pb) = std::thread::scope(|s| {
            let ta = s.spawn(|| {
                start.wait();
                a.add_place("Garaža").unwrap()
            });
            let tb = s.spawn(|| {
                start.wait();
                b.add_place("garaža").unwrap()
            });
            (ta.join().unwrap(), tb.join().unwrap())
        });
        assert_eq!(pa.id, pb.id);
        assert_eq!(a.list_places().unwrap().iter().filter(|p| p.name.to_lowercase() == "garaža").count(), 1);
        drop((a, b));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn old_databases_are_migrated() {
        // A database from before batches and shopping status existed.
        let dir = std::env::temp_dir().join(format!("zaklon-migrate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("household.db");
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(
                "CREATE TABLE items (id TEXT PRIMARY KEY, name TEXT NOT NULL, quantity REAL NOT NULL DEFAULT 0,
                   unit TEXT NOT NULL DEFAULT 'pcs', category TEXT NOT NULL DEFAULT 'other', place TEXT, expiry TEXT,
                   barcode TEXT, min_quantity REAL, notes TEXT, updated_at TEXT NOT NULL, updated_by TEXT,
                   deleted INTEGER NOT NULL DEFAULT 0);
                 INSERT INTO items (id, name, quantity, expiry, updated_at) VALUES ('a', 'Brašno', 2, '2027-01-31', 'x');
                 CREATE TABLE shopping (id TEXT PRIMARY KEY, item_id TEXT, text TEXT NOT NULL, quantity REAL, unit TEXT,
                   done INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, updated_by TEXT);
                 INSERT INTO shopping (id, text, done, created_at, updated_at) VALUES ('s1', 'Hleb', 1, 'x', 'x');",
            )
            .unwrap();
        }
        let db = Db::open(&path).unwrap();
        let item = db.get_item("a").unwrap().unwrap();
        assert_eq!(item.batches.len(), 1);
        assert_eq!(item.batches[0].quantity, 2.0);
        assert_eq!(item.batches[0].expiry.as_deref(), Some("2027-01-31"));
        assert_eq!(db.to_put_away().unwrap()[0].text, "Hleb", "old done entries count as bought");
        drop(db);
        // Opening again does not add batches twice.
        let db = Db::open(&path).unwrap();
        assert_eq!(db.get_item("a").unwrap().unwrap().batches.len(), 1);
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod review_fixes {
    use super::*;

    fn pa(q: f64) -> PutAwayInput {
        PutAwayInput { quantity: q, expiry: None, place: None, item_id: None, name: None, unit: None, category: None, barcode: None }
    }

    #[test]
    fn a_bought_thing_is_put_away_once() {
        let db = Db::open_in_memory().unwrap();
        let e = db.add_shopping("Sugar", Some(1.0), Some("kg".into()), None, "t").unwrap();
        assert!(db.mark_bought(&e.id, "t").unwrap());
        assert!(db.put_away(&e.id, pa(1.0), "t").unwrap().is_some());
        assert!(db.put_away(&e.id, pa(1.0), "t").unwrap().is_none(), "second tap does nothing");
        let sugar: Vec<Item> = db.list_items().unwrap().into_iter().filter(|i| i.name == "Sugar").collect();
        assert_eq!(sugar.len(), 1);
        assert_eq!(sugar[0].quantity, 1.0);
    }

    #[test]
    fn a_failed_put_away_keeps_the_entry() {
        let db = Db::open_in_memory().unwrap();
        let e = db.add_shopping("Salt", None, None, None, "t").unwrap();
        db.mark_bought(&e.id, "t").unwrap();
        assert!(db.put_away(&e.id, pa(0.0), "t").is_err(), "zero is refused");
        assert_eq!(db.to_put_away().unwrap().len(), 1, "still waiting to be put away");
    }

    #[test]
    fn running_low_is_bought_once() {
        let db = Db::open_in_memory().unwrap();
        let milk = db
            .create_item(ItemInput { name: Some("Milk".into()), quantity: Some(1.0), unit: Some("l".into()), min_quantity: Some(Some(3.0)), ..Default::default() }, "t")
            .unwrap();
        let low = format!("low:{}", milk.id);
        assert!(db.mark_bought(&low, "t").unwrap());
        assert!(!db.mark_bought(&low, "t").unwrap(), "a repeat does not add a second entry");
        assert_eq!(db.to_put_away().unwrap().len(), 1);
    }

    #[test]
    fn a_put_away_that_fails_halfway_stores_nothing() {
        let db = Db::open_in_memory().unwrap();
        let flour = db.create_item(ItemInput { name: Some("Flour".into()), quantity: Some(1.0), ..Default::default() }, "t").unwrap();
        let e = db.add_shopping("Flour", Some(2.0), None, Some(flour.id.clone()), "t").unwrap();
        db.mark_bought(&e.id, "t").unwrap();
        // The place change, after the new batch, fails.
        db.lock()
            .execute_batch("CREATE TRIGGER no_place BEFORE UPDATE OF place ON items BEGIN SELECT RAISE(ABORT, 'disk gone'); END;")
            .unwrap();
        let mut input = pa(2.0);
        input.place = Some("garage".into());
        assert!(db.put_away(&e.id, input.clone(), "t").is_err());
        assert_eq!(db.get_item(&flour.id).unwrap().unwrap().quantity, 1.0, "the batch was not kept");
        assert_eq!(db.to_put_away().unwrap().len(), 1, "still waiting to be put away");

        // Trying again once it works adds it once.
        db.lock().execute_batch("DROP TRIGGER no_place;").unwrap();
        let item = db.put_away(&e.id, input, "t").unwrap().unwrap();
        assert_eq!((item.quantity, item.place.as_deref()), (3.0, Some("garage")));
        assert!(db.to_put_away().unwrap().is_empty());
        assert!(db.put_away(&e.id, pa(2.0), "t").unwrap().is_none());
        assert_eq!(db.get_item(&flour.id).unwrap().unwrap().quantity, 3.0);
    }

    #[test]
    fn an_entry_left_halfway_by_an_older_version_comes_back() {
        let dir = std::env::temp_dir().join(format!("zaklon-putting-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("household.db");
        let db = Db::open(&path).unwrap();
        let milk = db
            .create_item(ItemInput { name: Some("Milk".into()), quantity: Some(1.0), min_quantity: Some(Some(3.0)), ..Default::default() }, "t")
            .unwrap();
        assert!(db.mark_bought(&format!("low:{}", milk.id), "t").unwrap());
        db.lock().execute("UPDATE shopping SET status = 'putting'", []).unwrap();
        assert!(db.to_put_away().unwrap().is_empty(), "hidden, as an older version left it");
        drop(db);

        let db = Db::open(&path).unwrap();
        let away = db.to_put_away().unwrap();
        assert_eq!(away.len(), 1, "back with the things to put away");
        assert_eq!(db.put_away(&away[0].id, pa(2.0), "t").unwrap().unwrap().quantity, 3.0);
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn places_are_read_by_name() {
        let db = Db::open_in_memory().unwrap();
        let cottage = db.add_place("Vikendica").unwrap();
        for (name, place) in [("Lamp", Some(cottage.id.as_str())), ("Aspirin", Some("medicine_cabinet")), ("Rope", Some("the shed")), ("Salt", None)] {
            db.create_item(ItemInput { name: Some(name.into()), place: Some(place.map(str::to_string)), ..Default::default() }, "t").unwrap();
        }
        let place_of = |items: &[Item], name: &str| items.iter().find(|i| i.name == name).unwrap().place.clone();
        let sr = db.list_items_for_reading("sr").unwrap();
        assert_eq!(place_of(&sr, "Lamp").as_deref(), Some("Vikendica"));
        assert_eq!(place_of(&sr, "Aspirin").as_deref(), Some("Kućna apoteka"));
        assert_eq!(place_of(&sr, "Rope").as_deref(), Some("the shed"));
        assert_eq!(place_of(&sr, "Salt"), None);
        let en = db.list_items_for_reading("en").unwrap();
        assert_eq!(place_of(&en, "Aspirin").as_deref(), Some("Medicine cabinet"));
        // The app itself still gets the ids it stores.
        assert_eq!(place_of(&db.list_items().unwrap(), "Lamp"), Some(cottage.id.clone()));

        // A removed place leaves no item pointing at it.
        assert!(db.delete_place(&cottage.id, "t").unwrap());
        assert!(!db.delete_place(&cottage.id, "t").unwrap());
        assert!(!db.delete_place("pantry", "t").unwrap(), "built-in places stay");
        assert_eq!(place_of(&db.list_items().unwrap(), "Lamp"), None);
        let h = db.history(1).unwrap();
        assert_eq!((h[0].action.as_str(), h[0].actor.as_deref()), ("update", Some("t")));
    }

    #[test]
    fn every_built_in_place_has_names() {
        let db = Db::open_in_memory().unwrap();
        let presets: Vec<Place> = db.list_places().unwrap().into_iter().filter(|p| p.preset).collect();
        assert_eq!(presets.len(), PRESET_PLACES.len());
        for p in &presets {
            assert_ne!(place_name(&presets, &p.id, "sr"), p.id);
            assert_ne!(place_name(&presets, &p.id, "en"), p.id);
        }
    }

    #[test]
    fn restocking_from_zero_keeps_the_date() {
        let db = Db::open_in_memory().unwrap();
        let flour = db.create_item(ItemInput { name: Some("Flour".into()), quantity: Some(1.0), ..Default::default() }, "t").unwrap();
        db.adjust_item(&flour.id, -1.0, "t").unwrap();
        let after = db
            .update_item(&flour.id, ItemInput { quantity: Some(3.0), expiry: Some(Some("2027-05-01".into())), ..Default::default() }, "t")
            .unwrap()
            .unwrap();
        assert_eq!(after.quantity, 3.0);
        assert_eq!(after.expiry.as_deref(), Some("2027-05-01"));
    }
}
