//! The supplies as the assistant sees them: the list it answers from, the
//! stored items a question names, and the changes it proposes, in the
//! item's own unit.

use serde::Serialize;
use zaklon_core::supplies::Item;

use super::plan::PlannedChange;
use super::prompts::HISTORY_TURNS;
use super::text::{plain, starts_word, stem};
use super::Turn;

/// A change to the supplies the assistant proposes. Nothing changes until
/// someone confirms it in the app, which then calls the normal supplies API.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Proposal {
    /// "add", "use" or "shopping".
    pub action: String,
    /// The existing item it applies to, if one matches.
    pub item_id: Option<String>,
    /// The item's name as stored, or the new name.
    pub name: String,
    pub quantity: f64,
    pub unit: String,
    pub category: String,
    /// How much is in stock now (existing items).
    pub current: Option<f64>,
}

/// What a spoken name matches in the supplies.
#[derive(Debug)]
pub enum ItemMatch<'a> {
    None,
    One(&'a Item),
    /// Equally good different items: better to ask than to guess.
    Several(Vec<&'a Item>),
}

/// The stored items a spoken name most likely means ("mleka" -> "Mleko 2,8%").
/// The exact name first, then a name or a word of it in the same basic form
/// ("sira" -> "Sir gauda", not "Sirće"), then a word starting with it (only
/// for four letters or more: "so" is not "Sok"), then any part of a name.
pub fn match_items<'a>(name: &str, items: &'a [Item]) -> ItemMatch<'a> {
    let full = plain(name.trim());
    let want = plain(&stem(&full));
    let n = want.chars().count();
    if n < 2 {
        return ItemMatch::None;
    }
    let scored: Vec<(u8, &Item)> = items
        .iter()
        .filter_map(|i| {
            let item = plain(&i.name);
            let score = if item == full {
                5
            } else if stem(&item) == want {
                4
            } else if item.split_whitespace().any(|w| stem(w) == want) {
                3
            } else if n >= 4 && item.split_whitespace().any(|w| w.starts_with(&want)) {
                2
            } else if n >= 3 && item.contains(&want) {
                1
            } else {
                0
            };
            (score > 0).then_some((score, i))
        })
        .collect();
    let Some(best) = scored.iter().map(|(s, _)| *s).max() else { return ItemMatch::None };
    let top: Vec<&Item> = scored.into_iter().filter(|(s, _)| *s == best).map(|(_, i)| i).collect();
    if top.len() == 1 {
        ItemMatch::One(top[0])
    } else {
        ItemMatch::Several(top)
    }
}

/// The one stored item a spoken name means, if it is clear.
pub fn match_item<'a>(name: &str, items: &'a [Item]) -> Option<&'a Item> {
    match match_items(name, items) {
        ItemMatch::One(i) => Some(i),
        _ => None,
    }
}

fn unit_text(unit: &str, sr: bool) -> &str {
    match (unit, sr) {
        ("pcs", true) => "kom",
        ("pcs", false) => "pcs",
        ("pack", true) => "pak.",
        ("pack", false) => "packs",
        (u, _) => u,
    }
}

