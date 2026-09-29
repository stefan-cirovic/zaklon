//! What a question is about among the app's topics: the Library's topics
//! and the maps (`zaklon_core::catalog::TOPICS`).
//! Decided by the question's words, from a small table of words for each
//! topic, matched the way the rest of the assistant matches Serbian: plain
//! Latin without diacritics (so "фрижидер" and "frizider" are "frižider"),
//! and the beginning of a word for its forms ("sijalic*" is "sijalica",
//! "sijalice", "sijalicama"). No model is asked: the topics choose the packs
//! searched first and what is suggested under the answer (`suggest`), and
//! both must be reliable with the smallest model too.
//!
//! The pieces of a question (`tokens`) and the matching (`phrase_at`) are
//! shared with `suggest`, which reads amounts from the same pieces.

use super::plan::health_hits;
use super::text::plain;

/// A piece of a question.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Tok {
    /// A word in plain Latin, lower case, without diacritics.
    Word(String),
    Num(f64),
    /// Two numbers with an "x" between them: "3x1.2", "3 m x 1,2 m".
    Dims(f64, f64),
    /// A comma, a full stop, a question mark...: where one thing said ends.
    Sep,
}

/// The question as words, numbers ("1,5" is one and a half) and sizes
/// ("3x1.2"), with the places where one thing said ends.
pub(super) fn tokens(text: &str) -> Vec<Tok> {
    let c: Vec<char> = plain(text)
        .chars()
        .map(|c| match c {
            '×' => 'x',
            '²' => '2',
            c => c,
        })
        .collect();
    let mut out: Vec<Tok> = Vec::new();
    let mut i = 0;
    while i < c.len() {
        if c[i].is_ascii_digit() {
            let (a, next) = number(&c, i);
            match dims(&c, next) {
                Some((b, after)) => {
                    out.push(Tok::Dims(a, b));
                    i = after;
                }
                None => {
                    out.push(Tok::Num(a));
                    i = next;
                }
            }
        } else if c[i].is_alphabetic() {
            let start = i;
            while i < c.len() && c[i].is_alphanumeric() {
                i += 1;
            }
            out.push(Tok::Word(c[start..i].iter().collect()));
        } else {
            if matches!(c[i], ',' | ';' | '.' | '?' | '!' | ':' | '\n' | '(' | ')') && out.last().is_some_and(|t| *t != Tok::Sep) {
                out.push(Tok::Sep);
            }
            i += 1;
        }
    }
    out
}

/// The number at `i` ("3", "1.2", "1,5") and where it ends.
fn number(c: &[char], i: usize) -> (f64, usize) {
    let mut j = i;
    while j < c.len() && c[j].is_ascii_digit() {
        j += 1;
    }
    let mut s: String = c[i..j].iter().collect();
    if j + 1 < c.len() && matches!(c[j], '.' | ',') && c[j + 1].is_ascii_digit() {
        let mut k = j + 1;
        while k < c.len() && c[k].is_ascii_digit() {
            k += 1;
        }
        s.push('.');
        s.extend(&c[j + 1..k]);
        j = k;
    }
    (s.parse().unwrap_or(0.0), j)
}

/// After a number, "x1.2" (spaces and a unit "m" allowed around it): the
/// second number and where the size ends.
fn dims(c: &[char], i: usize) -> Option<(f64, usize)> {
    let skip = |mut i: usize| {
        while i < c.len() && c[i] == ' ' {
            i += 1;
        }
        i
    };
    // "m" on its own, not the start of a word ("3 mala").
    let metre = |i: usize| i < c.len() && c[i] == 'm' && !c.get(i + 1).is_some_and(|n| n.is_alphanumeric());
    let mut i = skip(i);
    if metre(i) {
        i = skip(i + 1);
    }
    if i >= c.len() || !matches!(c[i], 'x' | '*') {
        return None;
    }
    i = skip(i + 1);
    if i >= c.len() || !c[i].is_ascii_digit() {
        return None;
    }
    let (b, mut end) = number(c, i);
    let unit = skip(end);
    if metre(unit) {
        end = unit + 1;
    }
    Some((b, end))
}

