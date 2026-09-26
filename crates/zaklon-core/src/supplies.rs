//! Household supplies: items, places, remembered barcodes, the shopping list
//! and the change history. Everything here is shared by the whole household.

use anyhow::{bail, Result};
use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use crate::db::{now_rfc3339, Db};

pub const CATEGORIES: &[&str] = &["food", "drink", "medicine", "hygiene", "equipment", "fuel", "other"];
pub const UNITS: &[&str] = &["pcs", "kg", "g", "l", "ml", "pack"];
pub const PRESET_PLACES: &[&str] = &["pantry", "fridge", "freezer", "medicine_cabinet", "garage", "basement"];

/// Items expiring within this many days show up as "expiring soon".
pub const EXPIRING_DAYS: i64 = 30;

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
CREATE INDEX IF NOT EXISTS items_expiry ON items(expiry);
"#;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Item {
    pub id: String,
    pub name: String,
    pub quantity: f64,
    pub unit: String,
    pub category: String,
    pub place: Option<String>,
    /// ISO date "YYYY-MM-DD".
    pub expiry: Option<String>,
    pub barcode: Option<String>,
    pub min_quantity: Option<f64>,
    pub notes: Option<String>,
    pub updated_at: String,
    pub updated_by: Option<String>,
}

/// Fields a caller may set when creating or editing an item. For edits,
/// `None` leaves a field unchanged; `Some(None)`/empty string clears it.
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
    pub done: bool,
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
}

fn clean(s: Option<String>) -> Option<String> {
    s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn valid_date(d: &str) -> bool {
    let b = d.as_bytes();
    d.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && d.chars().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
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
    })
}

const ITEM_COLS: &str =
    "id, name, quantity, unit, category, place, expiry, barcode, min_quantity, notes, updated_at, updated_by";

fn today() -> String {
    let d = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc()).date();
    format!("{:04}-{:02}-{:02}", d.year(), u8::from(d.month()), d.day())
}

fn plus_days(date: &str, days: i64) -> String {
    let fmt = time::macros::format_description!("[year]-[month]-[day]");
    match time::Date::parse(date, &fmt) {
        Ok(d) => {
            let n = d + time::Duration::days(days);
            format!("{:04}-{:02}-{:02}", n.year(), u8::from(n.month()), n.day())
        }
        Err(_) => date.to_string(),
    }
}

impl Db {
    // ---- items ----------------------------------------------------------

