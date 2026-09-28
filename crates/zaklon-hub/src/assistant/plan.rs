//! What a question is about: the plan the model makes (a library question
//! with its search terms, the supplies, a change to them, or something to
//! remember), and the checks by the question's own words that correct the
//! plan or stand in for it.

use std::sync::atomic::Ordering;
use std::time::Duration;

use serde::Deserialize;
use tracing::warn;
use zaklon_core::supplies::Item;

use super::engine::{SLOT_PLAN, SLOT_PLAN_EN};
use super::supplies::{category_of_word, match_items, ItemMatch};
use super::text::{plain, search_words, stem};
use super::Assistant;

/// How long the plan may take. With its instructions cached (after the first
/// plan on a running engine) it takes seconds; before that the engine reads
/// about a thousand tokens of instructions, which takes a slow computer up to
/// two minutes. Past this, the question is routed by its own words, and the
/// engine keeps what it has read for the next plan.
const PLAN_TIME: Duration = Duration::from_secs(45);
const PLAN_TIME_COLD: Duration = Duration::from_secs(120);

impl Assistant {
    /// What the question is about, decided by the model in a fixed JSON
    /// shape (the engine enforces the schema): a library question with its
    /// search terms in basic form ("konzerva, pasulj, rok trajanja"), a
    /// question about the supplies, or a change to them. None when the engine
    /// gave no reply in time (`PLAN_TIME`).
    pub(super) async fn plan(&self, port: u16, question: &str, language: &str) -> Option<Plan> {
        let sr = language == "sr";
        let prompt = if sr {
            "Odluči o čemu je poruka i odgovori samo JSON-om.\n\
kind: \"library\" za opšta pitanja (zdravlje, hrana, popravke, priroda...), \"supplies_question\" za pitanja o zalihama u kući \
(šta imam, koliko imam, šta ističe, šta treba kupiti), \"supplies_change\" kad treba dodati, potrošiti ili staviti na listu za kupovinu.\n\
\"remember\" kad treba nešto zapamtiti (note je ta činjenica kao rečenica o domaćinstvu).\n\
safety: true ako je pitanje o zdravlju, bolesti, leku ili dozi, povredi, trovanju, ujedu ili ubodu, ili prvoj pomoći; inače false.\n\
terms: 2 do 4 pojma za pretragu, latinicom. Prvi pojam je glavna tema pitanja, izraz od jedne do tri reči \
(npr. „zamena osigurača“, „pijaća voda“, „ujed zmije“). Ne piši same opšte reči („lečenje“, „simptomi“, „zamena“, „prva pomoć“, \
„cena“, „rok trajanja“); spoji ih sa temom („lečenje opekotina“). Ljudi često kucaju bez kvačica: vrati ih (c → č ili ć, s → š, \
z → ž, dj → đ), npr. „sargarepa“ → „šargarepa“, „cvece“ → „cveće“, i biraj reč koja ima smisla uz ostatak pitanja.\n\
terms_en: isti pojmovi na engleskom, za knjige na engleskom (npr. [\"snakebite\", \"first aid\"]).\n\
change (samo za supplies_change): action je \"add\" (dodaj u zalihe), \"use\" (potrošeno) ili \"shopping\" (na listu za kupovinu); \
name je naziv stvari u osnovnom obliku; quantity je broj (0 ako nije rečeno); unit je pcs, kg, g, l, ml ili pack; \
category je food, drink, medicine, hygiene, equipment, fuel ili other."
        } else {
            "Decide what the message is about and answer only with JSON.\n\
kind: \"library\" for general questions (health, food, repairs, nature...), \"supplies_question\" for questions about the household's supplies \
(what do I have, how much, what expires, what to buy), \"supplies_change\" to add, use up or put something on the shopping list.\n\
\"remember\" when something should be remembered (note is that fact as a sentence about the household).\n\
safety: true for health, illness, medicine or dose, injury, poisoning, bites or stings, or first aid; otherwise false.\n\
terms: 2 to 4 search terms. The first is the main topic of the question, a phrase of one to three words \
(e.g. \"fuse replacement\", \"drinking water\", \"snakebite\"). Do not write generic words alone (\"treatment\", \"symptoms\", \
\"first aid\", \"price\", \"shelf life\"); join them with the topic (\"burn treatment\").\n\
change (only for supplies_change): action is \"add\", \"use\" or \"shopping\"; name is the thing in its basic form; \
quantity is a number (0 if not said); unit is pcs, kg, g, l, ml or pack; category is food, drink, medicine, hygiene, equipment, fuel or other."
        };
        // Keys in the order the engine's grammar writes them (required ones
        // alphabetically, then the optional ones), so examples and output agree.
        let examples: &[(&str, &str)] = if sr {
            &[
                ("Koliko dugo traje hleb?", r#"{"kind":"library","safety":false,"terms":["čuvanje hleba","hleb"],"terms_en":["bread storage"]}"#),
                (
                    "Kako da izlečim prehladu kod deteta?",
                    r#"{"kind":"library","safety":true,"terms":["prehlada kod dece","lečenje prehlade"],"terms_en":["common cold in children"]}"#,
                ),
                ("kako se cisti bunar", r#"{"kind":"library","safety":false,"terms":["čišćenje bunara","bunar"],"terms_en":["well disinfection"]}"#),
                ("Koliko imam brašna?", r#"{"kind":"supplies_question","safety":false,"terms":["brašno"],"terms_en":[]}"#),
                ("Šta imam u zalihama?", r#"{"kind":"supplies_question","safety":false,"terms":[],"terms_en":[]}"#),
                ("Šta treba da kupim?", r#"{"kind":"supplies_question","safety":false,"terms":[],"terms_en":[]}"#),
                (
                    "Dodaj 2 litra mleka",
                    r#"{"kind":"supplies_change","safety":false,"terms":["mleko"],"terms_en":[],"change":{"action":"add","category":"drink","name":"mleko","quantity":2,"unit":"l"}}"#,
                ),
                (
                    "Potrošili smo 3 konzerve pasulja",
                    r#"{"kind":"supplies_change","safety":false,"terms":["pasulj"],"terms_en":[],"change":{"action":"use","category":"food","name":"pasulj","quantity":3,"unit":"pcs"}}"#,
                ),
                ("Zapamti da je Marko alergičan na orahe", r#"{"kind":"remember","safety":false,"terms":[],"terms_en":[],"note":"Marko je alergičan na orahe."}"#),
            ]
        } else {
            &[
                ("How long does bread last?", r#"{"kind":"library","safety":false,"terms":["bread storage","bread"]}"#),
                ("How do I treat a cold in a child?", r#"{"kind":"library","safety":true,"terms":["common cold in children","cold treatment"]}"#),
                ("how do i clean a well", r#"{"kind":"library","safety":false,"terms":["well disinfection","well"]}"#),
                ("How much flour do we have?", r#"{"kind":"supplies_question","safety":false,"terms":["flour"]}"#),
                ("What do we have in the supplies?", r#"{"kind":"supplies_question","safety":false,"terms":[]}"#),
                ("What do we need to buy?", r#"{"kind":"supplies_question","safety":false,"terms":[]}"#),
                (
                    "Add 2 liters of milk",
                    r#"{"kind":"supplies_change","safety":false,"terms":["milk"],"change":{"action":"add","category":"drink","name":"milk","quantity":2,"unit":"l"}}"#,
                ),
                (
                    "We used 3 cans of beans",
                    r#"{"kind":"supplies_change","safety":false,"terms":["beans"],"change":{"action":"use","category":"food","name":"beans","quantity":3,"unit":"pcs"}}"#,
                ),
                ("Remember that Mark is allergic to walnuts", r#"{"kind":"remember","safety":false,"terms":[],"note":"Mark is allergic to walnuts."}"#),
            ]
        };
        let mut messages = vec![serde_json::json!({ "role": "system", "content": prompt })];
        for (q, a) in examples {
            messages.push(serde_json::json!({ "role": "user", "content": q }));
            messages.push(serde_json::json!({ "role": "assistant", "content": a }));
        }
        messages.push(serde_json::json!({ "role": "user", "content": question }));
        let mut schema = serde_json::json!({
            "type": "object",
            "properties": {
                "kind": { "type": "string", "enum": ["library", "supplies_question", "supplies_change", "remember"] },
                "note": { "type": "string" },
                "safety": { "type": "boolean" },
                "terms": { "type": "array", "items": { "type": "string" }, "maxItems": 4 },
                "change": {
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["add", "use", "shopping"] },
                        "name": { "type": "string" },
                        "quantity": { "type": "number" },
                        "unit": { "type": "string", "enum": ["pcs", "kg", "g", "l", "ml", "pack"] },
                        "category": { "type": "string", "enum": ["food", "drink", "medicine", "hygiene", "equipment", "fuel", "other"] }
                    },
                    "required": ["action", "name", "quantity", "unit", "category"]
                }
            },
            "required": ["kind", "safety", "terms"]
        });
        if sr {
            // Serbian questions also get English terms, for the English books.
            schema["properties"]["terms_en"] = serde_json::json!({ "type": "array", "items": { "type": "string" }, "maxItems": 3 });
            schema["required"] = serde_json::json!(["kind", "safety", "terms", "terms_en"]);
        }
        let body = serde_json::json!({
            "messages": messages,
            // Room for a long note; the grammar ends the output at the closing brace anyway.
            "max_tokens": 256,
            "id_slot": if sr { SLOT_PLAN } else { SLOT_PLAN_EN },
            "cache_prompt": true,
            "temperature": 0.1,
            "chat_template_kwargs": { "enable_thinking": false },
            "response_format": { "type": "json_schema", "json_schema": { "name": "plan", "schema": schema } },
        });
        let warm = self.plan_warm.load(Ordering::Relaxed);
        let reply = self
            .http
            .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
            .timeout(if warm { PLAN_TIME } else { PLAN_TIME_COLD })
            .json(&body)
            .send()
            .await
            .ok()?;
        if !reply.status().is_success() {
            warn!(status = %reply.status(), "assistant: the AI engine refused the plan");
            return None;
        }
        let v = reply.json::<serde_json::Value>().await.ok()?;
        // The first plan on a new engine reads all of its instructions: a
        // good measure of how fast this computer reads.
        self.note_read_speed(&v["timings"]);
        let text = v["choices"][0]["message"]["content"].as_str()?;
        self.plan_warm.store(true, Ordering::Relaxed);
        Some(parse_plan(text))
    }
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct PlannedChange {
    pub action: String,
    pub name: String,
    #[serde(default)]
    pub quantity: f64,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub category: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct Plan {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub terms: Vec<String>,
    /// The same terms in English, for the English books.
    #[serde(default)]
    pub terms_en: Vec<String>,
    /// A health or first-aid question; None when the model did not say.
    #[serde(default)]
    pub safety: Option<bool>,
    #[serde(default)]
    pub change: Option<PlannedChange>,
    /// For "remember": the fact, as a sentence about the household.
    #[serde(default)]
    pub note: String,
}

/// Search terms as the model wrote them: Latin, lower case, no repeats.
fn clean_terms(terms: &[String], max: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for x in terms.iter().map(|x| zaklon_core::translit::cyrillic_to_latin(x.trim()).to_lowercase()) {
        if !x.is_empty() && x.chars().count() <= 40 && !out.contains(&x) {
            out.push(x);
        }
    }
    out.truncate(max);
    out
}

/// The model's JSON; anything unusable becomes a library question with the
/// words it wrote as search terms.
pub fn parse_plan(text: &str) -> Plan {
    let t = text.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
    match serde_json::from_str::<Plan>(t) {
        Ok(mut p) => {
            p.terms = clean_terms(&p.terms, 4);
            p.terms_en = clean_terms(&p.terms_en, 3);
            if !["library", "supplies_question", "supplies_change", "remember"].contains(&p.kind.as_str()) {
                p.kind = "library".into();
            }
            if let Some(c) = p.change.as_mut() {
                c.name = zaklon_core::translit::cyrillic_to_latin(c.name.trim()).chars().take(120).collect();
                if !c.quantity.is_finite() || c.quantity < 0.0 {
                    c.quantity = 0.0;
                }
                if !zaklon_core::supplies::UNITS.contains(&c.unit.as_str()) {
                    c.unit = "pcs".into();
                }
                if !zaklon_core::supplies::CATEGORIES.contains(&c.category.as_str()) {
                    c.category = "other".into();
                }
                if !["add", "use", "shopping"].contains(&c.action.as_str()) || c.name.is_empty() {
                    p.change = None;
                }
            }
            if p.kind == "supplies_change" && p.change.is_none() {
                p.kind = "supplies_question".into();
            }
            p
        }
        Err(_) => Plan { kind: "library".into(), terms: parse_keywords(text), ..Plan::default() },
    }
}

/// Corrections to what the model decided, for the cases it gets wrong.
pub fn route(plan: &mut Plan, question: &str, items: &[Item]) {
    // A "library" question that is plainly about the household's own supplies.
    if plan.kind == "library" && supplies_override(question, &plan.terms, items) {
        plan.kind = "supplies_question".into();
    }
    // A question about the supplies taken for a change: "Šta nam ponestaje?"
    // must not put an item called "Ponestaje" on the shopping list.
    if plan.kind == "supplies_change" {
        let asks = is_question(question) && mentions_supplies(question);
        let odd_name = plan.change.as_ref().is_some_and(|c| supply_word(&c.name));
        if asks || odd_name {
            plan.kind = "supplies_question".into();
            plan.change = None;
        }
    }
    if let Some(note) = remember_request(question) {
        if plan.kind != "remember" || plan.note.trim().is_empty() {
            plan.kind = "remember".into();
            plan.note = note;
        }
    }
}

/// A question plainly about the household's supplies: asked as a question
/// ("Koliko imamo brašna?", not "Stavi hleb na listu za kupovinu") with a
/// clear cue (`SUPPLY_MARKS`), and not a request to remember something.
/// `route` turns any plan for it into a supplies question (all but a
/// mistaken "remember"), so the model need not be asked.
pub fn plain_supplies_question(question: &str) -> bool {
    is_question(question) && mentions_supplies(question) && remember_request(question).is_none()
}

/// The plan for a plain supplies question, from its own words.
pub(super) fn supplies_plan(question: &str) -> Plan {
    Plan { kind: "supplies_question".into(), terms: search_words(question).iter().map(|w| stem(w)).collect(), ..Plan::default() }
}

/// "Zapamti da je Ana alergična na penicilin" -> "Ana je alergična na penicilin."
pub fn remember_request(question: &str) -> Option<String> {
    let q = question.trim();
    let lower = q.to_lowercase();
    const STARTS: &[&str] = &["zapamti da ", "zapamti: ", "zapamti ", "upamti da ", "upamti ", "remember that ", "remember: ", "remember "];
    for s in STARTS {
        if lower.starts_with(s) {
            // The prefixes are plain ASCII, so the same number of bytes of the
            // original text is the prefix too; `get` stays safe if not.
            let rest = q.get(s.len()..)?;
            let rest = rest.trim().trim_end_matches(['.', '!']);
            if rest.chars().count() < 3 {
                return None;
            }
            // "je Ana alergična" reads better as "Ana je alergična".
            let words: Vec<&str> = rest.split_whitespace().collect();
            let rest = if words.len() >= 3 && ["je", "su", "ima", "imaju", "nije", "nisu"].contains(&words[0]) {
                let mut w = words.clone();
                w.swap(0, 1);
                w.join(" ")
            } else {
                rest.to_string()
            };
            let mut c = rest.chars();
            let first = c.next()?;
            return Some(format!("{}{}.", first.to_uppercase(), c.as_str()));
        }
    }
    None
}

/// The question as lower-case plain words, padded with spaces, so a cue like
/// " imam " matches whole words only.
fn padded(question: &str) -> String {
    let words: String = plain(question).chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
    format!(" {} ", words.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn has_mark(question: &str, marks: &[&str]) -> bool {
    let q = padded(question);
    marks.iter().any(|m| q.contains(&format!(" {m} ")))
}

/// Cues that a question is about the household's own supplies ("Šta imamo u
/// zalihama?", "What do we have?"). A supply word alone is not enough:
/// "Kako da napravim zalihe hrane?" is a how-to question for the library.
const SUPPLY_MARKS: &[&str] = &[
    // Serbian: what we have.
    "sta imamo", "koliko imamo", "da li imamo", "imamo li", "jel imamo", "je l imamo", "imamo u kuci", "u kuci imam", "u kuci imamo",
    "imam u kuci", "imamo kod kuce",
    // Serbian: our supplies and lists.
    "u zalihama", "nase zalihe", "nasih zaliha", "nasim zalihama", "moje zalihe", "mojih zaliha", "mojim zalihama", "zalihe u kuci",
    "na listi", "lista za kupovinu", "listu za kupovinu", "listi za kupovinu", "sta mi istice", "sta nam istice", "sta mi isticu",
    "sta nam isticu", "sta je isteklo", "sta nam je isteklo", "sta mi je isteklo", "istice mi", "istice nam", "ponestaje",
    // English.
    "do we have", "have we got", "how much do we", "how many do we", "our supplies", "my supplies", "our pantry", "my pantry",
    "in the pantry", "in stock", "shopping list", "what expires", "what s expiring", "whats expiring", "what is expiring", "expiring soon",
    "running low", "are we out of", "we re out of",
];

/// "Da li imam…", "Do I have…": about the supplies only when a stored item
/// is named. "Da li imam upalu grla?" is a health question.
const OWNER_MARKS: &[&str] = &[
    "sta imam", "sta imas", "koliko imam", "koliko imas", "da li imam", "da li imas", "imam li", "imas li", "jel imam", "do i have",
    "have i got",
];

/// Questions that are clearly about the household's own supplies, whatever
/// the model thought.
pub fn mentions_supplies(question: &str) -> bool {
    has_mark(question, SUPPLY_MARKS)
}

/// A question the model sent to the library that belongs to the supplies:
/// a clear supplies cue, or "da li imam…" naming a stored item or a kind of
/// them ("Šta imam od lekova?").
pub fn supplies_override(question: &str, terms: &[String], items: &[Item]) -> bool {
    if mentions_supplies(question) {
        return true;
    }
    has_mark(question, OWNER_MARKS)
        && (terms.iter().any(|t| !matches!(match_items(t, items), ItemMatch::None))
            || question.split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() >= 3).any(|w| category_of_word(w).is_some()))
}

/// A question rather than a request: "Šta nam ponestaje?", not "Stavi hleb na listu".
fn is_question(question: &str) -> bool {
    let q = padded(question);
    const REQUESTS: &[&str] = &[" da li mozes ", " da li bi ", " mozes li ", " mozete li ", " molim ", " can you ", " could you ", " please ", " would you "];
    if REQUESTS.iter().any(|r| q.starts_with(r)) {
        return false;
    }
    const STARTS: &[&str] = &[
        " sta ", " koliko ", " kolko ", " da li ", " jel ", " je l ", " imamo li ", " imam li ", " ima li ", " koji ", " koja ", " koje ", " what ",
        " how ", " do we ", " do i ", " is there ", " are there ", " which ",
    ];
    STARTS.iter().any(|s| q.starts_with(s))
}

/// A word about the supplies, taken by the model for the name of an item.
fn supply_word(name: &str) -> bool {
    let n = plain(name.trim());
    ["ponestaje", "ponestalo", "istice", "isticu", "isteklo", "zalihe", "zaliha", "running low", "expiring", "supplies"].contains(&n.as_str())
}

/// "konzerva, pasulj, rok trajanja" -> terms; tolerant of numbering, quotes and odd separators.
pub fn parse_keywords(text: &str) -> Vec<String> {
    text.split([',', ';', '\n'])
        .map(|t| t.trim().trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == '-' || c == '*' || c == ' ').trim())
        .map(|t| t.trim_matches(|c: char| c == '"' || c == '\'' || c == '„' || c == '“' || c == '.' || c == '*'))
        .filter(|t| !t.is_empty() && t.chars().count() <= 40 && t.split_whitespace().count() <= 3)
        .map(|t| zaklon_core::translit::cyrillic_to_latin(t).to_lowercase())
        .fold(Vec::new(), |mut acc: Vec<String>, t| {
            if !acc.contains(&t) {
                acc.push(t);
            }
            acc
        })
        .into_iter()
        .take(4)
        .collect()
}

/// Phrases, beginnings of words and whole words of a question about health
/// or first aid (plain Latin, lower case), for when the model does not say.
const HEALTH_PHRASES: &[&str] = &[
    "prva pomoc", "prvu pomoc", "prvoj pomoci", "hitna pomoc", "hitnu pomoc", "masaza srca", "strujni udar", "strujnog udara",
    "toplotni udar", "ugljen monoksid", "first aid", "heart attack", "carbon monoxide", "electric shock",
];
const HEALTH_STARTS: &[&str] = &[
    "povred", "krvar", "opekot", "opeklin", "opece", "opekao", "opekla", "gusen", "zagrcn", "trovan", "otrov", "najotrov", "botul", "lekov",
    "lekar", "doziran", "tablet", "antibiot", "paracetamol", "ibuprofen", "brufen", "aspirin", "febricet", "nurofen", "panadol", "temperatur",
    "groznic", "nesvest", "onesves", "srcan", "infarkt", "mozdan", "alerg", "anafila", "zmij", "poskok", "krpelj", "pcel", "strsljen", "ujed",
    "ujel", "ujeo", "ubod", "ubol", "reanimac", "ozivljav", "disanj", "povrac", "proliv", "mucnin", "vrtoglav", "glavobolj", "dijabet",
    "insulin", "astm", "epilep", "trudn", "prelom", "slomlj", "uganu", "dehidrat", "suncanic", "smrzot", "promrz", "hipoterm", "bolest",
    "bolesn", "bolov", "infekc", "zaraz", "vakcin", "posekot", "injur", "bleed", "chok", "poison", "medic", "dosage", "overdose", "fever",
    "unconscious", "faint", "stroke", "allerg", "anaphyla", "snake", "venom", "wound", "fractur", "vomit", "diarrh", "diabet", "asthma",
    "pregnan", "acetaminophen", "seizure", "hypotherm", "frostbit", "heatstroke", "dehydrat", "concuss", "infect", "sprain", "nausea",
    "headache", "drown",
];
const HEALTH_WORDS: &[&str] = &[
    "lek", "leka", "leku", "lekom", "rana", "ranu", "rane", "krv", "krvi", "gusi", "guse", "davi", "boli", "bole", "bol", "doza", "dozu", "doze",
    "upala", "upalu", "upale", "dise", "srce", "srca", "slog", "sloga", "kasalj", "bite", "bites", "bitten", "sting", "stings", "stung", "tick",
    "ticks", "pill", "pills", "dose", "doses", "burn", "burns", "burned", "burnt", "pain", "cpr",
];

/// A question about health or first aid, by its words. Used together with
/// the model's own `safety` answer; a false alarm only makes the rules stricter.
pub fn health_question(question: &str) -> bool {
    let q = padded(question);
    if HEALTH_PHRASES.iter().any(|p| q.contains(&format!(" {p} "))) {
        return true;
    }
    q.split_whitespace().any(|w| HEALTH_WORDS.contains(&w) || HEALTH_STARTS.iter().any(|s| w.starts_with(s)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::test_util::item;

    #[test]
    fn keywords_are_parsed_from_what_the_model_writes() {
        assert_eq!(parse_keywords("konzerva, pasulj, rok trajanja"), vec!["konzerva", "pasulj", "rok trajanja"]);
        assert_eq!(parse_keywords("1. Voda\n2. Prečišćavanje vode\n"), vec!["voda", "prečišćavanje vode"]);
        assert_eq!(parse_keywords("\"Вода\", \"филтер\"."), vec!["voda", "filter"]);
        assert!(parse_keywords("").is_empty());
        assert_eq!(parse_keywords("so, so, so"), vec!["so"]);
        assert!(parse_keywords("This is a very long sentence that is not a keyword at all").is_empty());
    }

    #[test]
    fn remember_requests_become_notes() {
        assert_eq!(remember_request("Zapamti da je Ana alergična na penicilin").as_deref(), Some("Ana je alergična na penicilin."));
        assert_eq!(remember_request("zapamti: ključ od podruma je kod komšije").as_deref(), Some("Ključ od podruma je kod komšije."));
        assert_eq!(remember_request("Remember that the water tank holds 200 liters.").as_deref(), Some("The water tank holds 200 liters."));
        assert!(remember_request("Kako da zapamtim brojeve?").is_none());
    }

    #[test]
    fn supply_words_are_recognised() {
        for q in [
            "Šta imam u zalihama?",
            "sta mi istice ove nedelje",
            "Koliko imamo brašna?",
            "Da li imamo sveće u kući?",
            "Šta nam ponestaje?",
            "Pokaži naše zalihe",
            "Šta je na listi za kupovinu?",
            "Шта имам у залихама?",
            "What's on the shopping list?",
            "Do we have any rice?",
            "How much water is in our supplies?",
            "What's in my pantry?",
            "What expires this week?",
            "What are we running low on?",
        ] {
            assert!(mentions_supplies(q), "{q}");
        }
        for q in [
            "Koliko dugo mogu da čuvam zalihe vode?",
            "Kako da napravim zalihe hrane za zimu?",
            "Kako se leči ubod pčele?",
            "Imam temperaturu, šta da radim?",
            "Koje zalihe su potrebne za 72 sata?",
            "Kako se čuva brašno?",
            "How do I treat a burn?",
            "How long can I store water supplies?",
            "How do I build an emergency pantry?",
            "What supplies should a first aid kit contain?",
            "Does canned food expire?",
            "How do I ration food during a long blackout?",
        ] {
            assert!(!mentions_supplies(q), "{q}");
        }
    }

    #[test]
    fn plans_are_parsed_and_cleaned() {
        let p = parse_plan(r#"{"kind":"supplies_change","terms":["Млеко"],"change":{"action":"add","name":"mleko","quantity":2,"unit":"l","category":"drink"}}"#);
        assert_eq!(p.kind, "supplies_change");
        assert_eq!(p.terms, vec!["mleko"]);
        assert_eq!(p.change.unwrap().quantity, 2.0);
        let bad_unit = parse_plan(r#"{"kind":"supplies_change","terms":[],"change":{"action":"add","name":"x","quantity":-1,"unit":"liters","category":"?"}}"#);
        let c = bad_unit.change.unwrap();
        assert_eq!((c.unit.as_str(), c.category.as_str(), c.quantity), ("pcs", "other", 0.0));
        let not_json = parse_plan("hleb, rok trajanja");
        assert_eq!(not_json.kind, "library");
        assert_eq!(not_json.terms, vec!["hleb", "rok trajanja"]);
        let no_change = parse_plan(r#"{"kind":"supplies_change","terms":["x"]}"#);
        assert_eq!(no_change.kind, "supplies_question");
    }

    #[test]
    fn plain_supplies_questions_need_no_plan() {
        for q in [
            "koliko imamo brasna",
            "sta nam istice ovog meseca?",
            "imamo li jos vode u flasama",
            "Šta treba da kupim, šta nam ponestaje?",
            "jel imamo baterija za lampu",
            "Колико имамо шећера?",
            "do we have enough rice for a week?",
            "Šta je na listi za kupovinu?",
        ] {
            assert!(plain_supplies_question(q), "{q}");
            let mut p = supplies_plan(q);
            route(&mut p, q, &[]);
            assert_eq!(p.kind, "supplies_question", "{q}");
        }
        for q in [
            // Library questions, requests, and supplies questions that need the plan's judgement.
            "koliko vode treba jednom coveku dnevno, pravim zalihe",
            "Kako da napravim zalihe hrane za zimu?",
            "stavi toalet papir na listu za kupovinu",
            "Da li možeš da staviš hleb na listu za kupovinu?",
            "treba da kupimo 2 kila brasna, zapisi na spisak",
            "da li imam paracetamol u kuci",
            "sta imam od lekova",
            "zapamti da je Ana alergicna na ibuprofen",
            "Da li imam upalu grla ako me boli kad gutam?",
        ] {
            assert!(!plain_supplies_question(q), "{q}");
        }
        assert_eq!(supplies_plan("koliko imamo brasna").terms, vec!["imam", "brasn"]);
    }

    #[test]
    fn health_questions_are_recognised() {
        for q in [
            "sta da radim kad se neko opece na sporet, jel stavljam led?",
            "kako se zaustavlja krvarenje iz nosa",
            "Dete je progutalo nesto i gusi se, sta radim??",
            "kako se radi masaza srca, koliko pritisaka pa koliko udisaja",
            "kako da prepoznam poskoka i sta ako te ujede zmija",
            "uhvatio me krpelj, kako da ga izvadim?",
            "Ana ima temperaturu 38.5, jel moze da popije brufen",
            "How do I treat a bee sting?",
            "koje su najotrovnije pecurke kod nas",
        ] {
            assert!(health_question(q), "{q}");
        }
        for q in [
            "kad je poceo prvi srpski ustanak i ko ga je vodio",
            "kako da zamenim osigurac u kuci, izbacuje mi struju",
            "curi mi slavina u kupatilu sta da radim",
            "Koliko dugo moze da stoji kuvano jelo u frizideru",
            "koliko kosta hleb danas u maksiju u nisu",
            "How long does bread last?",
            "kako da upalim sporet",
        ] {
            assert!(!health_question(q), "{q}");
        }
    }

    #[test]
    fn health_questions_are_not_taken_for_supplies() {
        let items = vec![item("Paracetamol", 20.0, "pcs"), item("Brašno", 2.0, "kg")];
        let plan = |kind: &str, terms: &[&str]| Plan { kind: kind.into(), terms: terms.iter().map(|s| s.to_string()).collect(), ..Plan::default() };
        let mut p = plan("library", &["upala grla"]);
        route(&mut p, "Da li imam upalu grla ako me boli kad gutam?", &items);
        assert_eq!(p.kind, "library");
        let mut p = plan("library", &["groznica"]);
        route(&mut p, "Do I have a fever if my temperature is 37.8?", &items);
        assert_eq!(p.kind, "library");
        let mut p = plan("library", &["paracetamol"]);
        route(&mut p, "da li imam paracetamol u kuci", &items);
        assert_eq!(p.kind, "supplies_question", "a stored item is named");
        let mut p = plan("library", &["baterija"]);
        route(&mut p, "jel imamo baterija za lampu", &items);
        assert_eq!(p.kind, "supplies_question");
        let mut p = plan("library", &["lekovi"]);
        route(&mut p, "sta imam od lekova", &items);
        assert_eq!(p.kind, "supplies_question", "a kind of stored things");
        let mut p = plan("library", &["bol u stomaku"]);
        route(&mut p, "Šta imam ako me boli stomak i imam proliv?", &items);
        assert_eq!(p.kind, "library");
    }

    #[test]
    fn a_question_about_the_supplies_is_not_a_change() {
        let change = |name: &str| PlannedChange { action: "shopping".into(), name: name.into(), quantity: 0.0, unit: "pcs".into(), category: "other".into() };
        let mut p = Plan { kind: "supplies_change".into(), change: Some(change("Ponestaje")), ..Plan::default() };
        route(&mut p, "Šta treba da kupim, šta nam ponestaje?", &[]);
        assert_eq!(p.kind, "supplies_question");
        assert!(p.change.is_none());
        let mut p = Plan { kind: "supplies_change".into(), change: Some(change("toalet papir")), ..Plan::default() };
        route(&mut p, "stavi toalet papir na listu za kupovinu", &[]);
        assert_eq!(p.kind, "supplies_change");
        let mut p = Plan { kind: "supplies_change".into(), change: Some(change("hleb")), ..Plan::default() };
        route(&mut p, "Da li možeš da staviš hleb na listu za kupovinu?", &[]);
        assert_eq!(p.kind, "supplies_change", "a polite request is still a change");
    }
}