/// A word and a key of the tables: "frizider*" is every word that begins
/// with "frizider", "tv" that word only.
pub(super) fn word_is(word: &str, key: &str) -> bool {
    match key.strip_suffix('*') {
        Some(start) => word.starts_with(start),
        None => word == key,
    }
}

/// How many words a phrase of keys ("kap po kap", "led sijalic*") takes
/// when it matches the words at `at`.
pub(super) fn phrase_at(toks: &[Tok], at: usize, phrase: &str) -> Option<usize> {
    let mut n = 0;
    for key in phrase.split(' ') {
        match toks.get(at + n) {
            Some(Tok::Word(w)) if word_is(w, key) => n += 1,
            _ => return None,
        }
    }
    Some(n)
}

/// Whether any of the phrases is in the question.
pub(super) fn has_any(toks: &[Tok], phrases: &[&str]) -> bool {
    phrases.iter().any(|p| (0..toks.len()).any(|i| phrase_at(toks, i, p).is_some()))
}

/// How many of the phrases are in the question.
fn hits(toks: &[Tok], phrases: &[&str]) -> usize {
    phrases.iter().filter(|p| (0..toks.len()).any(|i| phrase_at(toks, i, p).is_some())).count()
}

/// The words of each topic, in the order of `TOPICS`. Health also counts
/// the words that make a question about health for the safety rules
/// (`plan::health_hits`). A word here is enough for the topic, so a word
/// with another common meaning is left out or written as a phrase: "led"
/// is ice in Serbian, "vodi" also "leads", "karta" a ticket, "rat" an
/// animal in English, "route" the start of "router", "gde je" asks where
/// anything is.
const TOPIC_WORDS: &[(&str, &[&str])] = &[
    (
        "health",
        &[
            "zdravlj*", "bolnic*", "doktor*", "apotek*", "ambulant*", "health", "hospital*", "doctor*", "pharmac*", "ambulanc*", "medicin*",
        ],
    ),
    (
        "water",
        &[
            // Serbian
            "voda", "vode", "vodu", "vodom", "vodama", "pitk*", "vodovod*", "bunar*", "cistern*", "kisnic*", "rezervoar*", "precisc*", "precist*",
            "filtrir*", "prokuv*", "hlorisa*", "flasir*", "cesm*", "zaliv*", "navodnj*", "kap po kap", "kapaljk*",
            // English
            "water", "waters", "rainwater", "rain barrel*", "cistern*", "purif*", "drinking", "irrigat*", "drip", "watering", "chlorin*", "aquifer*",
        ],
    ),
    (
        "food",
        &[
            // Serbian
            "hran*", "namirnic*", "zimnic*", "konzerv*", "tegl*", "recept*", "kuva*", "jelo", "jela", "jelu", "obrok*", "brasn*", "hleb*", "pasulj*",
            "pirin*", "mlek*", "meso", "mesa", "mesom", "susen*", "dimljen*", "kiseljen*", "tursij*", "dzem*", "pekmez*", "kompot*", "fermentis*",
            "kvasac", "kvasca", "rok trajanja", "kalorij*", "secer*", "jaja", "jaje",
            // English
            "food*", "recipe*", "cook*", "canning", "canned", "preserv*", "pickl*", "ferment*", "jerky", "bread*", "flour", "rice", "beans", "meal*",
            "calori*", "pantry", "shelf life", "eat", "eating", "jars", "jam", "meat", "milk", "eggs", "sugar", "yeast", "sourdough",
        ],
    ),
    (
        "garden",
        &[
            // Serbian
            "bast*", "vrt", "vrta", "vrtu", "vrtlar*", "povrc*", "paradajz*", "krastav*", "paprik*", "sargarep*", "krompir*", "luk", "salat*",
            "spanac*", "blitv*", "rukol*", "kupus*", "tikvic*", "bundev*", "cvekl*", "rotkvic*", "persun*", "mirodij*", "bosiljak*", "biljk*",
            "bilje", "bilja", "jagod*", "malin*", "kupin*", "ribizl*", "borovnic*", "vocn*", "vock*", "voce", "voca", "sadn*", "sadim", "saditi",
            "posadi*", "presadi*", "sejanj*", "sejati", "posej*", "seme", "semena", "semenje", "rasad*", "leja", "leje", "leju", "lejama", "lejom",
            "gredic*", "djubr*", "kompost*", "malc*", "korov*", "stetocin*", "orezivanj*", "kalemlj*", "plastenik*", "staklenik*", "mikrozelen*",
            "klijanj*", "zemljist*", "berba", "berbu", "prinos*", "zaliv*", "navodnj*", "kap po kap", "kukuruz*", "boranij*", "grasak", "graska",
            "njiv*", "okopav*",
            // English
            "garden*", "vegetable*", "veggie*", "tomato*", "cucumber*", "pepper", "peppers", "carrot*", "potato*", "onion*", "garlic", "lettuce",
            "spinach", "kale", "cabbage*", "zucchini", "squash", "pumpkin*", "beet", "beets", "beetroot*", "radish*", "herbs", "strawberr*",
            "raspberr*", "blackberr*", "blueberr*", "currant*", "orchard*", "fruit tree*", "planting", "plants", "seedling*", "transplant*", "sowing",
            "seeds", "compost*", "fertiliz*", "fertilis*", "manure", "mulch*", "weeds", "weeding", "pests", "aphid*", "pruning", "graft*",
            "greenhouse*", "microgreen*", "sprout*", "soil", "raised bed*", "garden bed*", "harvest*", "irrigat*", "drip", "watering", "peas",
        ],
    ),
    (
        "power",
        &[
            // Serbian
            "struj*", "bateri*", "akumulator*", "solar*", "panel*", "invertor*", "inverter*", "agregat*", "generator*", "elektricn*", "elektrik*",
            "kilovat*", "vat", "vati", "kwh", "kw", "ah", "amper*", "volt*", "punjac*", "power bank*", "powerbank*", "ups", "lifepo4", "litijum*",
            "fotonapon*", "mppt", "kontroler* punjenj*",
            // English
            "power", "electric*", "outage*", "blackout*", "battery", "batteries", "inverter*", "watt*", "kilowatt*", "amp", "amps", "ampere*",
            "charger*", "charging", "lithium", "off grid", "offgrid", "photovoltaic", "pv",
        ],
    ),
    (
        "build",
        &[
            // Serbian
            "popravk*", "popravi*", "popravlj*", "ugradnj*", "ugradi*", "montaz*", "montir*", "instalacij*", "osigurac*", "cev", "cevi", "cevovod*",
            "slavin*", "curi", "curenj*", "zid", "zida", "zidu", "zidov*", "zidanj*", "krov*", "alat", "alata", "alatom", "alatk*", "busilic*",
            "sraf*", "zavari*", "zavarivanj*", "cement*", "beton*", "malter*", "izolacij*", "dimnjak*", "oluk*", "pumpa", "pumpe", "pumpu", "pumpom",
            "hidrofor*", "bojler*", "ventil*", "kvar", "kvara", "pokvari*", "gradnj*", "sagradi*", "izgradi*", "stolarij*", "kanalizacij*",
            "septick*", "vodoinstal*", "elektroinstal*", "kabl*", "zica", "zice", "zicu", "uticnic*", "prekidac*",
            // English
            "repair*", "fix", "fixing", "install*", "build", "building", "diy", "tools", "drill*", "pipe", "pipes", "plumb*", "leak*", "faucet*",
            "roof*", "wall", "walls", "fuse", "fuses", "breaker*", "wiring", "wires", "weld*", "concrete", "cement", "insulat*", "chimney*",
            "gutter*", "pump", "pumps", "valve*", "broken", "construct*", "outlet*", "socket*", "septic", "carpentr*", "woodwork*", "screw*",
        ],
    ),
    (
        "reference",
        &[
            // Serbian
            "istorij*", "ratu", "ratov*", "ratni*", "ustanak", "ustanka", "ustanku", "bitk*", "kralj*", "vek", "veka", "veku", "recnik*",
            "prevod*", "preved*", "prevesti", "znacenj*", "enciklopedij*", "vikipedij*", "wikipedij*", "knjig*", "pisac", "pisca", "ko je bio",
            "ko je bila", "ko su bili", "kada je bio", "kad je poceo", "kada je poceo", "glavni grad", "geografij*", "matematik*", "fizik*",
            "hemij*", "biologij*", "astronomij*", "planet*",
            // English
            "history", "historical", "war", "wars", "battle*", "king", "kings", "kingdom*", "century", "centuries", "dictionary", "translat*",
            "meaning of", "definition", "define", "encyclopedia*", "wikipedia", "book", "books", "author*", "who was", "who were", "when was",
            "when did", "capital of", "geograph*", "mathemat*", "physics", "chemistry", "biology", "astronom*", "planet*",
        ],
    ),
    (
        "maps",
        &[
            // Serbian
            "mapa", "mape", "mapu", "mapi", "mapama", "mapom", "geografsk* kart*", "topografsk* kart*", "navigacij*", "koordinat*", "kompas*", "gps",
            "put do", "puta do", "kako da stignem", "kako da dodjem", "gde se nalazi", "najbliz*", "adres*", "ruta", "rutu", "rute", "rutom",
            "lokacij*", "evakuacij*", "udaljenost*", "koliko je daleko",
            // English
            "map", "maps", "navigat*", "coordinate*", "compass*", "route", "routes", "directions", "nearest", "how do i get to", "how far",
            "location*", "evacuat*", "trail*", "hiking",
        ],
    ),
];

