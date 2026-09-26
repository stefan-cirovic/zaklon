//! Which language a short text (a question) is written in.

/// "sr" or "en" when the text clearly is one of them.
pub fn question_language(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    if lower.chars().any(|c| matches!(c, 'č' | 'ć' | 'ž' | 'š' | 'đ') || ('\u{0400}'..='\u{04FF}').contains(&c)) {
        return Some("sr");
    }
    const SR: &[&str] = &[
        "je", "da", "li", "koliko", "kako", "sta", "gde", "zasto", "koji", "koja", "koje", "sam", "se", "za", "od", "na", "u", "i",
        "treba", "moze", "mogu", "ima", "nema", "traje", "dugo", "kada", "kad", "sto", "ili", "ne", "mi", "ti", "hleb", "voda",
    ];
    const EN: &[&str] = &[
        "the", "is", "are", "how", "what", "does", "do", "can", "why", "where", "which", "of", "to", "in", "and", "a", "an", "it",
        "long", "last", "should", "i", "my", "you", "when", "much", "many", "for", "with", "water", "food",
    ];
    let (mut sr, mut en) = (0, 0);
    for w in lower.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()) {
        if SR.contains(&w) {
            sr += 1;
        }
        if EN.contains(&w) {
            en += 1;
        }
    }
    match sr.cmp(&en) {
        std::cmp::Ordering::Greater => Some("sr"),
        std::cmp::Ordering::Less => Some("en"),
        std::cmp::Ordering::Equal => None,
    }
}

#[cfg(test)]
mod tests {
    use super::question_language;

    #[test]
    fn guesses_the_language_of_a_question() {
        assert_eq!(question_language("How long does canned food last?"), Some("en"));
        assert_eq!(question_language("Koliko dugo traje konzerva pasulja?"), Some("sr"));
        assert_eq!(question_language("koliko traje hleb"), Some("sr"));
        assert_eq!(question_language("Šta da radim"), Some("sr"));
        assert_eq!(question_language("Колико траје хлеб?"), Some("sr"));
        assert_eq!(question_language("What is the best way to store water?"), Some("en"));
        assert_eq!(question_language("pasulj"), None);
    }
}
