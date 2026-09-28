//! A written answer before it is shown: its citations checked, a health
//! answer cut to what its sources back, and the hub's own words where the
//! model has nothing checked to say.

use super::Source;

/// Written by the hub under every health answer.
fn emergency_line(sr: bool) -> &'static str {
    if sr {
        "Ako je hitno: Hitna pomoć 194 (Srbija) ili 112 (EU)."
    } else {
        "If it is urgent: ambulance 194 (Serbia) or 112 (EU)."
    }
}

/// Household notes, shown by the hub above a health answer.
fn notes_block(notes: &[String], sr: bool) -> String {
    if notes.is_empty() {
        return String::new();
    }
    let head = if sr { "Beleške domaćinstva:" } else { "Household notes:" };
    format!("{head}\n{}\n\n", notes.iter().map(|n| format!("- {n}")).collect::<Vec<_>>().join("\n"))
}

/// The hub's own reply to a health question with no checked answer in the
/// library: where to get help, not advice from the model's memory.
pub(super) fn fixed_reply(language: &str, notes: &[String]) -> String {
    let sr = language == "sr";
    let mut t = notes_block(notes, sr);
    t.push_str(if sr {
        "U biblioteci nemam proveren odgovor na ovo. Ako je hitno, pozovi Hitnu pomoć: 194 u Srbiji, ili 112, broj za hitne slučajeve u EU. \
Ako nije hitno, pitaj lekara ili farmaceuta."
    } else {
        "I have no checked answer for this in the library. If it is urgent, call an ambulance: 194 in Serbia, or 112, the emergency number in the EU. \
Otherwise ask a doctor or pharmacist."
    });
    t
}

/// Source numbers cited in an answer: "[1]", "[2, 3]".
fn cited_numbers(text: &str) -> Vec<usize> {
    let mut out = Vec::new();
    for part in text.split('[').skip(1) {
        let Some(inner) = part.split(']').next() else { continue };
        for n in inner.split(',') {
            if let Ok(n) = n.trim().parse::<usize>() {
                if !out.contains(&n) {
                    out.push(n);
                }
            }
        }
    }
    out
}

/// "[1]" or "[1, 2]" at the start: its length in characters.
fn mark_len(chars: &[char]) -> Option<usize> {
    if chars.first() != Some(&'[') {
        return None;
    }
    let end = chars.iter().position(|c| *c == ']')?;
    let inner = &chars[1..end];
    (inner.iter().all(|c| c.is_ascii_digit() || *c == ',' || *c == ' ') && inner.iter().any(|c| c.is_ascii_digit())).then_some(end + 1)
}

/// The text with citation marks that point to no source removed ("[4]" when
/// there are three, often copied from an earlier answer), the others kept.
pub(super) fn keep_marks(text: &str, numbers: &[usize]) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let Some(len) = mark_len(&chars[i..]) else {
            out.push(chars[i]);
            i += 1;
            continue;
        };
        let inner: String = chars[i + 1..i + len - 1].iter().collect();
        let valid: Vec<String> = inner.split(',').filter_map(|n| n.trim().parse::<usize>().ok()).filter(|n| numbers.contains(n)).map(|n| n.to_string()).collect();
        if valid.is_empty() {
            // "text [4]." becomes "text."
            while out.ends_with(' ') {
                out.pop();
            }
        } else {
            out.push_str(&format!("[{}]", valid.join(", ")));
        }
        i += len;
    }
    out
}

/// Whether the mark just added ends a sentence: not the number of a list
/// item ("1.") or a short form ("npr.", "min.").
fn ends_sentence(sentence: &str) -> bool {
    let body = sentence.trim_end_matches(['.', '!', '?']);
    let words: Vec<&str> = body.split_whitespace().collect();
    let Some(last) = words.last() else { return false };
    if words.len() == 1 && last.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    const SHORT: &[&str] = &["npr", "tj", "dr", "br", "st", "min", "e.g", "i.e", "approx", "vs", "mr", "mrs"];
    !SHORT.contains(&last.trim_start_matches('(').to_lowercase().as_str())
}

