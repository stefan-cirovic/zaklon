//! What of each source the model gets: the parts of an article about the
//! question, shortened to what this computer reads in time, and without
//! first aid that current guidance calls harmful.

use super::finish::split_sentences;
use super::sources::{prepare_terms, Passage};
use super::text::{clip, paragraphs, plain, search_words, starts_word, stem, words};

/// First aid that current guidance calls harmful but older encyclopedia text
/// still gives: tying off, cutting or sucking a snakebite, and ice, butter,
/// oil or toothpaste on a burn. For a health question, source sentences that
/// recommend it are left out before the model sees them; a sentence that
/// says not to do it stays.
pub(super) fn without_harmful_advice(text: &str, question: &str) -> String {
    const SNAKE: &[&str] = &["zmij", "ujed", "ujel", "ujeo", "otrovnic", "poskok", "snake", "bite", "bitten", "venom"];
    const SNAKE_HARM: &[&str] = &["podvez", "isisa", "usisa", "zasec", "zasek", "tourniquet", "suck", "incision", "cutting"];
    const BURN: &[&str] = &["opekot", "opeklin", "opece", "opekao", "opekla", "burn"];
    const BURN_HARM: &[&str] = &[
        "led", "leda", "ledom", "ledu", "ice", "puter", "putera", "puterom", "maslac", "maslacem", "ulje", "ulja", "uljem", "butter", "oil", "zubnu",
        "zubna", "zubnom", "toothpaste",
    ];
    const NOT: &[&str] = &["ne", "nemoj", "nemojte", "nikad", "nikada", "nikako", "nije", "nisu", "not", "never", "avoid", "don", "doesn", "shouldn", "no"];
    let has = |w: &[&str], list: &[&str], prefix: bool| list.iter().any(|x| w.iter().any(|y| if prefix { y.starts_with(x) } else { y == x }));
    let q = plain(question);
    let qw = words(&q);
    let (snake_question, burn_question) = (has(&qw, SNAKE, true), has(&qw, BURN, true));
    let mut lines = Vec::new();
    for line in text.lines() {
        let kept: Vec<String> = split_sentences(line)
            .into_iter()
            .filter(|s| {
                let p = plain(s);
                let w = words(&p);
                if has(&w, NOT, false) {
                    return true;
                }
                let snake = (snake_question || has(&w, SNAKE, true)) && has(&w, SNAKE_HARM, true);
                let burn = (burn_question || has(&w, BURN, true)) && has(&w, BURN_HARM, false);
                !(snake || burn)
            })
            .collect();
        if !kept.is_empty() {
            lines.push(kept.join(" "));
        }
    }
    lines.join("\n")
}

/// Stems that rank the paragraphs of a source when it is shortened, with
/// their weight: the search terms' count twice as much as the question's
/// other words.
pub(super) fn trim_stems(terms: &[String], terms_en: &[String], question: &str) -> Vec<(String, u32)> {
    let mut out: Vec<(String, u32)> = Vec::new();
    let mut add = |s: String, weight: u32| {
        if s.chars().count() >= 3 && !out.iter().any(|(x, _)| *x == s) {
            out.push((s, weight));
        }
    };
    for t in prepare_terms(terms).into_iter().chain(prepare_terms(terms_en)) {
        t.stems.into_iter().for_each(|s| add(s, 2));
    }
    for w in search_words(question) {
        add(zaklon_core::translit::fold(&stem(&w)), 1);
    }
    out
}

