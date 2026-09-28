//! Words and plain text: what people type folded to plain Latin, rough
//! stems, stop words, clipping, and the text of an article's HTML.

/// Lower-case Latin without diacritics, for matching what people type
/// ("brasno") with what is stored ("Brašno", "Брашно").
pub(super) fn plain(s: &str) -> String {
    zaklon_core::translit::cyrillic_to_latin(&s.to_lowercase())
        .chars()
        .map(|c| match c {
            'č' | 'ć' => "c".to_string(),
            'š' => "s".to_string(),
            'ž' => "z".to_string(),
            'đ' => "dj".to_string(),
            c => c.to_string(),
        })
        .collect()
}

/// The basic form of a word, roughly: Serbian case endings and English plurals off.
pub fn stem(word: &str) -> String {
    let w = word.to_lowercase();
    let n = w.chars().count();
    const ENDINGS: [&str; 16] = ["ama", "ima", "ovi", "eve", "om", "em", "og", "ih", "im", "es", "a", "e", "i", "u", "o", "s"];
    for e in ENDINGS {
        let keep = if e.chars().count() == 1 { 3 } else { 4 };
        if w.ends_with(e) && n - e.chars().count() >= keep {
            return w[..w.len() - e.len()].to_string();
        }
    }
    w
}

const STOP_SR: &[&str] = &[
    "je", "da", "li", "koliko", "kako", "sta", "šta", "gde", "zasto", "zašto", "koji", "koja", "koje", "sam", "se", "za", "od", "na", "u", "i",
    "treba", "moze", "može", "mogu", "ima", "nema", "kada", "kad", "sto", "što", "ili", "ne", "mi", "ti", "su", "biti", "bi", "da", "po", "sa", "iz",
    "o", "a", "ali", "ako", "to", "taj", "ta", "te", "ovo", "ono", "nešto", "nesto", "neki", "neka", "koliki", "najbolje", "dobro", "moj", "moja",
    "jel", "radim", "kuci", "kući", "kod", "nam", "mu", "ga", "jos", "još", "nas",
];
const STOP_EN: &[&str] = &[
    "the", "is", "are", "how", "what", "does", "do", "can", "why", "where", "which", "of", "to", "in", "and", "a", "an", "it", "should", "i", "my",
    "you", "when", "much", "many", "for", "with", "be", "on", "at", "by", "or", "if", "me", "we", "our", "this", "that", "there", "best", "way",
];

pub(super) fn is_stop_word(word: &str) -> bool {
    let l = word.to_lowercase();
    STOP_SR.contains(&l.as_str()) || STOP_EN.contains(&l.as_str())
}

/// The meaningful words of a question, for the library search.
pub fn search_terms(question: &str) -> String {
    search_words(question).join(" ")
}

pub fn search_words(question: &str) -> Vec<String> {
    question
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 3 || w.chars().all(|c| c.is_ascii_digit()) && !w.is_empty())
        .filter(|w| !is_stop_word(w))
        .take(6)
        .map(str::to_string)
        .collect()
}

/// The words of a folded text.
pub(super) fn words(text: &str) -> Vec<&str> {
    text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect()
}

/// Whether a stem starts one of the words. A short stem must also be most of
/// the word, so "вод" (water) finds "воде" but not "водовод" or "производ".
pub(super) fn starts_word(words: &[&str], stem: &str) -> bool {
    let n = stem.chars().count();
    words.iter().any(|w| w.starts_with(stem) && (n >= 4 || w.chars().count() <= n + 3))
}

pub(super) fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max).collect();
    match cut.rfind(['.', '!', '?']) {
        Some(i) if i > max / 2 => cut[..=i].to_string(),
        _ => format!("{cut}…"),
    }
}

/// The paragraphs of an article as plain Latin text, without reference marks.
pub(super) fn paragraphs(html: &str) -> Vec<String> {
    let text = article_text(html, usize::MAX);
    text.lines().map(str::to_string).filter(|l| !l.is_empty()).collect()
}