/// The sentences of one line, each with the citation marks that follow it
/// ("Ohladi vodom. [1]" is one sentence).
pub(super) fn split_sentences(line: &str) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut i = 0;
    while i < chars.len() {
        cur.push(chars[i]);
        i += 1;
        let end = matches!(chars[i - 1], '.' | '!' | '?') && chars.get(i).is_none_or(|c| c.is_whitespace());
        if !end || !ends_sentence(&cur) {
            continue;
        }
        // Citation marks right after the end belong to this sentence.
        loop {
            let mut k = i;
            while chars.get(k) == Some(&' ') {
                k += 1;
            }
            let Some(len) = mark_len(&chars[k.min(chars.len())..]) else { break };
            cur.extend(&chars[i..k + len]);
            i = k + len;
            if let Some(p) = chars.get(i).filter(|p| matches!(p, '.' | '!' | '?')) {
                cur.push(*p);
                i += 1;
            }
        }
        let s = cur.trim();
        if !s.is_empty() {
            out.push(s.to_string());
        }
        cur.clear();
    }
    let s = cur.trim();
    if !s.is_empty() {
        out.push(s.to_string());
    }
    out
}

/// Only the sentences that name one of the sources, line by line: a health
/// answer shows nothing the sources do not back. While an answer is being
/// written, a sentence appears once its citation has arrived.
pub(super) fn cited_sentences(text: &str, numbers: &[usize]) -> String {
    let mut lines = Vec::new();
    for line in text.lines() {
        let kept: Vec<String> = split_sentences(line).into_iter().filter(|s| cited_numbers(s).iter().any(|n| numbers.contains(n))).collect();
        if !kept.is_empty() {
            lines.push(kept.join(" "));
        }
    }
    lines.join("\n")
}

/// How a written answer is checked before it is shown.
#[derive(Debug, Clone, Default)]
pub(super) struct Finish {
    /// Written from sources: citations are checked, and an answer that cites
    /// none of them is not grounded.
    pub(super) library: bool,
    /// A health question: only sentences that name a source are kept.
    pub(super) safety: bool,
    /// Every source is a web page.
    pub(super) web_only: bool,
    /// Notes the hub shows above a health answer.
    pub(super) notes: Vec<String>,
}

#[derive(Debug)]
pub(super) struct Finished {
    pub(super) text: String,
    pub(super) sources: Vec<Source>,
    pub(super) cited: bool,
    pub(super) grounded: bool,
    pub(super) fixed: bool,
}

/// The final text of an answer and the sources to show with it.
pub(super) fn finish_answer(raw: &str, sources: &[Source], finish: &Finish, language: &str) -> Finished {
    let sr = language == "sr";
    let text = finish_text(raw, language);
    if !finish.library {
        return Finished { text, sources: sources.to_vec(), cited: false, grounded: false, fixed: false };
    }
    let numbers: Vec<usize> = sources.iter().map(|s| s.n).collect();
    let text = keep_marks(&text, &numbers);
    let used_sources = |used: &[usize]| sources.iter().filter(|s| used.contains(&s.n)).cloned().collect::<Vec<_>>();
    if finish.safety {
        let kept = cited_sentences(&text, &numbers);
        if kept.is_empty() {
            return Finished { text: fixed_reply(language, &finish.notes), sources: Vec::new(), cited: false, grounded: false, fixed: true };
        }
        let mut out = notes_block(&finish.notes, sr);
        if finish.web_only {
            out.push_str(if sr { "Sa interneta, neprovereno:\n" } else { "From the internet, not checked:\n" });
        }
        out.push_str(&kept);
        out.push_str("\n\n");
        out.push_str(emergency_line(sr));
        return Finished { text: out, sources: used_sources(&cited_numbers(&kept)), cited: true, grounded: true, fixed: false };
    }
    let used = cited_numbers(&text);
    if used.is_empty() {
        // It names no source: it may come from the model's own memory.
        return Finished { text, sources: sources.to_vec(), cited: false, grounded: false, fixed: false };
    }
    Finished { text, sources: used_sources(&used), cited: true, grounded: true, fixed: false }
}