/// The sentences of a line of source text. One ends at ".", "!" or "?"
/// followed by a space and a capital letter, a digit or an opening quote,
/// so "(lat. combustio)" and "npr. hladnom vodom" stay in one piece.
fn line_sentences(line: &str) -> Vec<&str> {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let mut out = Vec::new();
    let mut start = 0;
    for (k, &(i, c)) in chars.iter().enumerate() {
        if !matches!(c, '.' | '!' | '?') || !chars.get(k + 1).is_some_and(|(_, n)| n.is_whitespace()) {
            continue;
        }
        let Some(&(_, next)) = chars[k + 1..].iter().find(|(_, n)| !n.is_whitespace()) else { continue };
        if next.is_uppercase() || next.is_ascii_digit() || matches!(next, '„' | '"' | '“' | '«' | '(') {
            let end = i + c.len_utf8();
            let s = line[start..end].trim();
            if !s.is_empty() {
                out.push(s);
            }
            start = end;
        }
    }
    let s = line[start..].trim();
    if !s.is_empty() {
        out.push(s);
    }
    out
}

/// How much a piece of source text names of the search words.
fn weight(text: &str, loose_stems: &[(String, u32)]) -> u32 {
    let folded = zaklon_core::translit::fold_loose(text);
    let w = words(&folded);
    loose_stems.iter().filter(|(st, _)| starts_word(&w, st)).map(|(_, weight)| weight).sum()
}

/// A source's paragraphs in the order they are worth keeping: the first
/// (what the article is about; in an encyclopedia often a summary of all of
/// it), then those that name the most of the search words, then the rest,
/// in the order of the article on a tie.
fn ranked_paragraphs(paragraphs: &[&str], stems: &[(String, u32)]) -> Vec<usize> {
    let loose: Vec<(String, u32)> = stems.iter().map(|(s, w)| (zaklon_core::translit::loosen(s), *w)).collect();
    let weights: Vec<u32> = paragraphs.iter().map(|p| weight(p, &loose)).collect();
    let mut rest: Vec<usize> = (1..paragraphs.len()).collect();
    rest.sort_by(|a, b| weights[*b].cmp(&weights[*a]).then(a.cmp(b)));
    (0..paragraphs.len().min(1)).chain(rest).collect()
}

/// The first whole sentences of a paragraph that fit in `max` characters
/// ("" when not even the first does).
fn first_sentences(paragraph: &str, max: usize) -> String {
    let mut out = String::new();
    let mut len = 0;
    for s in line_sentences(paragraph) {
        let add = s.chars().count() + usize::from(len > 0);
        if len + add > max {
            break;
        }
        if len > 0 {
            out.push(' ');
        }
        out.push_str(s);
        len += add;
    }
    out
}

/// Room left in a source worth the first sentences of a paragraph.
const MIN_PIECE: usize = 120;

/// What a source says about the question in at most `max` characters:
/// whole paragraphs in the order of `ranked_paragraphs`, as many as fit, and
/// where one does not fit but there is room, its first sentences; kept in
/// the order of the article. Text stays in runs: sentences picked here and
/// there by their words lose what they refer to (a list of symptoms rarely
/// names the illness again), and the model then finds nothing to answer with.
fn trim_passage(text: &str, stems: &[(String, u32)], max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let paragraphs: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let mut kept: Vec<(usize, String)> = Vec::new();
    let mut len = 0;
    for i in ranked_paragraphs(&paragraphs, stems) {
        let gap = usize::from(!kept.is_empty());
        let room = max.saturating_sub(len + gap);
        let whole = paragraphs[i].chars().count();
        let piece = if whole <= room {
            paragraphs[i].to_string()
        } else if room >= MIN_PIECE {
            first_sentences(paragraphs[i], room)
        } else {
            continue;
        };
        if !piece.is_empty() {
            len += gap + piece.chars().count();
            kept.push((i, piece));
        }
    }
    if kept.is_empty() {
        // Not even the first sentence fits.
        return clip(paragraphs.first().copied().unwrap_or(text), max);
    }
    kept.sort_by_key(|(i, _)| *i);
    kept.into_iter().map(|(_, t)| t).collect::<Vec<_>>().join("\n")
}

/// Shares of the source text by a source's place (best first): the best
/// found is most often the one that answers, and a third source rarely
/// adds to the first two.
const SHARES: [usize; 3] = [3, 2, 1];

