//! What the assistant offers under an answer: the app's tools and the guides
//! for the question's topics, and, for a question that asks how much is
//! needed, a calculator already filled in with what the question says ("a
//! fridge, 6 LED bulbs and a laptop for 3 days"). All of it is read from the
//! question's words (see `topics`), never by the model: the model may give
//! rough numbers, but the sizing is done in the calculator, and the button
//! that opens it with the right list must not depend on a small model.

use serde::Serialize;

use super::plan::{mentions_supplies, plain_supplies_question, remember_request};
use super::topics::{amount, follow_up, has_any, phrase_at, small_talk, tokens, topics_in, word_is, Tok, COLLECTIVE};
use super::Turn;

/// Suggestions under one answer, at most; tools take at most `MAX_TOOLS` of them.
pub(super) const MAX_SUGGESTIONS: usize = 3;
const MAX_TOOLS: usize = 2;

/// One suggestion under an answer: a tool of the app, or the guides of a
/// topic (its folder in Add-ons).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Suggestion {
    /// "tool" or "guides".
    pub kind: &'static str,
    /// The tool (its id in ui/src/tools.ts), or for guides the topic.
    pub id: &'static str,
    pub topic: &'static str,
    /// The app's address it opens: "#power?items=fridge:1&days=3", "#addons/garden".
    pub link: String,
    /// Its text in the app (ui/src/i18n.ts); "{topic}" in it is the topic's name.
    pub label_key: &'static str,
}

/// What the hub reads from a question by its words: the topics it is about
/// (for the library search) and what to suggest under the answer. An
/// earlier question of the conversation counts for one that goes on from
/// it ("A za 5 dana?"). Small talk and questions the supplies or the
/// household's notes answer get nothing.
pub fn read_question(question: &str, history: &[Turn]) -> (Vec<&'static str>, Vec<Suggestion>) {
    if plain_supplies_question(question) || mentions_supplies(question) || remember_request(question).is_some() {
        return (Vec::new(), Vec::new());
    }
    let toks = tokens(question);
    if small_talk(&toks) {
        return (Vec::new(), Vec::new());
    }
    let mut topics = topics_of(question);
    // The questions amounts are read from, the earliest first.
    let mut said: Vec<&str> = vec![question];
    let own = !topics.is_empty();
    if !own || follow_up(&toks) {
        let before = history.iter().rev().map(|t| t.question.as_str()).find(|q| !topics_of(q).is_empty());
        if let Some(before) = before {
            let earlier = topics_of(before);
            if !own && follow_up(&toks) {
                topics = earlier;
                said.insert(0, before);
            } else if own && earlier.iter().any(|t| topics.contains(t)) {
                said.insert(0, before);
            }
        }
    }
    let suggestions = suggest(&topics, &said);
    (topics, suggestions)
}

/// The calculators among the suggestions, which the model is told about.
pub(super) fn calculators(suggestions: &[Suggestion]) -> Vec<&'static str> {
    suggestions.iter().filter(|s| s.kind == "tool" && (s.id == POWER_TOOL || s.id == WATER_TOOL)).map(|s| s.id).collect()
}

/// A text's topics, with power for one that names appliances to keep going
/// ("a fridge, 6 LED bulbs and a laptop for 3 days") without saying power:
/// two kinds of them, or one with an amount or a time, unless it is about
/// food ("Can I keep milk in the fridge for 3 days?").
fn topics_of(text: &str) -> Vec<&'static str> {
    let mut topics = topics_in(text);
    if topics.contains(&"power") || topics.contains(&"food") {
        return topics;
    }
    let ask = power_ask(&tokens(text), english(text));
    let kinds = ask.items.len();
    let amounts = ask.days.is_some() || ask.items.iter().any(|(_, n, h)| *n > 1 || h.is_some());
    if kinds >= 2 {
        topics.insert(0, "power");
    } else if kinds == 1 && amounts {
        topics.push("power");
    }
    topics
}

fn english(text: &str) -> bool {
    zaklon_core::lang::question_language(text) == Some("en")
}

/// Words that ask how much is needed.
const SIZING: &[&str] = &[
    "kolik*", "kolko", "dovoljn*", "potrebn*", "izracun*", "proracun*", "dimenzionis*", "how much", "how many", "how big", "how long", "how large",
    "what size", "size", "sizing", "enough", "calculat*",
];

/// Words about keeping food, for the Supplies tool.
const STORING: &[&str] = &[
    "zalih*", "cuva*", "skladist*", "ostav*", "rok trajanja", "istek*", "istic*", "store", "storing", "storage", "stock*", "stockpil*",
    "pantry", "shelf life", "expir*",
];

fn tool(id: &'static str, topic: &'static str, link: String, label_key: &'static str) -> Suggestion {
    Suggestion { kind: "tool", id, topic, link, label_key }
}