/// The topics of a text, the one with the most words first (in the order
/// of `TOPICS` when two have as many).
pub(super) fn topics_in(text: &str) -> Vec<&'static str> {
    let toks = tokens(text);
    let mut found: Vec<(usize, usize, &'static str)> = Vec::new();
    for (order, (topic, words)) in TOPIC_WORDS.iter().enumerate() {
        let mut n = hits(&toks, words);
        if *topic == "health" {
            n += health_hits(text);
        }
        if n > 0 {
            found.push((n, order, topic));
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    found.into_iter().map(|(_, _, t)| t).collect()
}

/// Numbers in words.
const NUMBER_WORDS: &[(&str, f64)] = &[
    ("jedan", 1.0), ("jedna", 1.0), ("jedno", 1.0), ("jednu", 1.0), ("jednog", 1.0), ("jednom", 1.0), ("jednoj", 1.0), ("dva", 2.0),
    ("dve", 2.0), ("par", 2.0), ("tri", 3.0), ("cetiri", 4.0), ("pet", 5.0), ("sest", 6.0), ("sedam", 7.0), ("osam", 8.0), ("devet", 9.0),
    ("deset", 10.0), ("dvanaest", 12.0), ("petnaest", 15.0), ("dvadeset", 20.0), ("trideset", 30.0), ("a", 1.0), ("an", 1.0), ("one", 1.0),
    ("two", 2.0), ("couple", 2.0), ("three", 3.0), ("four", 4.0), ("five", 5.0), ("six", 6.0), ("seven", 7.0), ("eight", 8.0), ("nine", 9.0),
    ("ten", 10.0), ("twelve", 12.0), ("fifteen", 15.0), ("twenty", 20.0), ("thirty", 30.0),
];

/// People counted as a group: "nas četvoro", "za dvoje".
pub(super) const COLLECTIVE: &[(&str, f64)] = &[
    ("dvoje", 2.0), ("troje", 3.0), ("cetvoro", 4.0), ("petoro", 5.0), ("sestoro", 6.0), ("sedmoro", 7.0), ("osmoro", 8.0), ("devetoro", 9.0),
    ("desetoro", 10.0),
];

/// A number, in figures or in words.
pub(super) fn amount(t: &Tok) -> Option<f64> {
    match t {
        Tok::Num(n) => Some(*n),
        Tok::Word(w) => NUMBER_WORDS.iter().chain(COLLECTIVE).find(|(k, _)| k == w).map(|(_, n)| *n),
        _ => None,
    }
}

/// Words of greetings and thanks: a message of only these is small talk.
const SMALL_TALK: &[&str] = &[
    "hvala", "puno", "mnogo", "ti", "vam", "thanks", "thank", "you", "ok", "okay", "okej", "u", "redu", "super", "odlicno", "sjajno", "zdravo",
    "cao", "hello", "hi", "hey", "hej", "dobro", "dobar", "jutro", "dan", "vece", "laku", "noc", "bye", "great", "nice", "cool", "perfect",
    "good", "morning", "evening", "night", "pozdrav", "e", "pa", "da", "ne", "yes", "no", "jasno", "razumem", "got", "it", "fine", "so", "much",
];

/// "Hvala!", "OK, thanks": nothing to look up or suggest.
pub(super) fn small_talk(toks: &[Tok]) -> bool {
    toks.iter().any(|t| matches!(t, Tok::Word(_)))
        && toks.iter().all(|t| match t {
            Tok::Word(w) => SMALL_TALK.contains(&w.as_str()),
            Tok::Sep => true,
            _ => false,
        })
}

/// How a question that goes on from the one before starts: "A za 5 dana?",
/// "And with two fridges?". "A" and "i" only with the word after them: in
/// English they are "a" and "I".
const FOLLOW_UP: &[&str] = &[
    "a za", "a sa", "a bez", "a ako", "a koliko", "a sta", "a kad", "a kada", "i za", "i sa", "i bez", "i ako", "i koliko", "and", "also",
    "jos", "what about", "how about", "what if", "sta ako", "sta je sa", "with", "without", "sa", "bez", "but", "ali", "same", "isto", "onda",
    "then",
];

/// A question that goes on from the one before: it starts like one, or it
/// is short and says a number ("5 dana?", "za četvoro").
pub(super) fn follow_up(toks: &[Tok]) -> bool {
    if FOLLOW_UP.iter().any(|p| phrase_at(toks, 0, p).is_some()) {
        return true;
    }
    let words = toks.iter().filter(|t| !matches!(t, Tok::Sep)).count();
    let article = |t: &Tok| matches!(t, Tok::Word(w) if w == "a" || w == "an");
    words <= 6 && toks.iter().any(|t| matches!(t, Tok::Dims(..)) || (amount(t).is_some() && !article(t)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn questions_are_cut_into_words_numbers_and_sizes() {
        let w = |s: &str| Tok::Word(s.into());
        assert_eq!(
            tokens("2 leje 3x1,2 m paradajz, i jedna 2 × 1m za salatu."),
            vec![Tok::Num(2.0), w("leje"), Tok::Dims(3.0, 1.2), w("paradajz"), Tok::Sep, w("i"), w("jedna"), Tok::Dims(2.0, 1.0), w("za"), w("salatu"), Tok::Sep]
        );
        assert_eq!(tokens("Фрижидер 1,5 дана"), vec![w("frizider"), Tok::Num(1.5), w("dana")]);
        assert_eq!(tokens("12V i 10m² i 72h"), vec![Tok::Num(12.0), w("v"), w("i"), Tok::Num(10.0), w("m2"), w("i"), Tok::Num(72.0), w("h")]);
        assert_eq!(tokens("3 x dnevno"), vec![Tok::Num(3.0), w("x"), w("dnevno")], "not a size");
        assert_eq!(tokens("3 m x 1.2 m malina"), vec![Tok::Dims(3.0, 1.2), w("malina")]);
        assert!(word_is("sijalice", "sijalic*") && word_is("tv", "tv") && !word_is("tvrdo", "tv"));
    }

    fn check(question: &str, want: &[&str]) {
        let got = topics_in(question);
        assert_eq!(got, want, "{question}");
    }

    #[test]
    fn questions_are_sorted_into_topics() {
        // Power
        check("koliko baterija mi treba za frižider 3 dana", &["power"]);
        check("Колико батерија ми треба за фрижидер 3 дана?", &["power"]);
        check("How big a battery do I need to run a fridge in a blackout?", &["power"]);
        check("koliko solarnih panela za vikendicu", &["power"]);
        check("which inverter for a 12V LiFePO4 battery", &["power"]);
        // Garden and water
        check("kap po kap za 10 m2 paradajza", &["garden", "water"]);
        check("kako da navodnjavam baštu leti", &["garden", "water"]);
        check("drip irrigation for tomatoes and cucumbers", &["garden", "water"]);
        check("kada se sadi krompir", &["garden"]);
        check("When should I plant garlic?", &["garden"]);
        // Water
        check("kako da prečistim vodu za piće", &["water"]);
        check("Како да пречистим воду из бунара?", &["water"]);
        check("How do I purify rainwater?", &["water"]);
        // Health
        check("šta da radim kod opekotine", &["health"]);
        check("шта да радим код опекотине", &["health"]);
        check("How do I treat a bee sting?", &["health"]);
        check("sta da radim ako dete ima temperaturu", &["health"]);
        // Food
        check("kako se pravi zimnica od paprike", &["food", "garden"]);
        check("How long does canned food last?", &["food"]);
        check("recept za hleb bez kvasca", &["food"]);
        // Build
        check("kako da zamenim osigurac u kuci, izbacuje mi struju", &["power", "build"]);
        check("curi mi slavina u kupatilu sta da radim", &["build"]);
        check("How do I fix a leaking roof?", &["build"]);
        // Encyclopedias and dictionaries
        check("kad je poceo prvi srpski ustanak i ko ga je vodio", &["reference"]);
        check("Who was Nikola Tesla?", &["reference"]);
        // Maps
        check("kako da stignem do najbliže bolnice", &["maps", "health"]);
        check("Where can I find a map of the hiking trails?", &["maps"]);
    }

    #[test]
    fn small_talk_and_other_questions_have_no_topic() {
        for q in ["Hvala!", "zdravo", "OK, thanks", "Ko si ti?", "What can you do?", "kako si danas", "Laku noć", "tell me a joke", "šta ima novo"] {
            assert!(topics_in(q).is_empty(), "{q}: {:?}", topics_in(q));
        }
        // Words with another meaning are not taken for a topic.
        for q in ["stavljam led na čelo?", "gde je moj telefon", "sta me vodi kroz zivot", "kupio sam kartu za voz"] {
            let t = topics_in(q);
            assert!(!t.contains(&"garden") && !t.contains(&"maps") && !t.contains(&"water"), "{q}: {t:?}");
        }
        assert!(small_talk(&tokens("Hvala puno!")));
        assert!(small_talk(&tokens("ok thanks")));
        assert!(!small_talk(&tokens("hvala, a za 5 dana?")));
        assert!(!small_talk(&tokens("")));
    }

    #[test]
    fn a_follow_up_is_recognised() {
        assert!(follow_up(&tokens("A za 5 dana?")));
        assert!(follow_up(&tokens("and with two fridges?")));
        assert!(follow_up(&tokens("za 7 dana")));
        assert!(follow_up(&tokens("za četvoro?")));
        assert!(follow_up(&tokens("What about a week?")));
        assert!(!follow_up(&tokens("Kako se čuva brašno?")));
        assert!(!follow_up(&tokens("Kako da zamenim osigurac u kuci kad izbacuje 3 puta dnevno")));
    }

    #[test]
    fn every_topic_is_one_of_the_apps() {
        let ours: Vec<&str> = TOPIC_WORDS.iter().map(|(t, _)| *t).collect();
        assert_eq!(ours, zaklon_core::catalog::TOPICS.to_vec(), "the same topics, in the same order");
        for (topic, words) in TOPIC_WORDS {
            for w in *words {
                assert!(w.split(' ').all(|k| !k.is_empty() && k.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '*')), "{topic}: {w}");
            }
        }
    }
}
