//! What the household asked the assistant to remember: the notes that
//! matter for a question, and how they go into the prompt.

use zaklon_core::memory::Note;

use super::text::{plain, search_words, starts_word, stem, words};

/// All household notes in a prompt together.
const NOTES_CHARS: usize = 1200;
const MAX_NOTES: usize = 8;

/// The root of a word for matching names and nouns in their forms:
/// "Ana", "Ani", "Anu", "Anom" -> "an"; "penicilina" -> "penicilin".
fn root(word: &str) -> String {
    let w = plain(word);
    let n = w.chars().count();
    if n <= 4 {
        for e in ["om", "em", "oj", "a", "e", "i", "u", "o"] {
            if w.ends_with(e) && n - e.len() >= 2 {
                return w[..w.len() - e.len()].to_string();
            }
        }
        return w;
    }
    stem(&w)
}

/// Brand and generic names of common medicines, which are the same medicine
/// for the notes: "brufen" finds "alergična na ibuprofen".
const MEDICINES: &[&[&str]] = &[
    &["ibuprofen", "brufen", "nurofen", "advil"],
    &["paracetamol", "acetaminophen", "panadol", "febricet", "tylenol"],
    &["aspirin", "andol", "acetilsalicil", "acetylsalicyl"],
    &["penicilin", "penicillin", "amoksicilin", "amoxicillin", "amoksiklav", "augmentin", "sinacilin"],
    &["diklofenak", "diclofenac", "voltaren"],
];

/// Beginnings of words that make a note about health (allergies, illnesses, medicines).
const HEALTH_NOTE: &[&str] = &[
    "alergi", "alergic", "allerg", "lek", "bolest", "bolesn", "dijabet", "diabet", "astm", "asthm", "trudn", "pregnan", "pritis", "epilep",
    "medic", "insulin", "srcan", "terapij",
];

/// The notes that matter for a question: those sharing a word with it, in
/// any of its forms or as another name of the same medicine. At most eight,
/// and not too long together.
pub fn relevant_notes(notes: &[Note], question: &str, terms: &[String]) -> Vec<String> {
    // Short words ("na", "je", "i") say nothing about a note; a name like
    // "Ana" does (its root "an" is short, so words are measured before rooting).
    let long_enough = |w: &&str| w.chars().count() >= 3;
    let mut words: Vec<String> = search_words(question).iter().map(String::as_str).filter(long_enough).map(root).collect();
    words.extend(terms.iter().flat_map(|t| t.split_whitespace().filter(long_enough).map(root).collect::<Vec<_>>()));
    // "brufen" also as "ibuprofen", "nurofen"...
    let said: Vec<String> = question.split(|c: char| !c.is_alphanumeric()).chain(terms.iter().flat_map(|t| t.split_whitespace())).map(plain).collect();
    for group in MEDICINES {
        if said.iter().any(|w| group.iter().any(|m| w.starts_with(m))) {
            words.extend(group.iter().map(|m| root(m)));
        }
    }
    words.retain(|w| w.chars().count() >= 2);
    let picked = notes
        .iter()
        .filter(|n| {
            let note_words: Vec<String> = n.text.split(|c: char| !c.is_alphanumeric()).filter(long_enough).map(root).collect();
            words.iter().any(|w| note_words.iter().any(|nw| nw == w || prefix_match(nw, w)))
        })
        .map(|n| n.text.clone());
    limit_notes(picked)
}

/// The same word in another form: one begins the other and the shorter is
/// long enough to mean something, or they share a long beginning
/// ("alergija" / "alergična"). So "na" never matches "nalazi".
fn prefix_match(a: &str, b: &str) -> bool {
    let (short, long) = if a.chars().count() <= b.chars().count() { (a, b) } else { (b, a) };
    if short.chars().count() >= 4 && long.starts_with(short) {
        return true;
    }
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count() >= 5
}

/// For a health question: the notes found for it, and every note about
/// health too. An allergy matters whatever words the question uses.
pub fn with_health_notes(found: &[String], notes: &[Note]) -> Vec<String> {
    let health = notes.iter().filter(|n| health_note(&n.text)).map(|n| n.text.clone());
    limit_notes(found.iter().cloned().chain(health))
}

fn health_note(text: &str) -> bool {
    let p = plain(text);
    let w = words(&p);
    HEALTH_NOTE.iter().any(|h| starts_word(&w, h))
}

/// At most `MAX_NOTES` different notes, `NOTES_CHARS` together.
fn limit_notes(notes: impl Iterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut len = 0;
    for n in notes {
        if out.len() >= MAX_NOTES {
            break;
        }
        let l = n.chars().count();
        if out.contains(&n) || len + l > NOTES_CHARS {
            continue;
        }
        len += l;
        out.push(n);
    }
    out
}