/// Plain text of an article's paragraphs, in Latin script, up to `max` characters.
pub fn article_text(html: &str, max: usize) -> String {
    let lower = html.to_ascii_lowercase();
    let mut out = String::new();
    let mut out_chars = 0usize;
    let mut pos = 0;
    while out_chars < max {
        let Some(start) = lower[pos..].find("<p").map(|i| pos + i) else { break };
        // "<p>" or "<p ..." but not "<pre", "<param"...
        let after = lower.as_bytes().get(start + 2).copied();
        if !matches!(after, Some(b'>') | Some(b' ') | Some(b'\n') | Some(b'\t')) {
            pos = start + 2;
            continue;
        }
        let Some(open_end) = lower[start..].find('>').map(|i| start + i + 1) else { break };
        let Some(close) = lower[open_end..].find("</p>").map(|i| open_end + i) else { break };
        let para = strip_tags(&html[open_end..close]);
        let para = para.split_whitespace().collect::<Vec<_>>().join(" ");
        let n = para.chars().count();
        if n >= 20 {
            if !out.is_empty() {
                out.push('\n');
                out_chars += 1;
            }
            out.push_str(&para);
            out_chars += n;
        }
        pos = close + 4;
    }
    let out = zaklon_core::translit::cyrillic_to_latin(&out);
    // Wikipedia reference marks like [1] would be confused with our source numbers.
    let out = remove_ref_marks(&out);
    if out.chars().count() > max {
        let cut: String = out.chars().take(max).collect();
        match cut.rfind(['.', '!', '?']) {
            Some(i) if i > max / 2 => cut[..=i].to_string(),
            _ => format!("{cut}…"),
        }
    } else {
        out
    }
}

/// Plain text of a piece of HTML (tags off, entities decoded).
pub fn strip_html(s: &str) -> String {
    strip_tags(s)
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    decode_entities(&out)
}

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';').filter(|e| *e <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..end];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" | "#160" => Some(' '),
            _ if ent.starts_with("#x") || ent.starts_with("#X") => u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32),
            _ if ent.starts_with('#') => ent[1..].parse::<u32>().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn remove_ref_marks(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '[' {
            let mut inner = String::new();
            let mut closed = false;
            while let Some(&n) = chars.peek() {
                chars.next();
                if n == ']' {
                    closed = true;
                    break;
                }
                inner.push(n);
                if inner.len() > 12 {
                    break;
                }
            }
            let is_ref = closed && !inner.is_empty() && inner.chars().all(|c| c.is_ascii_digit() || c == ' ' || c.is_alphabetic() && inner.len() <= 6);
            if !is_ref {
                out.push('[');
                out.push_str(&inner);
                if closed {
                    out.push(']');
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_terms_keep_the_meaning() {
        assert_eq!(search_terms("Koliko dugo traje konzerva pasulja?"), "dugo traje konzerva pasulja");
        assert_eq!(search_terms("How long does canned food last?"), "long canned food last");
        assert_eq!(search_terms("Šta je?"), "");
    }

    #[test]
    fn stems_find_the_basic_form() {
        assert_eq!(stem("pasulja"), "pasulj");
        assert_eq!(stem("konzerva"), "konzerv");
        assert_eq!(stem("vodu"), "vod");
        assert_eq!(stem("sol"), "sol"); // too short to cut
        assert_eq!(stem("filtera"), "filter");
        assert_eq!(stem("beans"), "bean");
        assert_eq!(stem("water"), "water");
    }

    #[test]
    fn article_text_reads_paragraphs_in_latin() {
        let html = r#"<html><head><style>p{}</style></head><body><pre>code</pre>
<p class="x">Пасуљ је <b>махунарка</b> богата протеинима.<sup>[1]</sup> Чува се на сувом.</p>
<p>x</p><p>Second &amp; last paragraph with enough text in it.</p></body></html>"#;
        let t = article_text(html, 1000);
        assert_eq!(t, "Pasulj je mahunarka bogata proteinima. Čuva se na suvom.\nSecond & last paragraph with enough text in it.");
        let short = article_text(html, 40);
        assert!(short.chars().count() <= 41, "{short}");
    }
}