/// The suggestions for the topics, best first: a tool for a topic that has
/// one (a calculator filled in from what was said, when it says enough),
/// then the guides of each topic.
fn suggest(topics: &[&'static str], said: &[&str]) -> Vec<Suggestion> {
    let all: Vec<Vec<Tok>> = said.iter().map(|q| tokens(q)).collect();
    let Some(now) = all.last() else { return Vec::new() };
    let sizing = all.iter().any(|t| has_any(t, SIZING));
    let mut out: Vec<Suggestion> = Vec::new();
    for &topic in topics {
        let s = match topic {
            "power" => power_suggestion(said, sizing),
            "water" | "garden" => water_suggestion(topic, &all, sizing),
            "food" if has_any(now, STORING) => Some(tool("supplies", "food", "#supplies".into(), "supplies")),
            "knowledge" => Some(tool("library", "knowledge", "#library".into(), "library")),
            "maps" => Some(tool("maps", "maps", "#maps".into(), "maps")),
            _ => None,
        };
        if let Some(s) = s {
            if out.len() < MAX_TOOLS && !out.iter().any(|o| o.id == s.id) {
                out.push(s);
            }
        }
    }
    for &topic in topics {
        if out.len() >= MAX_SUGGESTIONS {
            break;
        }
        let label_key = if topic == "maps" { "topicMapsGuides" } else { "aiSugGuides" };
        out.push(Suggestion { kind: "guides", id: topic, topic, link: format!("#addons/{topic}"), label_key });
    }
    out.truncate(MAX_SUGGESTIONS);
    out
}

// ---- Numbers as people say them ----------------------------------------------------

/// A word that is one of the keys.
fn unit_is(t: &Tok, keys: &[&str]) -> bool {
    matches!(t, Tok::Word(w) if keys.iter().any(|k| word_is(w, k)))
}

/// "12", "12.5", "0.33": as short as it can be written.
fn num(x: f64) -> String {
    let r = (x * 100.0).round() / 100.0;
    if r.fract() == 0.0 {
        format!("{}", r as i64)
    } else {
        format!("{r}")
    }
}

const HOUR_UNITS: &[&str] = &["h", "sat", "sata", "sati", "casa", "casova", "hour", "hours", "hr", "hrs"];
const DAY_UNITS: &[&str] = &["dan", "dana", "dani", "day", "days", "noc", "noci", "night", "nights"];
const WEEK_UNITS: &[&str] = &["nedelj*", "sedmic*", "week", "weeks"];
const MONTH_UNITS: &[&str] = &["mesec", "meseca", "meseci", "month", "months"];

/// How long something must last, in days: "3 dana", "nedelju dana", "two
/// weeks", "72 sata" (the last one said counts). Tokens in `used` were read
/// as an appliance's hours a day.
fn duration(toks: &[Tok], used: &[usize]) -> Option<f64> {
    let mut found = None;
    for (i, t) in toks.iter().enumerate() {
        if used.contains(&i) {
            continue;
        }
        let n = i.checked_sub(1).filter(|j| !used.contains(j)).and_then(|j| amount(&toks[j]));
        // "nedelju dana", "mesec dana": a week, a month.
        let dana = matches!(toks.get(i + 1), Some(Tok::Word(w)) if w == "dana");
        let days = if unit_is(t, DAY_UNITS) {
            n
        } else if unit_is(t, WEEK_UNITS) {
            n.or(dana.then_some(1.0)).map(|n| n * 7.0)
        } else if unit_is(t, MONTH_UNITS) {
            n.or(dana.then_some(1.0)).map(|n| n * 30.0)
        } else if unit_is(t, HOUR_UNITS) {
            n.filter(|h| *h >= 24.0).map(|h| h / 24.0)
        } else {
            None
        };
        if let Some(d) = days.filter(|d| *d > 0.0) {
            found = Some(d);
        }
    }
    found.map(|d| d.clamp(0.5, 30.0))
}

const PEOPLE: &[&str] = &["osob*", "ljud*", "clan*", "ukucan*", "covek*", "people", "persons", "person", "members"];
const ADULTS: &[&str] = &["odrasl*", "adult*"];
const CHILDREN: &[&str] = &["dece", "decu", "dete", "deteta", "child*", "kid*", "beb*", "baby", "babies"];
const HOUSEHOLD: &[&str] = &["porodic*", "family", "families", "domacinstv*", "household*"];
/// "četvoročlana porodica": a family of four.
const MEMBERS: &[(&str, f64)] = &[("dvoclan*", 2.0), ("troclan*", 3.0), ("cetvoroclan*", 4.0), ("petoclan*", 5.0), ("sestoclan*", 6.0)];

/// How many people: "4 osobe", "nas četvoro", "a family of four", "2
/// odrasla i 2 deteta" (adults and children add up).
fn people(toks: &[Tok]) -> Option<f64> {
    let (mut general, mut adults, mut children) = (None, 0.0, 0.0);
    for (i, t) in toks.iter().enumerate() {
        let n = i.checked_sub(1).and_then(|j| amount(&toks[j]));
        if unit_is(t, ADULTS) {
            adults += n.unwrap_or(0.0);
        } else if unit_is(t, CHILDREN) {
            children += n.unwrap_or(0.0);
        } else if unit_is(t, PEOPLE) {
            general = n.or(general);
        } else if let Some((_, m)) = MEMBERS.iter().find(|(k, _)| matches!(t, Tok::Word(w) if word_is(w, k))) {
            general = Some(*m);
        } else if let Some(m) = amount(t) {
            // "porodica od 4", "a family of four"
            let of = i >= 2 && matches!(&toks[i - 1], Tok::Word(w) if w == "od" || w == "of") && unit_is(&toks[i - 2], HOUSEHOLD);
            let group = matches!(t, Tok::Word(w) if COLLECTIVE.iter().any(|(k, _)| k == w));
            let counted = toks.get(i + 1).is_some_and(|x| unit_is(x, PEOPLE) || unit_is(x, ADULTS) || unit_is(x, CHILDREN));
            if of || (group && !counted) {
                general = Some(m);
            }
        }
    }
    let n = if adults + children > 0.0 { Some(adults + children) } else { general };
    n.filter(|p| (1.0..=100.0).contains(p))
}

/// A place the question names: the power calculator's region (the nearest
/// one in its table) and the latitude, for the sun and the water.
struct Place {
    region: &'static str,
    lat: f64,
}

const PLACES: &[(&str, &str, f64)] = &[
    ("beograd*", "belgrade", 44.8), ("belgrade", "belgrade", 44.8), ("novi sad", "novi-sad", 45.3), ("novom sadu", "novi-sad", 45.3),
    ("novog sada", "novi-sad", 45.3), ("nis", "nis", 43.3), ("nisa", "nis", 43.3), ("kragujev*", "kragujevac", 44.0), ("sarajev*", "sarajevo", 43.9),
    ("zagreb*", "zagreb", 45.8), ("bec", "vienna", 48.2), ("becu", "vienna", 48.2), ("vienna", "vienna", 48.2), ("berlin*", "berlin", 52.5),
    ("london*", "london", 51.5), ("atin*", "athens", 38.0), ("athens", "athens", 38.0),
    // Other towns in Serbia, with the nearest place of the table.
    ("subotic*", "novi-sad", 46.1), ("zrenjanin*", "novi-sad", 45.4), ("sombor*", "novi-sad", 45.8), ("pancev*", "belgrade", 44.9),
    ("smederev*", "belgrade", 44.7), ("sabac", "belgrade", 44.8), ("sapc*", "belgrade", 44.8), ("valjev*", "belgrade", 44.3),
    ("loznic*", "belgrade", 44.5), ("cacak", "kragujevac", 43.9), ("cack*", "kragujevac", 43.9), ("kraljevo", "kragujevac", 43.7),
    ("kraljevu", "kragujevac", 43.7), ("uzic*", "kragujevac", 43.9), ("novi pazar", "kragujevac", 43.1), ("leskov*", "nis", 43.0),
    ("vranj*", "nis", 42.6), ("pirot*", "nis", 43.2), ("zajecar*", "nis", 43.9),
];

fn place(toks: &[Tok]) -> Option<Place> {
    (0..toks.len()).find_map(|i| PLACES.iter().find(|(p, _, _)| phrase_at(toks, i, p).is_some()).map(|(_, region, lat)| Place { region, lat: *lat }))
}

// ---- The power calculator (ui/src/power.ts) ----------------------------------------

const POWER_TOOL: &str = "power";

/// Appliances as people name them, with the power calculator's ids
/// (APPLIANCES in ui/src/power.ts). The longer names come first: "mali
/// frižider" is the small one, "LED sijalice" the bulbs ("led" alone is ice).
const APPLIANCES: &[(&str, &str)] = &[
    ("mal* frizider*", "fridge-small"), ("mini frizider*", "fridge-small"), ("small fridge*", "fridge-small"), ("mini fridge*", "fridge-small"),
    ("compact fridge*", "fridge-small"), ("small refrigerator*", "fridge-small"), ("vertikaln* zamrzivac*", "freezer-upright"),
    ("upright freezer*", "freezer-upright"), ("frizider*", "fridge"), ("fridge*", "fridge"), ("refrigerator*", "fridge"),
    ("zamrzivac*", "freezer"), ("freezer*", "freezer"), ("deep freeze*", "freezer"), ("mikrotalas*", "microwave"),
    ("mikrovaln*", "microwave"), ("microwave*", "microwave"), ("led sijalic*", "lights-led"), ("led svetl*", "lights-led"),
    ("led lamp*", "lights-led"), ("led bulb*", "lights-led"), ("led light*", "lights-led"), ("sijalic*", "lights-led"),
    ("svetla", "lights-led"), ("lamp*", "lights-led"), ("bulb*", "lights-led"), ("light", "lights-led"), ("lights", "lights-led"),
    ("cpap* sa ovlazivac*", "cpap-humid"), ("cpap* with humidifier*", "cpap-humid"), ("cpap*", "cpap"), ("bipap*", "cpap"),
    ("aparat* za apnej*", "cpap"), ("mobiln* telefon*", "phone"), ("telefon*", "phone"), ("mobilni", "phone"), ("mobilnih", "phone"),
    ("mobitel*", "phone"), ("smartphone*", "phone"), ("cell phone*", "phone"), ("phone", "phone"), ("phones", "phone"),
    ("laptop*", "laptop"), ("ruter*", "router"), ("router*", "router"), ("modem*", "router"), ("wifi", "router"), ("wi fi", "router"),
    ("televizor*", "tv"), ("tv", "tv"), ("television*", "tv"), ("radio aparat*", "radio"), ("tranzistor*", "radio"), ("radija", "radio"),
    ("radiju", "radio"), ("ventilator*", "fan"), ("fan", "fan"), ("fans", "fan"), ("elektricn* cebe*", "blanket"), ("cebe*", "blanket"),
    ("electric blanket*", "blanket"), ("heated blanket*", "blanket"), ("hidrofor*", "pump"), ("pumpa", "pump"), ("pumpe", "pump"),
    ("pumpu", "pump"), ("well pump*", "pump"), ("water pump*", "pump"), ("pump", "pump"), ("pumps", "pump"),
];
/// Only in English: "radio" is also "worked" in Serbian.
const APPLIANCES_EN: &[(&str, &str)] = &[("radio", "radio"), ("radios", "radio")];

/// Appliances that switch on and off by themselves: counted by their energy
/// a day, so hours are not said for them.
const CYCLING: &[&str] = &["fridge", "fridge-small", "freezer", "freezer-upright"];

/// Words between a number and the appliance it counts: "6 LED bulbs",
/// "dva mobilna telefona", "a couple of lamps".
const MODIFIERS: &[&str] = &[
    "led", "mal*", "small", "mini", "big", "velik*", "vec*", "manj*", "old", "star*", "new", "nov*", "electric*", "elektricn*", "mobiln*",
    "smart", "pametn*", "ceiling", "plafonsk*", "desk", "ston*", "sobn*", "of", "extra", "jos", "more", "additional", "dodatn*", "kom",
    "komad*", "pcs", "x",
];

const LITHIUM: &[&str] = &["lifepo4", "lifepo", "lfp", "litijum*", "lithium"];
const LEAD: &[&str] = &["olovn*", "lead acid", "lead batter*", "agm", "gel batter*", "gel akumulator*"];
const MONTHS: &[(&str, u32)] = &[
    ("januar*", 1), ("februar*", 2), ("mart", 3), ("marta", 3), ("martu", 3), ("march", 3), ("april*", 4), ("maj", 5), ("maja", 5), ("maju", 5),
    ("in may", 5), ("jun", 6), ("juna", 6), ("junu", 6), ("june", 6), ("jul", 7), ("jula", 7), ("julu", 7), ("july", 7), ("avgust*", 8),
    ("august", 8), ("septemb*", 9), ("oktob*", 10), ("october", 10), ("novemb*", 11), ("decemb*", 12),
];

/// What a question says for the power calculator.
#[derive(Debug, Default, Clone, PartialEq)]
struct PowerAsk {
    /// Appliance id, how many, and hours a day when said.
    items: Vec<(&'static str, u32, Option<f64>)>,
    days: Option<f64>,
    battery: Option<&'static str>,
    volts: Option<u32>,
    region: Option<&'static str>,
    month: Option<u32>,
}

impl PowerAsk {
    /// What a later question says, over what an earlier one said.
    fn then(mut self, later: PowerAsk) -> PowerAsk {
        for item in later.items {
            match self.items.iter_mut().find(|i| i.0 == item.0) {
                Some(i) => *i = item,
                None => self.items.push(item),
            }
        }
        self.days = later.days.or(self.days);
        self.battery = later.battery.or(self.battery);
        self.volts = later.volts.or(self.volts);
        self.region = later.region.or(self.region);
        self.month = later.month.or(self.month);
        self
    }
}

/// The appliance named at `i`, and how many words its name takes.
fn appliance_at(toks: &[Tok], i: usize, en: bool) -> Option<(&'static str, usize)> {
    let extra: &[(&'static str, &'static str)] = if en { APPLIANCES_EN } else { &[] };
    APPLIANCES.iter().chain(extra).find_map(|(name, id)| phrase_at(toks, i, name).map(|n| (*id, n)))
}

/// The number said just before the appliance at `i` ("6 LED bulbs"), if any.
fn count_before(toks: &[Tok], i: usize) -> Option<f64> {
    let mut j = i;
    for _ in 0..3 {
        j = j.checked_sub(1)?;
        if let Some(n) = amount(&toks[j]) {
            return Some(n);
        }
        if !unit_is(&toks[j], MODIFIERS) {
            return None;
        }
    }
    None
}

/// Hours a day said right after an appliance ("4 sijalice po 5 sati"), and
/// where the number is. A number for anything else ends the look.
fn hours_after(toks: &[Tok], from: usize) -> Option<(f64, usize)> {
    for (j, t) in toks.iter().enumerate().skip(from).take(4) {
        if matches!(t, Tok::Sep | Tok::Dims(..)) {
            return None;
        }
        // "a" is an article here more often than a number.
        if matches!(t, Tok::Word(w) if w == "a") {
            continue;
        }
        if let Some(n) = amount(t) {
            let hours = toks.get(j + 1).is_some_and(|u| unit_is(u, HOUR_UNITS));
            return (hours && n > 0.0 && n <= 24.0).then_some((n, j));
        }
    }
    None
}

/// The appliances, the days and the rest of what the power calculator
/// takes, as far as the question says them.
fn power_ask(toks: &[Tok], en: bool) -> PowerAsk {
    let mut ask = PowerAsk::default();
    let mut used: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let Some((id, len)) = appliance_at(toks, i, en) else {
            i += 1;
            continue;
        };
        let n = count_before(toks, i).unwrap_or(1.0).round().clamp(1.0, 100.0) as u32;
        let hours = if CYCLING.contains(&id) { None } else { hours_after(toks, i + len) };
        if let Some((_, at)) = hours {
            used.extend([at, at + 1]);
        }
        let hours = hours.map(|(h, _)| h);
        match ask.items.iter_mut().find(|x| x.0 == id) {
            Some(x) => {
                x.1 = x.1.max(n);
                x.2 = hours.or(x.2);
            }
            None => ask.items.push((id, n, hours)),
        }
        i += len;
    }
    ask.days = duration(toks, &used);
    ask.battery = if has_any(toks, LITHIUM) {
        Some("lifepo4")
    } else if has_any(toks, LEAD) {
        Some("lead")
    } else {
        None
    };
    ask.volts = toks.windows(2).find_map(|w| match (&w[0], &w[1]) {
        (Tok::Num(v), u) if [12.0, 24.0, 48.0].contains(v) && unit_is(u, &["v", "volt*", "vdc"]) => Some(*v as u32),
        _ => None,
    });
    let place = place(toks);
    ask.region = place.map(|p| p.region);
    ask.month = (0..toks.len()).find_map(|i| MONTHS.iter().find(|(m, _)| phrase_at(toks, i, m).is_some()).map(|(_, n)| *n));
    ask
}

/// The power calculator's deep link (see the top of ui/src/power.ts).
fn power_link(ask: &PowerAsk) -> String {
    let mut q: Vec<String> = Vec::new();
    if !ask.items.is_empty() {
        let items: Vec<String> = ask
            .items
            .iter()
            .map(|(id, n, h)| match h {
                Some(h) => format!("{id}:{n}:{}", num(*h)),
                None => format!("{id}:{n}"),
            })
            .collect();
        q.push(format!("items={}", items.join(",")));
    }
    if let Some(d) = ask.days {
        q.push(format!("days={}", num(d)));
    }
    if let Some(b) = ask.battery {
        q.push(format!("battery={b}"));
    }
    if let Some(v) = ask.volts {
        q.push(format!("volts={v}"));
    }
    if let Some(r) = ask.region {
        q.push(format!("region={r}"));
    }
    if let Some(m) = ask.month {
        q.push(format!("month={m}"));
    }
    if q.is_empty() {
        format!("#{POWER_TOOL}")
    } else {
        format!("#{POWER_TOOL}?{}", q.join("&"))
    }
}

/// The power calculator: with the list when the question names appliances,
/// with the days when it says only those, and empty when it asks how much
/// without either.
fn power_suggestion(said: &[&str], sizing: bool) -> Option<Suggestion> {
    let ask = said.iter().map(|q| power_ask(&tokens(q), english(q))).fold(PowerAsk::default(), PowerAsk::then);
    let label = if !ask.items.is_empty() {
        "aiSugPowerList"
    } else if ask.days.is_some() || sizing {
        "powerCalc"
    } else {
        return None;
    };
    Some(tool(POWER_TOOL, "power", power_link(&ask), label))
}

// ---- The water calculator ------------------------------------------------------------
//
// Built against the deep links of the water calculator (made alongside this):
//   #tools/water?people=4&days=7                                    water to store
//   #tools/water?part=drip&beds=3x1.2:tomatoes,2x1:greens&lat=44.8  drip irrigation
// Its tool id, its parameters and its crop groups are all in this section, so
// a change of names on its side is a change here only. Assumed here: each bed
// is length x width in metres, two beds alike are listed twice, a bed without
// a crop has none after it, and an area without sizes ("10 m2") is one bed
// that long and 1 m wide.

const WATER_TOOL: &str = "water";
const WATER_LINK: &str = "#tools/water";
/// The app's address of the tool without anything filled in.
const WATER_PLAIN: &str = "#water";

/// Words about watering a garden: the drip part of the calculator.
const DRIP: &[&str] = &["kap po kap", "kapaljk*", "navodnj*", "zaliv*", "drip", "irrigat*", "watering", "soaker*"];

/// Crops as people name them, and the calculator's crop groups.
const CROPS: &[(&str, &str)] = &[
    ("paradajz*", "tomatoes"), ("tomato*", "tomatoes"), ("paprik*", "tomatoes"), ("pepper", "tomatoes"), ("peppers", "tomatoes"),
    ("patlidzan*", "tomatoes"), ("eggplant*", "tomatoes"), ("krastav*", "squash"), ("cucumber*", "squash"), ("tikvic*", "squash"),
    ("zucchini", "squash"), ("bundev*", "squash"), ("squash", "squash"), ("lubenic*", "squash"), ("dinj*", "squash"),
    ("melon*", "squash"), ("salat*", "greens"), ("lettuce", "greens"), ("spanac*", "greens"), ("spinach", "greens"), ("blitv*", "greens"),
    ("chard", "greens"), ("rukol*", "greens"), ("kupus*", "greens"), ("cabbage*", "greens"), ("kelj*", "greens"), ("kale", "greens"),
    ("greens", "greens"), ("zacinsk*", "greens"), ("herbs", "greens"), ("pasulj*", "beans"), ("boranij*", "beans"), ("grasak", "beans"),
    ("graska", "beans"), ("bean*", "beans"), ("peas", "beans"), ("sargarep*", "roots"), ("carrot*", "roots"), ("cvekl*", "roots"),
    ("beet", "roots"), ("beets", "roots"), ("beetroot*", "roots"), ("rotkv*", "roots"), ("radish*", "roots"), ("persun*", "roots"),
    ("parsnip*", "roots"), ("krompir*", "potatoes"), ("potato*", "potatoes"), ("luk", "onions"), ("luka", "onions"), ("luku", "onions"),
    ("praziluk*", "onions"), ("onion*", "onions"), ("garlic", "onions"), ("leek*", "onions"), ("kukuruz*", "corn"), ("corn", "corn"),
    ("jagod*", "strawberries"), ("strawberr*", "strawberries"), ("malin*", "bushes"), ("kupin*", "bushes"), ("ribizl*", "bushes"),
    ("borovnic*", "bushes"), ("aronij*", "bushes"), ("raspberr*", "bushes"), ("blackberr*", "bushes"), ("blueberr*", "bushes"),
    ("currant*", "bushes"), ("bush*", "bushes"),
];

/// Words between a number and the size it counts: "2 leje 3x1.2", "two 3x1.2 m beds".
const BED_WORDS: &[&str] = &["lej*", "gredic*", "bed", "beds", "of", "od", "po", "x"];
/// Units of an area: "10 m2", "10 kvadrata", "10 square meters".
const AREA_UNITS: &[&str] = &["m2", "kvadrat*", "kvm", "sqm", "square meter*", "square metre*", "sq m"];
/// At most this many beds in a link.
const MAX_BEDS: usize = 20;

#[derive(Debug, Clone, PartialEq)]
struct Bed {
    len: f64,
    width: f64,
    crop: Option<&'static str>,
}

fn crop_of(t: &Tok) -> Option<&'static str> {
    match t {
        Tok::Word(w) => CROPS.iter().find(|(k, _)| word_is(w, k)).map(|(_, c)| *c),
        _ => None,
    }
}