/// Put what the household asked to remember in front of the question, with
/// a rule to take it into account first. Small models skip a note that sits
/// quietly in the instructions ("Ana is allergic to penicillin" matters more
/// than any encyclopedia article about penicillin). With `mention`, the model
/// is asked to name the note at the start; health answers leave that to the
/// hub, which shows the notes above the answer itself. The rule goes with
/// the notes, not into the instructions: those stay the same from one
/// question to the next, notes or not, so the engine need not read them again.
pub(super) fn with_notes(mut messages: Vec<serde_json::Value>, notes: &[String], language: &str, mention: bool) -> Vec<serde_json::Value> {
    if notes.is_empty() {
        return messages;
    }
    let sr = language == "sr";
    let rule = match (sr, mention) {
        (true, true) => "Beleške domaćinstva su proverene činjenice o ovoj porodici. Ako se neka beleška tiče pitanja, uzmi je u obzir pre svega i pomeni je na početku odgovora.",
        (true, false) => "Beleške domaćinstva su proverene činjenice o ovoj porodici. Uzmi ih u obzir (na primer alergije) i ne predlaži ništa što im protivreči.",
        (false, true) => "The household notes are checked facts about this family. If a note matters for the question, take it into account before anything else and mention it at the start of the answer.",
        (false, false) => "The household notes are checked facts about this family. Take them into account (allergies, for example) and suggest nothing that goes against them.",
    };
    let head = if sr { "Beleške domaćinstva:" } else { "Household notes:" };
    let list = notes.iter().map(|n| format!("- {n}")).collect::<Vec<_>>().join("\n");
    if let Some(user) = messages.last_mut() {
        let content = user["content"].as_str().unwrap_or_default().to_string();
        user["content"] = serde_json::Value::String(format!("{head}\n{list}\n{rule}\n\n{content}"));
    }
    messages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_reach_the_prompt() {
        let note = |t: &str| Note { id: t.into(), text: t.into(), created_at: String::new(), created_by: None };
        let few = vec![note("Ana je alergična na penicilin.")];
        assert_eq!(relevant_notes(&few, "Šta da dam Ani za temperaturu?", &[]).len(), 1, "the name in another form");
        assert!(relevant_notes(&few, "Kako da prečistim vodu bez filtera?", &["voda".into()]).is_empty(), "not about Ana or penicillin");
        let many: Vec<Note> = (0..20).map(|i| note(&format!("Beleška broj {i} o nečemu."))).chain([note("Ana je alergična na penicilin.")]).collect();
        let r = relevant_notes(&many, "Da li Ana sme penicilin?", &[]);
        assert_eq!(r, vec!["Ana je alergična na penicilin."]);
        let m = with_notes(vec![serde_json::json!({"role":"system","content":"Base."}), serde_json::json!({"role":"user","content":"Pitanje?"})], &r, "sr", true);
        assert_eq!(m[0]["content"], "Base.", "the instructions stay the same, notes or not (the engine keeps them read)");
        let user = m[1]["content"].as_str().unwrap();
        assert!(user.starts_with("Beleške domaćinstva:\n- Ana je alergična na penicilin.\nBeleške domaćinstva su proverene"), "{user}");
        assert!(user.ends_with("pomeni je na početku odgovora.\n\nPitanje?"), "{user}");
    }

    #[test]
    fn notes_find_other_names_of_the_same_medicine() {
        let note = |t: &str| Note { id: t.into(), text: t.into(), created_at: String::new(), created_by: None };
        let notes = vec![note("Marko je alergičan na ibuprofen."), note("Deda pije lek za pritisak."), note("Ključ je kod komšije.")];
        assert_eq!(relevant_notes(&notes, "Mogu li detetu da dam brufen za temperaturu?", &[]), vec!["Marko je alergičan na ibuprofen."]);
        let health = with_health_notes(&[], &notes);
        assert_eq!(health, vec!["Marko je alergičan na ibuprofen.", "Deda pije lek za pritisak."], "every note about health, not the key");
        let many: Vec<Note> = (0..20).map(|i| note(&format!("Alergija broj {i}: {}", "x".repeat(200)))).collect();
        let limited = with_health_notes(&[], &many);
        assert!(limited.len() <= MAX_NOTES && limited.iter().map(|n| n.chars().count()).sum::<usize>() <= NOTES_CHARS);
    }
}

#[cfg(test)]
mod note_matching_tests {
    use super::*;

    fn note(text: &str) -> Note {
        Note { id: "n1".into(), text: text.into(), created_at: "2026-09-28T10:00:00Z".into(), created_by: None }
    }

    #[test]
    fn a_short_note_word_does_not_match_a_longer_question_word() {
        let notes = [note("Ana je alergična na ibuprofen.")];
        assert!(relevant_notes(&notes, "Koliko je visok Midžor i gde se nalazi?", &[]).is_empty());
        assert!(relevant_notes(&notes, "koja je najduza reka kroz srbiju", &[]).is_empty());
    }

    #[test]
    fn the_note_is_found_by_name_and_by_medicine() {
        let notes = [note("Ana je alergična na ibuprofen.")];
        assert_eq!(relevant_notes(&notes, "Ana ima temperaturu, šta da joj dam?", &[]).len(), 1);
        assert_eq!(relevant_notes(&notes, "jel moze da popije brufen", &[]).len(), 1);
        assert_eq!(relevant_notes(&notes, "da li je alergija opasna", &[]).len(), 1);
    }
}