/// Serbian answers in Latin script, whatever the model wrote.
pub(super) fn finish_text(text: &str, language: &str) -> String {
    // Drop lines that are only citation marks ("[1]") left at the end, but
    // keep a line with only a number on it ("194").
    let kept: Vec<&str> = text
        .lines()
        .filter(|l| {
            let t = l.trim();
            t.is_empty() || !(t.contains('[') && t.chars().all(|c| c == '[' || c == ']' || c == ',' || c == ' ' || c.is_ascii_digit()))
        })
        .collect();
    let joined = kept.join("\n");
    let t = joined.trim();
    if language == "sr" && zaklon_core::translit::has_cyrillic(t) {
        zaklon_core::translit::cyrillic_to_latin(t)
    } else {
        t.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::test_util::passage;

    #[test]
    fn cited_sources_are_found() {
        assert_eq!(cited_numbers("A [1]. B [2, 3]. C [1]."), vec![1, 2, 3]);
        assert!(cited_numbers("No sources.").is_empty());
        assert_eq!(cited_numbers("[x] and [3]"), vec![3]);
    }

    #[test]
    fn serbian_answers_end_in_latin() {
        assert_eq!(finish_text(" Пасуљ траје дуго. ", "sr"), "Pasulj traje dugo.");
        assert_eq!(finish_text("Beans last long.", "en"), "Beans last long.");
        assert_eq!(finish_text("Boil it [1].

[1]
", "en"), "Boil it [1].");
        assert_eq!(finish_text("Pozovi Hitnu pomoć:\n194", "sr"), "Pozovi Hitnu pomoć:\n194", "a number alone is not a citation");
    }

    fn sources(n: usize) -> Vec<Source> {
        (1..=n).map(|i| passage(i, &format!("S{i}"), "").source).collect()
    }

    #[test]
    fn citations_that_point_nowhere_are_not_grounding() {
        let library = Finish { library: true, ..Finish::default() };
        let f = finish_answer("Prokuvaj vodu [4].", &sources(3), &library, "sr");
        assert_eq!(f.text, "Prokuvaj vodu.");
        assert!(!f.grounded && !f.cited);
        assert_eq!(f.sources.len(), 3, "the sources stay visible to check the answer against");
        let f = finish_answer("Prokuvaj vodu [2, 7]. Ohladi je.", &sources(3), &library, "sr");
        assert_eq!(f.text, "Prokuvaj vodu [2]. Ohladi je.");
        assert!(f.grounded && f.cited);
        assert_eq!(f.sources.iter().map(|s| s.n).collect::<Vec<_>>(), vec![2]);
        let supplies = finish_answer("Imaš 2 kg brašna.", &[], &Finish::default(), "sr");
        assert_eq!(supplies.text, "Imaš 2 kg brašna.");
    }

    #[test]
    fn health_answers_keep_only_what_the_sources_back() {
        let health = Finish { library: true, safety: true, ..Finish::default() };
        let f = finish_answer("Ostani miran. [1] Isisaj otrov iz rane.\nIdi odmah u bolnicu [2].\n**Važno:**", &sources(2), &health, "sr");
        assert_eq!(f.text, "Ostani miran. [1]\nIdi odmah u bolnicu [2].\n\nAko je hitno: Hitna pomoć 194 (Srbija) ili 112 (EU).");
        assert!(f.grounded && !f.fixed);
        let none = finish_answer("U biblioteci nisam našao pouzdan odgovor.", &sources(2), &health, "sr");
        assert!(none.fixed && !none.grounded && none.sources.is_empty());
        assert!(none.text.starts_with("U biblioteci nemam proveren odgovor na ovo.") && none.text.contains("194") && none.text.contains("112"), "{}", none.text);
        let noted = Finish { notes: vec!["Ana je alergična na ibuprofen.".into()], ..health.clone() };
        let f = finish_answer("Paracetamol snižava temperaturu [1].", &sources(1), &noted, "sr");
        assert!(f.text.starts_with("Beleške domaćinstva:\n- Ana je alergična na ibuprofen.\n\nParacetamol"), "{}", f.text);
        let web = Finish { web_only: true, ..health };
        assert!(finish_answer("Ohladi vodom [1].", &sources(1), &web, "sr").text.starts_with("Sa interneta, neprovereno:\nOhladi vodom [1]."));
        // While it is written, a sentence shows once its citation has come.
        assert_eq!(cited_sentences("Ohladi opekotinu hladnom vodom", &[1]), "");
        assert_eq!(cited_sentences("Ohladi opekotinu hladnom vodom. [1] Ne", &[1]), "Ohladi opekotinu hladnom vodom. [1]");
    }

    #[test]
    fn sentences_are_split_where_they_end() {
        assert_eq!(split_sentences("Ohladi vodom. [1] Ne stavljaj led [2]. Pozovi 194."), vec!["Ohladi vodom. [1]", "Ne stavljaj led [2].", "Pozovi 194."]);
        assert_eq!(split_sentences("1. Hladi npr. 20 min. pod vodom [1]."), vec!["1. Hladi npr. 20 min. pod vodom [1]."]);
        assert_eq!(split_sentences("Temperatura 37.5 je povišena [1]!"), vec!["Temperatura 37.5 je povišena [1]!"]);
        assert_eq!(keep_marks("A [1]. B [3]. C [1, 3, 2].", &[1, 2]), "A [1]. B. C [1, 2].");
    }
}