/// The crop said with the size at `i`: after it ("3x1.2 m paradajz"), else
/// before it ("paradajz na 3x1.2"), within what is said about it.
fn crop_near(toks: &[Tok], i: usize) -> Option<&'static str> {
    for t in toks.iter().skip(i + 1).take(5) {
        if matches!(t, Tok::Sep | Tok::Dims(..)) {
            break;
        }
        if let Some(c) = crop_of(t) {
            return Some(c);
        }
    }
    for t in toks[..i].iter().rev().take(5) {
        if matches!(t, Tok::Sep | Tok::Dims(..)) {
            break;
        }
        if let Some(c) = crop_of(t) {
            return Some(c);
        }
    }
    None
}

/// How many beds of the size at `i`: "2 leje 3x1.2", "two 3x1.2 m beds".
fn bed_count(toks: &[Tok], i: usize) -> usize {
    let mut j = i;
    for _ in 0..3 {
        let Some(k) = j.checked_sub(1) else { break };
        j = k;
        let t = &toks[j];
        if let Some(n) = amount(t).filter(|_| !matches!(t, Tok::Word(w) if w == "a")) {
            return n.round().clamp(1.0, MAX_BEDS as f64) as usize;
        }
        if !unit_is(t, BED_WORDS) {
            break;
        }
    }
    1
}