/// Shorten the sources to `budget` characters together, each to what it
/// says about the question (see `trim_passage`). The best source gets the
/// largest share (see `SHARES`), and what a short one does not need goes to
/// the others.
pub(super) fn trim_sources(passages: &mut [Passage], stems: &[(String, u32)], budget: usize) {
    let lens: Vec<usize> = passages.iter().map(|p| p.text.chars().count()).collect();
    if lens.iter().sum::<usize>() <= budget {
        return;
    }
    let weights: Vec<usize> = (0..passages.len()).map(|i| SHARES.get(i).copied().unwrap_or(1)).collect();
    for (p, max) in passages.iter_mut().zip(share(budget, &lens, &weights)) {
        p.text = trim_passage(&p.text, stems, max);
    }
}

/// `budget` shared among `wants` in proportion to `weights`, as far as each
/// wants it: what one does not need goes to the others.
fn share(budget: usize, wants: &[usize], weights: &[usize]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..wants.len()).collect();
    // Those that want the least for their weight are settled first.
    order.sort_by(|&a, &b| (wants[a] * weights[b]).cmp(&(wants[b] * weights[a])));
    let mut out = vec![0; wants.len()];
    let mut left = budget;
    let mut weight_left: usize = weights.iter().sum();
    for &i in &order {
        out[i] = wants[i].min(left * weights[i] / weight_left.max(1));
        left -= out[i];
        weight_left -= weights[i];
    }
    out
}

