//! Serbian Latin <-> Cyrillic transliteration. Serbian has a one-to-one
//! mapping between the two scripts (with three digraphs: lj, nj, dž), so this
//! is exact for Serbian text. Used so that a search typed in Latin script also
//! finds Serbian content written in Cyrillic, and vice versa.

/// Latin letters (lower case) and their Cyrillic counterparts, digraphs first.
const LAT_TO_CYR: &[(&str, char)] = &[
    ("dž", 'џ'),
    ("lj", 'љ'),
    ("nj", 'њ'),
    ("a", 'а'),
    ("b", 'б'),
    ("c", 'ц'),
    ("č", 'ч'),
    ("ć", 'ћ'),
    ("d", 'д'),
    ("đ", 'ђ'),
    ("e", 'е'),
    ("f", 'ф'),
    ("g", 'г'),
    ("h", 'х'),
    ("i", 'и'),
    ("j", 'ј'),
    ("k", 'к'),
    ("l", 'л'),
    ("m", 'м'),
    ("n", 'н'),
    ("o", 'о'),
    ("p", 'п'),
    ("r", 'р'),
    ("s", 'с'),
    ("š", 'ш'),
    ("t", 'т'),
    ("u", 'у'),
    ("v", 'в'),
    ("z", 'з'),
    ("ž", 'ж'),
];

fn upper_first(c: char) -> char {
    c.to_uppercase().next().unwrap_or(c)
}

/// Words where "nj" or "dž" are two letters, not one (a prefix meets a root):
/// the word's beginning and where in it the pair starts. "odžak" (оџак) is a
/// real џ, so only whole known beginnings are listed.
const SPLIT_PAIRS: &[(&str, usize)] = &[
    ("injek", 1),
    ("injun", 1),
    ("konjug", 2),
    ("konjunk", 2),
    ("tanjug", 2),
    ("nadživ", 2),
    ("nadžnje", 2),
    ("odžali", 1),
    ("podžanr", 2),
    ("podžup", 2),
    ("predžel", 3),
];

/// Whether the pair starting at `i` must stay two letters.
fn split_pair(chars: &[char], i: usize) -> bool {
    let mut start = i;
    while start > 0 && chars[start - 1].is_alphabetic() {
        start -= 1;
    }
    let word: String = chars[start..].iter().take_while(|c| c.is_alphabetic()).flat_map(|c| c.to_lowercase()).collect();
    SPLIT_PAIRS.iter().any(|(prefix, at)| i - start == *at && word.starts_with(prefix))
}