    pub fn list_items(&self) -> Result<Vec<Item>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {ITEM_COLS} FROM items WHERE deleted = 0 ORDER BY name COLLATE NOCASE"
        ))?;
        let rows = stmt.query_map([], row_item)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn get_item(&self, id: &str) -> Result<Option<Item>> {
        let conn = self.lock();
        Ok(conn
            .query_row(&format!("SELECT {ITEM_COLS} FROM items WHERE id = ?1 AND deleted = 0"), params![id], row_item)
            .optional()?)
    }

    pub fn find_item_by_barcode(&self, barcode: &str) -> Result<Option<Item>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                &format!("SELECT {ITEM_COLS} FROM items WHERE barcode = ?1 AND deleted = 0 ORDER BY updated_at DESC LIMIT 1"),
                params![barcode],
                row_item,
            )
            .optional()?)
    }

    pub fn create_item(&self, input: ItemInput, actor: &str) -> Result<Item> {
        let name = clean(input.name).ok_or_else(|| anyhow::anyhow!("name is required"))?;
        let unit = clean(input.unit).unwrap_or_else(|| "pcs".into());
        let category = clean(input.category).unwrap_or_else(|| "other".into());
        if !CATEGORIES.contains(&category.as_str()) {
            bail!("unknown category");
        }
        let expiry = input.expiry.flatten().and_then(|e| clean(Some(e)));
        if let Some(e) = &expiry {
            if !valid_date(e) {
                bail!("expiry must be a date like 2027-03-31");
            }
        }
        let quantity = input.quantity.unwrap_or(1.0).max(0.0);
        let item = Item {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.chars().take(120).collect(),
            quantity,
            unit: unit.chars().take(20).collect(),
            category,
            place: clean(input.place.flatten()),
            expiry,
            barcode: clean(input.barcode.flatten()),
            min_quantity: input.min_quantity.flatten().filter(|m| *m >= 0.0),
            notes: clean(input.notes.flatten()),
            updated_at: now_rfc3339(),
            updated_by: Some(actor.to_string()),
        };
        {
            let conn = self.lock();
            conn.execute(
                &format!("INSERT INTO items ({ITEM_COLS}, deleted) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 0)"),
                params![
                    item.id, item.name, item.quantity, item.unit, item.category, item.place, item.expiry, item.barcode,
                    item.min_quantity, item.notes, item.updated_at, item.updated_by
                ],
            )?;
        }
        if let Some(code) = &item.barcode {
            self.remember_barcode(code, &item.name, Some(&item.unit), Some(&item.category))?;
        }
        self.record("item", &item.id, "create", actor, None, Some(&item))?;
        Ok(item)
    }

    pub fn update_item(&self, id: &str, input: ItemInput, actor: &str) -> Result<Option<Item>> {
        let Some(before) = self.get_item(id)? else { return Ok(None) };
        let mut after = before.clone();
        if let Some(n) = clean(input.name) {
            after.name = n.chars().take(120).collect();
        }
        if let Some(q) = input.quantity {
            after.quantity = q.max(0.0);
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
            after.place = clean(p);
        }
        if let Some(e) = input.expiry {
            let e = clean(e);
            if let Some(d) = &e {
                if !valid_date(d) {
                    bail!("expiry must be a date like 2027-03-31");
                }
            }
            after.expiry = e;
        }
        if let Some(b) = input.barcode {
            after.barcode = clean(b);
        }
        if let Some(m) = input.min_quantity {
            after.min_quantity = m.filter(|v| *v >= 0.0);
        }
        if let Some(n) = input.notes {
            after.notes = clean(n);
        }
        if after == before {
            return Ok(Some(before));
        }
        after.updated_at = now_rfc3339();
        after.updated_by = Some(actor.to_string());
        {
            let conn = self.lock();
            conn.execute(
                "UPDATE items SET name=?2, quantity=?3, unit=?4, category=?5, place=?6, expiry=?7, barcode=?8,
                 min_quantity=?9, notes=?10, updated_at=?11, updated_by=?12 WHERE id=?1",
                params![
                    after.id, after.name, after.quantity, after.unit, after.category, after.place, after.expiry,
                    after.barcode, after.min_quantity, after.notes, after.updated_at, after.updated_by
                ],
            )?;
        }
        if let Some(code) = &after.barcode {
            self.remember_barcode(code, &after.name, Some(&after.unit), Some(&after.category))?;
        }
        self.record("item", id, "update", actor, Some(&before), Some(&after))?;
        Ok(Some(after))
    }

    /// Add to or take from the quantity; never below zero.
    pub fn adjust_item(&self, id: &str, delta: f64, actor: &str) -> Result<Option<Item>> {
        let Some(before) = self.get_item(id)? else { return Ok(None) };
        let mut after = before.clone();
        after.quantity = ((before.quantity + delta) * 1000.0).round() / 1000.0;
        if after.quantity < 0.0 {
            after.quantity = 0.0;
        }
        after.updated_at = now_rfc3339();
        after.updated_by = Some(actor.to_string());
        {
            let conn = self.lock();
            conn.execute(
                "UPDATE items SET quantity=?2, updated_at=?3, updated_by=?4 WHERE id=?1",
                params![after.id, after.quantity, after.updated_at, after.updated_by],
            )?;
        }
        let action = if delta < 0.0 { "consume" } else { "add" };
        self.record("item", id, action, actor, Some(&before), Some(&after))?;
        Ok(Some(after))
    }

    /// Soft delete: the row stays for history and sync, hidden from lists.
    pub fn delete_item(&self, id: &str, actor: &str) -> Result<bool> {
        let Some(before) = self.get_item(id)? else { return Ok(false) };
        {
            let conn = self.lock();
            conn.execute(
                "UPDATE items SET deleted=1, updated_at=?2, updated_by=?3 WHERE id=?1",
                params![id, now_rfc3339(), actor],
            )?;
            conn.execute("DELETE FROM shopping WHERE item_id = ?1", params![id])?;
        }
        self.record::<Item>("item", id, "delete", actor, Some(&before), None)?;
        Ok(true)
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
        let running_low: Vec<Item> = items
            .iter()
            .filter(|i| i.min_quantity.is_some_and(|m| i.quantity < m))
            .cloned()
            .collect();
        Ok(Summary { total_items: items.len() as i64, expired, expiring_soon: expiring, running_low })
    }

    // ---- places ---------------------------------------------------------

    pub fn list_places(&self) -> Result<Vec<Place>> {
        let mut out: Vec<Place> =
            PRESET_PLACES.iter().map(|p| Place { id: p.to_string(), name: p.to_string(), preset: true }).collect();
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
        if let Some(existing) = conn
            .query_row("SELECT id, name FROM places WHERE name = ?1 COLLATE NOCASE", params![name], |r| {
                Ok(Place { id: r.get(0)?, name: r.get(1)?, preset: false })
            })
            .optional()?
        {
            return Ok(existing);
        }
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute("INSERT INTO places (id, name, created_at) VALUES (?1, ?2, ?3)", params![id, name, now_rfc3339()])?;
        Ok(Place { id, name, preset: false })
    }

    pub fn delete_place(&self, id: &str) -> Result<bool> {
        let conn = self.lock();
        Ok(conn.execute("DELETE FROM places WHERE id = ?1", params![id])? > 0)
    }

    // ---- barcodes -------------------------------------------------------

    pub fn remember_barcode(&self, barcode: &str, name: &str, unit: Option<&str>, category: Option<&str>) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO barcodes (barcode, name, unit, category, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(barcode) DO UPDATE SET name=excluded.name, unit=excluded.unit,
               category=excluded.category, updated_at=excluded.updated_at",
            params![barcode.trim(), name, unit, category, now_rfc3339()],
        )?;
        Ok(())
    }

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

    /// Manual entries plus one computed entry per running-low item that is not
    /// already on the list.
    pub fn shopping_list(&self) -> Result<Vec<ShoppingEntry>> {
        let mut out: Vec<ShoppingEntry> = {
            let conn = self.lock();
            let mut stmt = conn.prepare(
                "SELECT id, item_id, text, quantity, unit, done FROM shopping ORDER BY done, created_at",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(ShoppingEntry {
                    id: r.get(0)?,
                    item_id: r.get(1)?,
                    text: r.get(2)?,
                    quantity: r.get(3)?,
                    unit: r.get(4)?,
                    done: r.get::<_, i64>(5)? != 0,
                    source: "manual",
                })
            })?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        for item in self.supplies_summary()?.running_low {
            if out.iter().any(|e| e.item_id.as_deref() == Some(item.id.as_str())) {
                continue;
            }
            let missing = item.min_quantity.unwrap_or(0.0) - item.quantity;
            out.push(ShoppingEntry {
                id: format!("low:{}", item.id),
                item_id: Some(item.id.clone()),
                text: item.name.clone(),
                quantity: Some((missing * 1000.0).round() / 1000.0),
                unit: Some(item.unit.clone()),
                done: false,
                source: "running_low",
            });
        }
        Ok(out)
    }

    pub fn add_shopping(
        &self,
        text: &str,
        quantity: Option<f64>,
        unit: Option<String>,
        item_id: Option<String>,
        actor: &str,
    ) -> Result<ShoppingEntry> {
        let text: String = text.trim().chars().take(120).collect();
        if text.is_empty() {
            bail!("text is required");
        }
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_rfc3339();
        let unit = clean(unit);
        let conn = self.lock();
        conn.execute(
            "INSERT INTO shopping (id, item_id, text, quantity, unit, done, created_at, updated_at, updated_by)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?6, ?7)",
            params![id, item_id, text, quantity, unit, now, actor],
        )?;
        Ok(ShoppingEntry { id, item_id, text, quantity, unit, done: false, source: "manual" })
    }

    pub fn set_shopping_done(&self, id: &str, done: bool, actor: &str) -> Result<bool> {
        let conn = self.lock();
        Ok(conn.execute(
            "UPDATE shopping SET done=?2, updated_at=?3, updated_by=?4 WHERE id=?1",
            params![id, done as i64, now_rfc3339(), actor],
        )? > 0)
    }

    pub fn delete_shopping(&self, id: &str) -> Result<bool> {
        let conn = self.lock();
        Ok(conn.execute("DELETE FROM shopping WHERE id = ?1", params![id])? > 0)
    }

    pub fn clear_done_shopping(&self) -> Result<usize> {
        let conn = self.lock();
        Ok(conn.execute("DELETE FROM shopping WHERE done = 1", [])?)
    }

    // ---- history --------------------------------------------------------

    fn record<T: Serialize>(
        &self,
        entity: &str,
        entity_id: &str,
        action: &str,
        actor: &str,
        before: Option<&T>,
        after: Option<&T>,
    ) -> Result<()> {
        let b = before.map(serde_json::to_string).transpose()?;
        let a = after.map(serde_json::to_string).transpose()?;
        let conn = self.lock();
        conn.execute(
            "INSERT INTO history (at, actor, entity, entity_id, action, before_json, after_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![now_rfc3339(), actor, entity, entity_id, action, b, a],
        )?;
        Ok(())
    }

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

        let item = db.adjust_item(&item.id, -1.5, "phone").unwrap().unwrap();
        assert_eq!(item.quantity, 0.5);
        let item = db.adjust_item(&item.id, -5.0, "phone").unwrap().unwrap();
        assert_eq!(item.quantity, 0.0, "never below zero");

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
    }

    #[test]
    fn places_and_shopping() {
        let db = Db::open_in_memory().unwrap();
        let p = db.add_place("Vikendica").unwrap();
        assert_eq!(db.add_place("vikendica").unwrap().id, p.id, "no duplicates");
        assert!(db.list_places().unwrap().iter().any(|x| x.name == "Vikendica" && !x.preset));
        let e = db.add_shopping("Hleb", Some(2.0), None, None, "laptop").unwrap();
        assert!(db.set_shopping_done(&e.id, true, "phone").unwrap());
        assert_eq!(db.clear_done_shopping().unwrap(), 1);
        assert!(db.shopping_list().unwrap().is_empty());
    }
}