/// The parts of an article that matter for the search words: the first
/// paragraph (what the thing is), then the paragraphs that mention the words
/// most, in article order, up to `max` characters, in Latin script. Words
/// are matched loosely and by their beginnings: "vodu" typed without
/// diacritics finds "vode", but "вод" does not find "производ".
pub fn relevant_text(html: &str, folded_stems: &[String], max: usize) -> String {
    let paras = paragraphs(html);
    if paras.is_empty() {
        return String::new();
    }
    let stems: Vec<String> = folded_stems.iter().map(|s| zaklon_core::translit::loosen(s)).collect();
    let hits = |p: &str| {
        let f = zaklon_core::translit::fold_loose(p);
        let w = words(&f);
        stems.iter().filter(|st| starts_word(&w, st)).count()
    };
    let mut ranked: Vec<(usize, usize)> = paras.iter().enumerate().skip(1).map(|(i, p)| (hits(p), i)).filter(|(h, _)| *h > 0).collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut chosen = vec![0usize];
    let mut len = paras[0].chars().count();
    for (_, i) in ranked {
        let l = paras[i].chars().count();
        if len + l > max && chosen.len() > 1 {
            continue;
        }
        chosen.push(i);
        len += l;
        if len >= max {
            break;
        }
    }
    chosen.sort_unstable();
    let joined = chosen.iter().map(|i| paras[*i].as_str()).collect::<Vec<_>>().join("\n");
    clip(&joined, max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::test_util::{passage, strings};

    #[test]
    fn relevant_paragraphs_are_chosen() {
        let html = "<p>Pasulj je biljka iz porodice mahunarki.</p><p>Istorija uzgoja pasulja u Americi je duga i zanimljiva.</p>\
<p>Suvi pasulj se čuva godinama na suvom i tamnom mestu, a kuvan u frižideru nekoliko dana.</p><p>Poznate sorte su tetovac i gradištanac.</p>";
        let stems = vec![zaklon_core::translit::fold("čuva"), zaklon_core::translit::fold("suv")];
        let t = relevant_text(html, &stems, 140);
        assert!(t.starts_with("Pasulj je biljka"), "{t}");
        assert!(t.contains("čuva godinama"), "{t}");
        assert!(!t.contains("Istorija"), "{t}");
    }

    #[test]
    fn sentences_end_before_a_capital_letter() {
        assert_eq!(
            line_sentences("Opekotina (lat. combustio) je povreda kože. Ohladi je, npr. mlakom vodom. Ustanak je počeo 1804. Vodio ga je Karađorđe!"),
            vec!["Opekotina (lat. combustio) je povreda kože.", "Ohladi je, npr. mlakom vodom.", "Ustanak je počeo 1804.", "Vodio ga je Karađorđe!"]
        );
        assert_eq!(line_sentences("Počeo je 15. februara 1804. godine u Orašcu"), vec!["Počeo je 15. februara 1804. godine u Orašcu"]);
        assert_eq!(line_sentences("Kraj. „Citat“ ide dalje. (Zagrada) kraj."), vec!["Kraj.", "„Citat“ ide dalje.", "(Zagrada) kraj."]);
        assert!(line_sentences("  ").is_empty());
    }

    fn burn_stems() -> Vec<(String, u32)> {
        trim_stems(&strings(&["lečenje opekotina", "opekotina"]), &strings(&["burn treatment"]), "sta da radim kad se neko opece, jel stavljam led?")
    }

    /// Paragraphs of 78, 74, 128 and 60 characters.
    const BURN: &str = "Opekotina je povreda kože nastala dejstvom toplote. Deli se na četiri stepena.\n\
Istorija medicine seže do starog Egipta, gde su lekari pisali na papirusu.\n\
Opekotinu treba odmah hladiti mlakom vodom desetak minuta. Posle toga se pokrije čistom gazom. Ne stavljaj led direktno na kožu.\n\
Crveni krst drži kurseve prve pomoći u svim većim gradovima.";

    #[test]
    fn a_source_keeps_what_it_says_about_the_question() {
        let stems = burn_stems();
        assert!(stems.contains(&(zaklon_core::translit::fold("opekotin"), 2)), "{stems:?}");
        assert!(stems.contains(&(zaklon_core::translit::fold("led"), 1)), "the question's own words count too: {stems:?}");
        let paragraphs: Vec<&str> = BURN.lines().collect();
        assert_eq!(ranked_paragraphs(&paragraphs, &stems), vec![0, 2, 1, 3], "the first, then what names the question's words");
        assert_eq!(trim_passage(BURN, &stems, 10_000), BURN, "enough room: nothing changes");
        // Room for two paragraphs: the first and the one about the question, in the article's order.
        let t = trim_passage(BURN, &stems, 220);
        assert_eq!(t, format!("{}\n{}", paragraphs[0], paragraphs[2]));
        // A paragraph that does not fit whole gives its first sentences.
        let t = trim_passage(BURN, &stems, 205);
        assert_eq!(t, format!("{}\nOpekotinu treba odmah hladiti mlakom vodom desetak minuta. Posle toga se pokrije čistom gazom.", paragraphs[0]));
        for max in [30, 60, 150, 205, 220, 300] {
            assert!(trim_passage(BURN, &stems, max).chars().count() <= max + 1, "{max}");
        }
        // Too little room for even the first sentence: it is cut.
        assert!(trim_passage(BURN, &stems, 30).starts_with("Opekotina je povreda kože"));
    }

    #[test]
    fn sources_share_the_budget() {
        let stems = burn_stems();
        let long = format!("Kuhinja je prostorija u kući. {}", ["Ovo je rečenica o nečem sasvim drugom."; 20].join(" "));
        let mut ps = vec![passage(1, "Opekotina", BURN), passage(2, "Kuhinja", &long), passage(3, "Opekotina 2", BURN)];
        let before: usize = ps.iter().map(|p| p.text.chars().count()).sum();
        trim_sources(&mut ps, &stems, 10_000);
        assert_eq!(ps.iter().map(|p| p.text.chars().count()).sum::<usize>(), before, "they fit: nothing changes");
        trim_sources(&mut ps, &stems, 900);
        let lens: Vec<usize> = ps.iter().map(|p| p.text.chars().count()).collect();
        // Shares of 3, 2 and 1: 450 for the best (it needs only 343), and of
        // the 557 left, two thirds for the second and one third for the third.
        assert_eq!(ps[0].text, BURN, "the best source is whole: {lens:?}");
        assert!(lens[1] <= 372 && lens[2] <= 185 && lens.iter().sum::<usize>() <= 900, "{lens:?}");
        assert!(ps[1].text.starts_with("Kuhinja je prostorija u kući. Ovo je"), "whole sentences from the start: {}", ps[1].text);
        assert!(ps[2].text.starts_with("Opekotina je povreda kože"), "{}", ps[2].text);
        // A short source leaves its room to the others.
        let mut ps = vec![passage(1, "Kratko", "Kratak tekst o opekotinama."), passage(2, "Kuhinja", &long)];
        trim_sources(&mut ps, &stems, 600);
        assert_eq!(ps[0].text, "Kratak tekst o opekotinama.");
        assert!(ps[1].text.chars().count() > 500, "{}", ps[1].text.chars().count());
    }

    #[test]
    fn budgets_are_shared_by_weight_as_far_as_wanted() {
        assert_eq!(share(900, &[100, 1000, 1000], &[1, 1, 1]), vec![100, 400, 400]);
        assert_eq!(share(900, &[1000, 1000, 1000], &[1, 1, 1]), vec![300, 300, 300]);
        assert_eq!(share(900, &[100, 100], &[1, 1]), vec![100, 100]);
        assert_eq!(share(2400, &[1400, 1400, 1400], &[3, 2, 1]), vec![1200, 800, 400]);
        assert_eq!(share(2400, &[300, 1400, 1400], &[3, 2, 1]), vec![300, 1400, 700], "what the best does not need goes on");
        assert_eq!(share(10, &[], &[]), Vec::<usize>::new());
    }

    #[test]
    fn short_stems_match_word_beginnings_only() {
        let w = ["производ", "воде", "водовод"];
        assert!(!starts_word(&w[..1], "вод"), "water is not inside 'product'");
        assert!(starts_word(&w[1..2], "вод"));
        assert!(!starts_word(&w[2..], "вод"), "a short stem must be most of the word");
        assert!(starts_word(&["производња"], "производ"), "longer stems may start longer words");
        let html = "<p>Voda je tečnost bez boje i mirisa.</p><p>Proizvodnja i uvoz robe su porasli ove godine.</p>\
<p>Vodu za piće treba prokuvati najmanje jedan minut.</p>";
        let t = relevant_text(html, &[zaklon_core::translit::fold(&stem("vodu")), zaklon_core::translit::fold("pice")], 90);
        assert!(t.contains("prokuvati"), "{t}");
        assert!(!t.contains("Proizvodnja"), "{t}");
    }

    #[test]
    fn outdated_first_aid_is_left_out_of_the_sources() {
        let zmije = "Zmije su gmizavci. Ako se ipak desi da zmija nekoga ugrize, prva pomoć se sastoji od podvezivanja ujedenog mesta, \
isisavanja otrova i hitnog transporta do lekara. Isisavanje otrova je veoma korisno.";
        let t = without_harmful_advice(zmije, "sta ako te ujede zmija");
        assert_eq!(t, "Zmije su gmizavci.");
        let en = "Trying to suck out the venom, cutting the wound with a knife, or using a tourniquet is not recommended. Keep the person calm.";
        assert_eq!(without_harmful_advice(en, "snakebite first aid"), en, "advice against it stays");
        let burn = "Opekotinu treba ohladiti mlakom vodom. Na opekotinu stavite led ili puter.";
        assert_eq!(without_harmful_advice(burn, "sta da radim kad se opecem"), "Opekotinu treba ohladiti mlakom vodom.");
        let bleeding = "Kod jakog krvarenja iz ruke primenjuje se podvezivanje.";
        assert_eq!(without_harmful_advice(bleeding, "kako se zaustavlja krvarenje"), bleeding, "a tourniquet is right for heavy bleeding");
    }
}