/// Convert Serbian Latin text to Cyrillic. Characters without a Serbian
/// counterpart (q, w, x, y, digits, punctuation) are kept as they are.
/// "dj" is treated as "đ", the common way to type it without diacritics.
pub fn latin_to_cyrillic(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len() * 2);
    let mut i = 0;
    'outer: while i < chars.len() {
        let c = chars[i];
        let lower_c: String = c.to_lowercase().collect();
        let is_upper = c.is_uppercase();

        // Two-letter sequences: dž, lj, nj, and dj as đ.
        if i + 1 < chars.len() {
            let pair: String = [c, chars[i + 1]].iter().flat_map(|ch| ch.to_lowercase()).collect();
            let digraph = match pair.as_str() {
                "dž" => Some('џ'),
                "lj" => Some('љ'),
                "nj" => Some('њ'),
                "dj" => Some('ђ'),
                _ => None,
            };
            if let Some(cyr) = digraph.filter(|_| !split_pair(&chars, i)) {
                out.push(if is_upper { upper_first(cyr) } else { cyr });
                i += 2;
                continue 'outer;
            }
        }

        for (lat, cyr) in LAT_TO_CYR.iter().skip(3) {
            if *lat == lower_c {
                out.push(if is_upper { upper_first(*cyr) } else { *cyr });
                i += 1;
                continue 'outer;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Letters of other Cyrillic alphabets that appear in Serbian texts (Russian,
/// Macedonian, Ukrainian names), written the usual Serbian way.
const EXTRA_CYR: &[(char, &str)] = &[
    ('я', "ja"),
    ('ю', "ju"),
    ('ё', "jo"),
    ('й', "j"),
    ('ы', "y"),
    ('э', "e"),
    ('щ', "šč"),
    ('ъ', ""),
    ('ь', ""),
    ('ѓ', "ǵ"),
    ('ќ', "ḱ"),
    ('ѕ', "dz"),
    ('і', "i"),
    ('ї', "ji"),
    ('є', "je"),
    ('ґ', "g"),
];

fn latin_for(lower: char) -> Option<&'static str> {
    LAT_TO_CYR
        .iter()
        .find(|(_, cyr)| *cyr == lower)
        .map(|(lat, _)| *lat)
        .or_else(|| EXTRA_CYR.iter().find(|(c, _)| *c == lower).map(|(_, l)| *l))
}

/// Convert Serbian Cyrillic text to Latin. A capital letter that becomes two
/// Latin letters (Љ, Њ, Џ) is written "Lj" at the start of a word and "LJ"
/// inside a word written in capitals ("ЉУБАВ" -> "LJUBAV").
pub fn cyrillic_to_latin(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len() + input.len() / 4);
    for (i, &c) in chars.iter().enumerate() {
        let lower = c.to_lowercase().next().unwrap_or(c);
        let Some(lat) = latin_for(lower) else {
            out.push(c);
            continue;
        };
        if !c.is_uppercase() {
            out.push_str(lat);
            continue;
        }
        let next_upper = chars.get(i + 1).is_some_and(|n| n.is_uppercase());
        let prev_upper = i > 0 && chars[i - 1].is_uppercase();
        let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
        if next_upper || (prev_upper && !next_lower) {
            out.push_str(&lat.to_uppercase());
        } else {
            let mut it = lat.chars();
            if let Some(first) = it.next() {
                out.extend(first.to_uppercase());
                out.extend(it);
            }
        }
    }
    out
}

/// True when the text contains Cyrillic letters.
pub fn has_cyrillic(input: &str) -> bool {
    input.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c))
}

/// True when the text contains at least one Latin letter that maps to Cyrillic.
pub fn has_serbian_latin(input: &str) -> bool {
    input.chars().any(|c| {
        let l: String = c.to_lowercase().collect();
        LAT_TO_CYR.iter().any(|(lat, _)| *lat == l)
    })
}

/// Cyrillic spellings a Latin query may stand for when it was typed without
/// diacritics ("secer" for "šećer"): c can be ц/ч/ћ, s can be с/ш, z can be з/ж.
/// The plain transliteration comes first. At most `max` candidates are
/// returned; queries with many ambiguous letters only expand the first few.
pub fn cyrillic_candidates(input: &str, max: usize) -> Vec<String> {
    let base = latin_to_cyrillic(input);
    let chars: Vec<char> = base.chars().collect();
    let options = |c: char| -> Option<&'static [char]> {
        match c {
            'ц' => Some(&['ц', 'ч', 'ћ']),
            'Ц' => Some(&['Ц', 'Ч', 'Ћ']),
            'с' => Some(&['с', 'ш']),
            'С' => Some(&['С', 'Ш']),
            'з' => Some(&['з', 'ж']),
            'З' => Some(&['З', 'Ж']),
            _ => None,
        }
    };
    let mut out: Vec<String> = vec![base.clone()];
    let mut frontier: Vec<Vec<char>> = vec![chars.clone()];
    for (i, c) in chars.iter().enumerate() {
        let Some(opts) = options(*c) else { continue };
        let mut next = Vec::new();
        for f in &frontier {
            for o in opts {
                let mut v = f.clone();
                v[i] = *o;
                next.push(v);
            }
        }
        if next.len() > max * 4 {
            break;
        }
        frontier = next;
    }
    for v in frontier {
        let s: String = v.into_iter().collect();
        if !out.contains(&s) {
            out.push(s);
        }
        if out.len() >= max {
            break;
        }
    }
    out
}