/// The beds a question describes, each with its crop: sizes like "3x1.2",
/// or an area ("10 m2") when no size is said.
fn beds(toks: &[Tok]) -> Vec<Bed> {
    let mut out: Vec<Bed> = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        if let Tok::Dims(len, width) = *t {
            if !(0.1..=500.0).contains(&len) || !(0.1..=500.0).contains(&width) {
                continue;
            }
            let crop = crop_near(toks, i);
            for _ in 0..bed_count(toks, i) {
                if out.len() < MAX_BEDS {
                    out.push(Bed { len, width, crop });
                }
            }
        }
    }
    if out.is_empty() {
        for (i, t) in toks.iter().enumerate() {
            if let Tok::Num(area) = *t {
                if (0.5..=10_000.0).contains(&area) && AREA_UNITS.iter().any(|u| phrase_at(toks, i + 1, u).is_some()) && out.len() < MAX_BEDS {
                    out.push(Bed { len: area, width: 1.0, crop: crop_near(toks, i) });
                }
            }
        }
    }
    // One crop for the whole garden: it is every bed's that has none of its own.
    let mut crops: Vec<&str> = toks.iter().filter_map(crop_of).collect();
    crops.sort_unstable();
    crops.dedup();
    if crops.len() == 1 {
        for b in out.iter_mut().filter(|b| b.crop.is_none()) {
            b.crop = Some(crops[0]);
        }
    }
    out
}