fn qty_text(q: f64) -> String {
    if (q - q.round()).abs() < 1e-9 {
        format!("{}", q.round() as i64)
    } else {
        format!("{q:.3}").trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// `qty` of `from` in `to`, when both measure the same thing (g and kg, ml and l).
pub fn convert(qty: f64, from: &str, to: &str) -> Option<f64> {
    match (from, to) {
        _ if from == to => Some(qty),
        ("g", "kg") | ("ml", "l") => Some(qty / 1000.0),
        ("kg", "g") | ("l", "ml") => Some(qty * 1000.0),
        _ => None,
    }
}

/// Units that are counted, where one is a fair guess when no amount was said.
fn counted(unit: &str) -> bool {
    matches!(unit, "pcs" | "pack")
}

/// The amount the user said, in the item's own unit (500 g of an item kept
/// in kg is 0.5 kg), or a question back when that cannot be known: pieces of
/// something kept in kg, or no amount of something weighed or measured.
fn amount_for(change: &PlannedChange, item: &Item, sr: bool) -> Result<f64, String> {
    let u = unit_text(&item.unit, sr);
    if change.quantity <= 0.0 {
        if counted(&item.unit) {
            return Ok(1.0);
        }
        return Err(if sr {
            format!("Koliko? U zalihama se „{}“ vodi u {u}.", item.name)
        } else {
            format!("How much? \"{}\" is kept in {u} in the supplies.", item.name)
        });
    }
    convert(change.quantity, &change.unit, &item.unit).ok_or_else(|| {
        if sr {
            format!("U zalihama se „{}“ vodi u {u}. Koliko je to {u}?", item.name)
        } else {
            format!("\"{}\" is kept in {u} in the supplies. How much is that in {u}?", item.name)
        }
    })
}

/// "0.5 kg (500 g)": the amount, and what was said when that was another unit.
fn amount_text(qty: f64, unit: &str, said: Option<&PlannedChange>, sr: bool) -> String {
    let mut t = format!("{} {}", qty_text(qty), unit_text(unit, sr));
    if let Some(c) = said.filter(|c| c.quantity > 0.0 && c.unit != unit) {
        t.push_str(&format!(" ({} {})", qty_text(c.quantity), unit_text(&c.unit, sr)));
    }
    t
}

/// "Na šta misliš: „Sir gauda“ ili „Sirće“?"
fn which_one(list: &[&Item], sr: bool) -> String {
    let names: Vec<String> = list.iter().take(4).map(|i| if sr { format!("„{}“", i.name) } else { format!("\"{}\"", i.name) }).collect();
    let joined = match names.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{}{}{last}", rest.join(", "), if sr { " ili " } else { " or " }),
        _ => names.join(""),
    };
    if sr {
        format!("Na šta misliš: {joined}?")
    } else {
        format!("Which one do you mean: {joined}?")
    }
}

/// What the assistant offers to change, in words, and the change itself.
/// With no proposal, the words ask back (which item, or how much).
pub fn propose(change: &PlannedChange, items: &[Item], language: &str) -> (String, Option<Proposal>) {
    let sr = language == "sr";
    let found = match match_items(&change.name, items) {
        ItemMatch::One(i) => Some(i),
        ItemMatch::None => None,
        // A shopping list entry needs no stored item; anything else must know which one.
        ItemMatch::Several(_) if change.action == "shopping" => None,
        ItemMatch::Several(list) => return (which_one(&list, sr), None),
    };
    match change.action.as_str() {
        "use" => {
            let Some(item) = found else {
                let text = if sr {
                    format!("U zalihama nemam ništa što liči na „{}“.", change.name)
                } else {
                    format!("I found nothing like \"{}\" in the supplies.", change.name)
                };
                return (text, None);
            };
            let qty = match amount_for(change, item, sr) {
                Ok(q) => q,
                Err(ask) => return (ask, None),
            };
            // Never more than there is.
            let used = qty.min(item.quantity);
            let unit = unit_text(&item.unit, sr);
            let amount = amount_text(used, &item.unit, (used == qty).then_some(change), sr);
            let text = if sr {
                format!("Da skinem {amount} sa „{}“? Sada ima {} {unit}.", item.name, qty_text(item.quantity))
            } else {
                format!("Take {amount} off \"{}\"? There are {} {unit} now.", item.name, qty_text(item.quantity))
            };
            let p = Proposal {
                action: "use".into(),
                item_id: Some(item.id.clone()),
                name: item.name.clone(),
                quantity: used,
                unit: item.unit.clone(),
                category: item.category.clone(),
                current: Some(item.quantity),
            };
            (text, Some(p))
        }
        "add" => {
            let (item_id, name, unit, category, current, qty) = match found {
                Some(i) => {
                    let q = match amount_for(change, i, sr) {
                        Ok(q) => q,
                        Err(ask) => return (ask, None),
                    };
                    (Some(i.id.clone()), i.name.clone(), i.unit.clone(), i.category.clone(), Some(i.quantity), q)
                }
                None => {
                    let name = capitalize(&change.name);
                    if change.quantity <= 0.0 && !counted(&change.unit) {
                        let u = unit_text(&change.unit, sr);
                        let ask = if sr { format!("Koliko da dodam „{name}“ (u {u})?") } else { format!("How much \"{name}\" should I add (in {u})?") };
                        return (ask, None);
                    }
                    let q = if change.quantity > 0.0 { change.quantity } else { 1.0 };
                    (None, name, change.unit.clone(), change.category.clone(), None, q)
                }
            };
            let u = unit_text(&unit, sr);
            let amount = amount_text(qty, &unit, current.is_some().then_some(change), sr);
            let text = match (current, sr) {
                (Some(c), true) => format!("Da dodam {amount} u „{name}“? Sada ima {} {u}.", qty_text(c)),
                (Some(c), false) => format!("Add {amount} to \"{name}\"? There are {} {u} now.", qty_text(c)),
                (None, true) => format!("Da dodam novu stavku „{name}“, {amount}?"),
                (None, false) => format!("Add a new item \"{name}\", {amount}?"),
            };
            let p = Proposal { action: "add".into(), item_id, name, quantity: qty, unit, category, current };
            (text, Some(p))
        }
        _ => {
            let (item_id, name) = match found {
                Some(i) => (Some(i.id.clone()), i.name.clone()),
                None => (None, capitalize(&change.name)),
            };
            // What to buy is kept as it was said: "500 g" stays 500 g,
            // whatever unit the stock is kept in.
            let amount = if change.quantity > 0.0 { format!(", {} {}", qty_text(change.quantity), unit_text(&change.unit, sr)) } else { String::new() };
            let text = if sr {
                format!("Da stavim „{name}“{amount} na listu za kupovinu?")
            } else {
                format!("Put \"{name}\"{amount} on the shopping list?")
            };
            let p = Proposal {
                action: "shopping".into(),
                item_id,
                name,
                quantity: change.quantity,
                unit: change.unit.clone(),
                category: change.category.clone(),
                current: None,
            };
            (text, Some(p))
        }
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// A category as the model sees it in the supplies list.
fn category_text(category: &str, sr: bool) -> &str {
    if !sr {
        return category;
    }
    match category {
        "food" => "hrana",
        "drink" => "piće",
        "medicine" => "lek",
        "hygiene" => "higijena",
        "equipment" => "oprema",
        "fuel" => "gorivo",
        "other" => "ostalo",
        c => c,
    }
}

/// The category a word names, for "Šta imam od lekova?".
pub(super) fn category_of_word(word: &str) -> Option<&'static str> {
    const WORDS: &[(&str, &str)] = &[
        ("lek", "medicine"),
        ("medicin", "medicine"),
        ("medication", "medicine"),
        ("drugs", "medicine"),
        ("meds", "medicine"),
        ("tablet", "medicine"),
        ("hran", "food"),
        ("namirnic", "food"),
        ("food", "food"),
        ("pic", "drink"),
        ("napit", "drink"),
        ("drink", "drink"),
        ("higijen", "hygiene"),
        ("hygien", "hygiene"),
        ("oprem", "equipment"),
        ("alat", "equipment"),
        ("equipment", "equipment"),
        ("tool", "equipment"),
        ("goriv", "fuel"),
        ("fuel", "fuel"),
    ];
    let w = plain(word);
    WORDS.iter().find(|(p, _)| starts_word(&[w.as_str()], p)).map(|(_, c)| *c)
}

/// Items in the supplies list given to the model.
const LIST_ITEMS: usize = 80;
/// Items named by a question, repeated next to it.
const NAMED_ITEMS: usize = 30;

/// A stored item as the model sees it, one line with its category.
fn item_line(i: &Item, sr: bool) -> String {
    let mut l = format!("- {}: {} {} ({})", i.name, qty_text(i.quantity), unit_text(&i.unit, sr), category_text(&i.category, sr));
    if let Some(e) = &i.expiry {
        l.push_str(&if sr { format!(", rok {e}") } else { format!(", expires {e}") });
    }
    if let Some(p) = &i.place {
        l.push_str(&if sr { format!(", mesto: {p}") } else { format!(", place: {p}") });
    }
    if let Some(m) = i.min_quantity {
        if i.quantity < m {
            l.push_str(if sr { ", ponestaje" } else { ", running low" });
        }
    }
    l
}

/// The whole supplies list, as the model sees it: what expires first comes
/// first. It is the same for every question until the supplies change, so it
/// goes with the fixed instructions, and the engine keeps what it has read
/// of it from one question to the next (see `supplies_messages`).
pub fn supplies_list(items: &[Item], language: &str) -> String {
    let sr = language == "sr";
    // Local date, like the supplies screen (UTC was a day behind after midnight in Serbia).
    let today = zaklon_core::supplies::today();
    let mut ordered: Vec<&Item> = items.iter().collect();
    ordered.sort_by(|a, b| a.expiry.as_deref().unwrap_or("9999").cmp(b.expiry.as_deref().unwrap_or("9999")));
    let mut lines = vec![if sr { format!("Danas je {today}. Zalihe ({} stavki):", items.len()) } else { format!("Today is {today}. Supplies ({} items):", items.len()) }];
    lines.extend(ordered.into_iter().take(LIST_ITEMS).map(|i| item_line(i, sr)));
    if items.is_empty() {
        lines.push(if sr { "(u zalihama još nema ničega)".into() } else { "(nothing in the supplies yet)".into() });
    }
    lines.join("\n")
}

/// The items a question is about, repeated next to it: those it names, then
/// those of a category it names ("lekovi"). Empty when it names none.
pub fn supplies_named(items: &[Item], terms: &[String], question: &str, language: &str) -> String {
    let sr = language == "sr";
    let stems: Vec<String> = terms.iter().map(|t| stem(&plain(t))).filter(|t| t.chars().count() >= 2).collect();
    let categories: Vec<&str> = terms
        .iter()
        .flat_map(|t| t.split_whitespace())
        .chain(question.split(|c: char| !c.is_alphanumeric()))
        .filter(|w| !w.is_empty())
        .filter_map(category_of_word)
        .collect();
    let by_name = |i: &Item| {
        let n = plain(&i.name);
        stems.iter().any(|s| n.contains(s.as_str()))
    };
    let by_category = |i: &Item| categories.contains(&i.category.as_str());
    let mut named: Vec<&Item> = items.iter().filter(|i| by_name(i)).collect();
    named.extend(items.iter().filter(|i| !by_name(i) && by_category(i)));
    if named.is_empty() {
        return String::new();
    }
    let head = if sr { "Stavke iz zaliha koje pitanje pominje:" } else { "Items in the supplies the question mentions:" };
    let lines: Vec<String> = named.into_iter().take(NAMED_ITEMS).map(|i| item_line(i, sr)).collect();
    format!("{head}\n{}", lines.join("\n"))
}

/// The instructions and the supplies list first, the question last: the
/// engine reads only what follows the part it still has from the last
/// question (the start of the last message), so a list that changes with the
/// question would be read again every time.
pub(super) fn supplies_messages(question: &str, language: &str, list: &str, named: &str, history: &[Turn]) -> Vec<serde_json::Value> {
    let rules = if language == "sr" {
        "Ti si Zaklon, pomoćnik za domaćinstvo. Odgovaraj na srpskom, latinicom, kratko i jasno, i obraćaj se sa „ti“. \
Koristi samo spisak zaliha ispod; ne izmišljaj stavke ni količine. U zagradi posle količine je vrsta stvari (hrana, lek...). \
Ako nečega nema na spisku, reci da toga nema u zalihama. Ne daj savete o lekovima ni dozama."
    } else {
        "You are Zaklon, a household assistant. Answer briefly and clearly. \
Use only the supplies list below; do not invent items or amounts. The word in brackets after the amount is the kind of thing (food, medicine...). \
If something is not on the list, say it is not in the supplies. Do not give advice on medicines or doses."
    };
    let mut messages = vec![serde_json::json!({ "role": "system", "content": format!("{rules}\n\n{list}") })];
    for t in history.iter().rev().take(HISTORY_TURNS).rev() {
        messages.push(serde_json::json!({ "role": "user", "content": t.question }));
        messages.push(serde_json::json!({ "role": "assistant", "content": t.answer }));
    }
    let user = if named.is_empty() { question.to_string() } else { format!("{named}\n\n{question}") };
    messages.push(serde_json::json!({ "role": "user", "content": user }));
    messages
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::test_util::item;

    #[test]
    fn spoken_names_find_stored_items() {
        let items = vec![item("Mleko 2,8%", 1.0, "l"), item("Pasulj tetovac", 2.0, "kg"), item("Brašno", 5.0, "kg")];
        assert_eq!(match_item("mleka", &items).unwrap().name, "Mleko 2,8%");
        assert_eq!(match_item("pasulj", &items).unwrap().name, "Pasulj tetovac");
        assert_eq!(match_item("brasno", &items).unwrap().name, "Brašno");
        assert!(match_item("šećer", &items).is_none());
    }

    #[test]
    fn proposals_say_what_will_change() {
        let items = vec![item("Mleko", 1.0, "l"), item("Pasulj", 2.0, "pcs")];
        let add = PlannedChange { action: "add".into(), name: "mleko".into(), quantity: 2.0, unit: "l".into(), category: "drink".into() };
        let (text, p) = propose(&add, &items, "sr");
        assert_eq!(text, "Da dodam 2 l u „Mleko“? Sada ima 1 l.");
        assert_eq!(p.unwrap().item_id.as_deref(), Some("id-Mleko"));
        let used = PlannedChange { action: "use".into(), name: "pasulja".into(), quantity: 3.0, unit: "pcs".into(), category: "food".into() };
        let (text, p) = propose(&used, &items, "sr");
        assert_eq!(text, "Da skinem 2 kom sa „Pasulj“? Sada ima 2 kom.");
        assert_eq!(p.unwrap().quantity, 2.0, "never more than there is");
        let new = PlannedChange { action: "add".into(), name: "šećer".into(), quantity: 2.0, unit: "kg".into(), category: "food".into() };
        let (text, p) = propose(&new, &items, "en");
        assert_eq!(text, "Add a new item \"Šećer\", 2 kg?");
        assert!(p.unwrap().item_id.is_none());
        let counted = PlannedChange { action: "add".into(), name: "sveća".into(), quantity: 0.0, unit: "pcs".into(), category: "other".into() };
        assert_eq!(propose(&counted, &items, "sr").0, "Da dodam novu stavku „Sveća“, 1 kom?", "one is a fair guess for pieces");
        let missing = PlannedChange { action: "use".into(), name: "so".into(), quantity: 1.0, unit: "kg".into(), category: "food".into() };
        assert!(propose(&missing, &items, "sr").1.is_none());
        let shop = PlannedChange { action: "shopping".into(), name: "hleb".into(), quantity: 0.0, unit: "pcs".into(), category: "food".into() };
        assert_eq!(propose(&shop, &items, "sr").0, "Da stavim „Hleb“ na listu za kupovinu?");
    }

    #[test]
    fn supplies_are_listed_the_same_for_every_question() {
        let mut mleko = item("Mleko", 1.0, "l");
        mleko.expiry = Some("2026-10-01".into());
        let items = vec![item("Brašno", 5.0, "kg"), mleko];
        let list = supplies_list(&items, "sr");
        let lines: Vec<&str> = list.lines().collect();
        assert!(lines[0].starts_with("Danas je 20") && lines[0].ends_with("Zalihe (2 stavki):"), "{list}");
        assert_eq!(lines[1], "- Mleko: 1 l (hrana), rok 2026-10-01", "what expires first comes first");
        assert_eq!(lines[2], "- Brašno: 5 kg (hrana)");
        assert!(supplies_list(&[], "en").ends_with("(nothing in the supplies yet)"));
        // The items a question names are repeated next to it.
        let named = supplies_named(&items, &["mleko".into()], "Koliko imamo mleka?", "sr");
        assert_eq!(named, "Stavke iz zaliha koje pitanje pominje:\n- Mleko: 1 l (hrana), rok 2026-10-01");
        assert_eq!(supplies_named(&items, &["kafa".into()], "Imamo li kafe?", "sr"), "", "nothing named");
    }

    #[test]
    fn supplies_named_include_a_named_category() {
        let mut brufen = item("Brufen", 2.0, "pcs");
        brufen.category = "medicine".into();
        let mut sveca = item("Sveća", 10.0, "pcs");
        sveca.category = "other".into();
        let items = vec![item("Brašno", 5.0, "kg"), sveca, brufen];
        let c = supplies_named(&items, &[], "Šta imam od lekova?", "sr");
        assert_eq!(c.lines().nth(1), Some("- Brufen: 2 kom (lek)"), "{c}");
        assert_eq!(c.lines().count(), 2, "{c}");
        let en = supplies_named(&items, &["medicines".into()], "What medicines do we have?", "en");
        assert_eq!(en.lines().nth(1), Some("- Brufen: 2 pcs (medicine)"), "{en}");
    }

    #[test]
    fn the_supplies_list_stays_ahead_of_the_question() {
        // The engine keeps what it read up to the last message: the list
        // must not change from one question to the next.
        let items = vec![item("Brašno", 5.0, "kg"), item("Baterije AA", 8.0, "pcs")];
        let list = supplies_list(&items, "sr");
        let ask = |q: &str, terms: &[&str]| {
            let terms: Vec<String> = terms.iter().map(|t| t.to_string()).collect();
            supplies_messages(q, "sr", &list, &supplies_named(&items, &terms, q, "sr"), &[])
        };
        let (a, b) = (ask("koliko imamo brasna", &["brasn"]), ask("jel imamo baterija za lampu", &["baterij"]));
        assert_eq!(a[0], b[0], "the same instructions and list");
        let system = a[0]["content"].as_str().unwrap();
        assert!(system.contains("Koristi samo spisak zaliha ispod") && system.ends_with("- Baterije AA: 8 kom (hrana)"), "{system}");
        assert_eq!(a.len(), 2);
        let user = a[1]["content"].as_str().unwrap();
        assert!(user.starts_with("Stavke iz zaliha koje pitanje pominje:\n- Brašno: 5 kg (hrana)") && user.ends_with("\n\nkoliko imamo brasna"), "{user}");
        let history = [Turn { question: "q".into(), answer: "a".into() }];
        let with_history = supplies_messages("Šta nam ističe?", "sr", &list, "", &history);
        assert_eq!(with_history.len(), 4);
        assert_eq!(with_history[3]["content"], "Šta nam ističe?", "nothing named: the question alone");
    }

    #[test]
    fn short_names_find_the_right_item() {
        let items = vec![item("Sir gauda", 1.0, "kg"), item("Sirće", 1.0, "l"), item("Kuhinjska so", 1.0, "kg"), item("Sok", 2.0, "l")];
        assert_eq!(match_item("sira", &items).unwrap().name, "Sir gauda");
        assert_eq!(match_item("so", &items).unwrap().name, "Kuhinjska so");
        assert_eq!(match_item("soka", &items).unwrap().name, "Sok");
        let oils = vec![item("Suncokretovo ulje", 1.0, "l"), item("Maslinovo ulje", 0.5, "l")];
        let used = PlannedChange { action: "use".into(), name: "ulje".into(), quantity: 0.5, unit: "l".into(), category: "food".into() };
        let (text, p) = propose(&used, &oils, "sr");
        assert_eq!(text, "Na šta misliš: „Suncokretovo ulje“ ili „Maslinovo ulje“?");
        assert!(p.is_none());
        let shop = PlannedChange { action: "shopping".into(), ..used };
        assert_eq!(propose(&shop, &oils, "sr").0, "Da stavim „Ulje“, 0.5 l na listu za kupovinu?", "the list does not need to know which");
    }

    #[test]
    fn amounts_are_converted_to_the_stored_unit() {
        let items = vec![item("Brašno", 2.0, "kg"), item("Kafa", 1.0, "kg"), item("Šećer", 1000.0, "g"), item("Pasulj", 3.0, "pcs")];
        let change = |action: &str, name: &str, quantity: f64, unit: &str| PlannedChange {
            action: action.into(),
            name: name.into(),
            quantity,
            unit: unit.into(),
            category: "food".into(),
        };
        let (text, p) = propose(&change("use", "brašna", 500.0, "g"), &items, "sr");
        assert_eq!(text, "Da skinem 0.5 kg (500 g) sa „Brašno“? Sada ima 2 kg.");
        let p = p.unwrap();
        assert_eq!((p.quantity, p.unit.as_str()), (0.5, "kg"));
        let (text, p) = propose(&change("add", "kafe", 250.0, "g"), &items, "sr");
        assert_eq!(text, "Da dodam 0.25 kg (250 g) u „Kafa“? Sada ima 1 kg.");
        assert_eq!(p.unwrap().quantity, 0.25);
        let (text, p) = propose(&change("use", "šećera", 0.5, "kg"), &items, "en");
        assert_eq!(text, "Take 500 g (0.5 kg) off \"Šećer\"? There are 1000 g now.");
        assert_eq!(p.unwrap().quantity, 500.0);
        let (text, p) = propose(&change("shopping", "brašno", 500.0, "g"), &items, "sr");
        assert_eq!(text, "Da stavim „Brašno“, 500 g na listu za kupovinu?", "what to buy stays as it was said");
        assert_eq!(p.unwrap().unit, "g");
        // Pieces of something weighed: ask, do not guess.
        let (text, p) = propose(&change("add", "brašno", 2.0, "pcs"), &items, "sr");
        assert_eq!(text, "U zalihama se „Brašno“ vodi u kg. Koliko je to kg?");
        assert!(p.is_none());
        let (text, p) = propose(&change("use", "brašno", 0.0, "kg"), &items, "sr");
        assert_eq!(text, "Koliko? U zalihama se „Brašno“ vodi u kg.");
        assert!(p.is_none());
        let (text, p) = propose(&change("add", "pasulj", 0.0, "pcs"), &items, "sr");
        assert_eq!(text, "Da dodam 1 kom u „Pasulj“? Sada ima 3 kom.");
        assert!(p.is_some());
        let (text, p) = propose(&change("add", "so", 0.0, "kg"), &items, "sr");
        assert_eq!(text, "Koliko da dodam „So“ (u kg)?");
        assert!(p.is_none());
        assert_eq!(convert(1.5, "l", "ml"), Some(1500.0));
        assert_eq!(convert(1.0, "pack", "pcs"), None);
    }
}