/// Lower case, without accent marks, Latin turned into Cyrillic: a form in
/// which "Во̀да", "voda" and "вода" all compare equal.
pub fn fold(input: &str) -> String {
    let no_marks: String = input
        .chars()
        .filter(|c| !('\u{0300}'..='\u{036F}').contains(c))
        .collect();
    latin_to_cyrillic(&no_marks.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_pairs_stay_two_letters() {
        assert_eq!(latin_to_cyrillic("injekcija"), "инјекција");
        assert_eq!(latin_to_cyrillic("Konjugacija"), "Конјугација");
        assert_eq!(latin_to_cyrillic("nadživeti"), "надживети");
        assert_eq!(latin_to_cyrillic("odžaliti"), "оджалити");
        assert_eq!(latin_to_cyrillic("odžak"), "оџак", "a real џ");
        assert_eq!(latin_to_cyrillic("konj"), "коњ");
        assert_eq!(latin_to_cyrillic("Tanjug"), "Танјуг");
    }

    #[test]
    fn latin_to_cyrillic_words() {
        assert_eq!(latin_to_cyrillic("voda"), "вода");
        assert_eq!(latin_to_cyrillic("Ljubav i njiva"), "Љубав и њива");
        assert_eq!(latin_to_cyrillic("džep"), "џеп");
        assert_eq!(latin_to_cyrillic("Đorđe"), "Ђорђе");
        assert_eq!(latin_to_cyrillic("Djordje"), "Ђорђе");
        assert_eq!(latin_to_cyrillic("čaša šećera žuta"), "чаша шећера жута");
        assert_eq!(latin_to_cyrillic("prva pomoć 112"), "прва помоћ 112");
        assert_eq!(latin_to_cyrillic("LJUBAV"), "ЉУБАВ");
    }

    #[test]
    fn cyrillic_to_latin_words() {
        assert_eq!(cyrillic_to_latin("вода"), "voda");
        assert_eq!(cyrillic_to_latin("Љубав"), "Ljubav");
        assert_eq!(cyrillic_to_latin("Ђорђе и џеп"), "Đorđe i džep");
        assert_eq!(cyrillic_to_latin("Београд 2026"), "Beograd 2026");
    }

    #[test]
    fn capitals_and_foreign_letters() {
        assert_eq!(cyrillic_to_latin("ЉУБАВ"), "LJUBAV");
        assert_eq!(cyrillic_to_latin("Љубав и ЊЕГОШ"), "Ljubav i NJEGOŠ");
        assert_eq!(cyrillic_to_latin("УНИЦЕФ"), "UNICEF");
        assert_eq!(cyrillic_to_latin("Достоевский"), "Dostoevskij");
        assert_eq!(cyrillic_to_latin("Юрий Гагарин"), "Jurij Gagarin");
        assert!(has_cyrillic("вода 1"));
        assert!(!has_cyrillic("voda 1"));
    }

    #[test]
    fn roundtrip() {
        for w in ["zaklon", "šećer", "njiva", "ljubav", "džem", "Srbija"] {
            assert_eq!(cyrillic_to_latin(&latin_to_cyrillic(w)), w);
        }
    }

    #[test]
    fn candidates_cover_missing_diacritics() {
        let c = cyrillic_candidates("secer", 12);
        assert_eq!(c[0], "сецер");
        assert!(c.contains(&"шећер".to_string()));
        assert!(cyrillic_candidates("prva pomoc", 12).contains(&"прва помоћ".to_string()));
        assert!(cyrillic_candidates("zaba", 12).contains(&"жаба".to_string()));
        assert!(cyrillic_candidates("cvece", 12).contains(&"цвеће".to_string()));
        assert!(cyrillic_candidates("sasasasasasasasa", 12).len() <= 12);
    }

    #[test]
    fn folding() {
        assert_eq!(fold("Во̀да"), "вода");
        assert_eq!(fold("Voda"), "вода");
    }

    #[test]
    fn detects_latin() {
        assert!(has_serbian_latin("voda"));
        assert!(!has_serbian_latin("вода 12"));
    }
}