fn drip_link(beds: &[Bed], lat: Option<f64>) -> String {
    let mut q = vec!["part=drip".to_string()];
    if !beds.is_empty() {
        let list: Vec<String> = beds
            .iter()
            .map(|b| match b.crop {
                Some(c) => format!("{}x{}:{c}", num(b.len), num(b.width)),
                None => format!("{}x{}", num(b.len), num(b.width)),
            })
            .collect();
        q.push(format!("beds={}", list.join(",")));
    }
    if let Some(lat) = lat {
        q.push(format!("lat={}", num(lat)));
    }
    format!("{WATER_LINK}?{}", q.join("&"))
}

fn storage_link(people: Option<f64>, days: Option<f64>) -> String {
    let mut q: Vec<String> = Vec::new();
    if let Some(p) = people {
        q.push(format!("people={}", num(p)));
    }
    if let Some(d) = days {
        q.push(format!("days={}", num(d)));
    }
    format!("{WATER_LINK}?{}", q.join("&"))
}

/// The water calculator: its drip part for watering a garden (with the
/// beds when the question describes them), or the water to store (with the
/// people and days when it says them, or empty when it asks how much).
fn water_suggestion(topic: &str, all: &[Vec<Tok>], sizing: bool) -> Option<Suggestion> {
    if all.iter().any(|t| has_any(t, DRIP)) {
        let beds = all.iter().rev().map(|t| beds(t)).find(|b| !b.is_empty()).unwrap_or_default();
        let lat = all.iter().rev().find_map(|t| place(t)).map(|p| p.lat);
        let label = if beds.is_empty() { "aiSugWater" } else { "aiSugDripList" };
        return Some(tool(WATER_TOOL, "water", drip_link(&beds, lat), label));
    }
    if topic != "water" {
        return None;
    }
    let people = all.iter().rev().find_map(|t| people(t));
    let days = all.iter().rev().find_map(|t| duration(t, &[]));
    if people.is_some() || days.is_some() {
        Some(tool(WATER_TOOL, "water", storage_link(people, days), "aiSugWaterList"))
    } else if sizing {
        Some(tool(WATER_TOOL, "water", WATER_PLAIN.into(), "aiSugWater"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn links(question: &str, history: &[&str]) -> Vec<String> {
        let history: Vec<Turn> = history.iter().map(|q| Turn { question: q.to_string(), answer: "…".into() }).collect();
        read_question(question, &history).1.into_iter().map(|s| s.link).collect()
    }

    fn power(question: &str) -> String {
        power_link(&power_ask(&tokens(question), english(question)))
    }

    #[test]
    fn appliances_and_days_become_the_power_calculators_list() {
        assert_eq!(power("a fridge, 6 LED bulbs and a laptop for 3 days"), "#power?items=fridge:1,lights-led:6,laptop:1&days=3");
        assert_eq!(power("koliko baterija mi treba za frižider 3 dana"), "#power?items=fridge:1&days=3");
        assert_eq!(power("Колико батерија ми треба за фрижидер 3 дана?"), "#power?items=fridge:1&days=3");
        assert_eq!(power("treba mi frizider, 4 sijalice, ruter i CPAP aparat za 2 dana"), "#power?items=fridge:1,lights-led:4,router:1,cpap:1&days=2");
        assert_eq!(power("dva mobilna telefona, mali frižider i TV, nedelju dana"), "#power?items=phone:2,fridge-small:1,tv:1&days=7");
        // Hours a day go with their appliance; a refrigerator has none.
        assert_eq!(power("4 sijalice po 5 sati i laptop 3 sata dnevno, frižider, 2 nedelje"), "#power?items=lights-led:4:5,laptop:1:3,fridge:1&days=14");
        assert_eq!(power("a CPAP with humidifier 8 hours a night for three nights"), "#power?items=cpap-humid:1:8&days=3");
        // The battery, its voltage, the place and the month.
        assert_eq!(
            power("LiFePO4 baterija 24V za zamrzivač i dva telefona u Beogradu u decembru, 72 sata"),
            "#power?items=freezer:1,phone:2&days=3&battery=lifepo4&volts=24&region=belgrade&month=12"
        );
        assert_eq!(power("olovni akumulator 12 V za ventilator, Niš"), "#power?items=fan:1&battery=lead&volts=12&region=nis");
        // "led" is ice, "radio" also "worked" in Serbian; "a" is not a number of hours.
        assert_eq!(power("stavljam led na opekotinu, radio sam ceo dan"), "#power");
        assert_eq!(power("a radio and a laptop for a week"), "#power?items=radio:1,laptop:1&days=7");
    }

    #[test]
    fn a_power_question_gets_the_calculator_filled_in() {
        let (topics, s) = read_question("a fridge, 6 LED bulbs and a laptop for 3 days", &[]);
        assert_eq!(topics, vec!["power"], "the appliances are enough");
        assert_eq!(s[0], Suggestion { kind: "tool", id: "power", topic: "power", link: "#power?items=fridge:1,lights-led:6,laptop:1&days=3".into(), label_key: "aiSugPowerList" });
        assert_eq!(s[1], Suggestion { kind: "guides", id: "power", topic: "power", link: "#addons/power".into(), label_key: "aiSugGuides" });
        assert_eq!(s.len(), 2);
        assert_eq!(links("koliko baterija mi treba za frižider 3 dana", &[]), vec!["#power?items=fridge:1&days=3", "#addons/power"]);
        // Asking how much without a list: the calculator as it is.
        assert_eq!(links("How big a solar panel do I need?", &[]), vec!["#power", "#addons/power"]);
        assert_eq!(links("koliko baterija za 3 dana", &[]), vec!["#power?days=3", "#addons/power"]);
        // About power, but nothing to size: the guides only.
        assert_eq!(links("kako radi invertor", &[]), vec!["#addons/power"]);
        // A fridge in a food question is not a power question.
        assert_eq!(read_question("Can I keep milk in the fridge for 3 days?", &[]).0, vec!["food"]);
    }

    #[test]
    fn a_follow_up_takes_the_list_from_the_question_before() {
        assert_eq!(links("A za 5 dana?", &["Koliko baterija za frižider i ruter?"]), vec!["#power?items=fridge:1,router:1&days=5", "#addons/power"]);
        assert_eq!(links("and with two fridges?", &["a fridge and a laptop for 3 days"]), vec!["#power?items=fridge:2,laptop:1&days=3", "#addons/power"]);
        // A new question starts afresh; small talk gets nothing.
        assert_eq!(links("kako se čuva brašno?", &["Koliko baterija za frižider i ruter?"]), vec!["#supplies", "#addons/food"]);
        assert!(links("Hvala!", &["Koliko baterija za frižider i ruter?"]).is_empty());
        assert!(links("Ko je napisao Na Drini ćuprija?", &["Koliko baterija za frižider i ruter?"]).is_empty(), "no topic and not a follow-up");
    }

    #[test]
    fn water_questions_get_the_water_calculator() {
        assert_eq!(links("Koliko vode treba za 4 osobe za 7 dana?", &[]), vec!["#tools/water?people=4&days=7", "#addons/water"]);
        assert_eq!(links("How much water should a family of four store for two weeks?", &[]), vec!["#tools/water?people=4&days=14", "#addons/water"]);
        assert_eq!(links("koliko vode da čuvamo nas petoro", &[]), vec!["#tools/water?people=5", "#addons/water"]);
        assert_eq!(links("koliko pijaće vode za 2 odrasla i 2 deteta, 3 dana", &[]), vec!["#tools/water?people=4&days=3", "#addons/water"]);
        assert_eq!(links("How much water does one person need a day?", &[]), vec!["#tools/water?people=1&days=1", "#addons/water"]);
        assert_eq!(links("How much water should we store?", &[]), vec!["#water", "#addons/water"], "how much, without numbers");
        assert_eq!(links("kako da prečistim vodu za piće", &[]), vec!["#addons/water"], "nothing to size");
        // Drip irrigation: the beds, their crops and the place.
        assert_eq!(
            links("kap po kap za 2 leje 3x1,2 m paradajz i jednu 2x1 salata, Novi Sad", &[]),
            vec!["#tools/water?part=drip&beds=3x1.2:tomatoes,3x1.2:tomatoes,2x1:greens&lat=45.3", "#addons/garden", "#addons/water"]
        );
        assert_eq!(links("kap po kap za 10 m2 paradajza", &[]), vec!["#tools/water?part=drip&beds=10x1:tomatoes", "#addons/garden", "#addons/water"]);
        assert_eq!(links("drip irrigation for two 3x1 m beds of strawberries", &[]), vec!["#tools/water?part=drip&beds=3x1:strawberries,3x1:strawberries", "#addons/garden", "#addons/water"]);
        assert_eq!(links("kako da postavim navodnjavanje kap po kap", &[]), vec!["#tools/water?part=drip", "#addons/water", "#addons/garden"]);
        // A garden question without watering: the guides.
        assert_eq!(links("kada se sadi krompir", &[]), vec!["#addons/garden"]);
    }

    #[test]
    fn other_topics_get_their_tools_and_guides() {
        let got = read_question("šta da radim kod opekotine", &[]).1;
        assert_eq!(got, vec![Suggestion { kind: "guides", id: "health", topic: "health", link: "#addons/health".into(), label_key: "aiSugGuides" }]);
        assert_eq!(links("kako da stignem do najbliže bolnice", &[]), vec!["#maps", "#addons/maps", "#addons/health"]);
        assert_eq!(read_question("Where is a map of the hiking trails?", &[]).1[1].label_key, "topicMapsGuides");
        assert_eq!(links("kad je poceo prvi srpski ustanak", &[]), vec!["#library", "#addons/knowledge"]);
        assert_eq!(links("Kako da napravim zalihe hrane za zimu?", &[]), vec!["#supplies", "#addons/food"]);
        assert_eq!(links("recept za hleb bez kvasca", &[]), vec!["#addons/food"]);
        assert_eq!(links("curi mi slavina u kupatilu", &[]), vec!["#addons/build"]);
        // Never more than three.
        let many = read_question("koliko vode i struje treba za baštu, frižider i pumpu u Nišu, gde je najbliža mapa", &[]).1;
        assert!(many.len() <= MAX_SUGGESTIONS && many.iter().filter(|s| s.kind == "tool").count() <= MAX_TOOLS, "{many:?}");
    }

    #[test]
    fn small_talk_and_the_supplies_get_nothing() {
        for q in [
            "Hvala!",
            "zdravo",
            "OK, thanks",
            "Ko si ti?",
            "Šta imamo u zalihama?",
            "Koliko imamo baterija?",
            "What's on the shopping list?",
            "Zapamti da je Ana alergična na penicilin",
            "Remember that the water tank holds 200 liters.",
        ] {
            let (topics, s) = read_question(q, &[]);
            assert!(s.is_empty() && topics.is_empty(), "{q}: {topics:?} {s:?}");
        }
    }

    #[test]
    fn calculators_are_named_for_the_model() {
        let s = read_question("a fridge, 6 LED bulbs and a laptop for 3 days", &[]).1;
        assert_eq!(calculators(&s), vec!["power"]);
        assert_eq!(calculators(&read_question("Koliko vode treba za 4 osobe za 7 dana?", &[]).1), vec!["water"]);
        assert!(calculators(&read_question("šta da radim kod opekotine", &[]).1).is_empty());
    }

    #[test]
    fn the_appliance_ids_are_the_power_calculators() {
        // The ids of APPLIANCES in ui/src/power.ts.
        let known = [
            "fridge", "fridge-small", "freezer", "freezer-upright", "microwave", "lights-led", "phone", "laptop", "router", "tv", "radio", "cpap",
            "cpap-humid", "fan", "blanket", "pump",
        ];
        for (_, id) in APPLIANCES.iter().chain(APPLIANCES_EN) {
            assert!(known.contains(id), "{id}");
        }
        let power_ts = include_str!("../../../../ui/src/power.ts");
        for id in known {
            assert!(power_ts.contains(&format!("{{ id: \"{id}\",")), "{id} is not in power.ts");
        }
    }
}
