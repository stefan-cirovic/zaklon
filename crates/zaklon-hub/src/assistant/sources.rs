//! Finding sources in the library: a few planned searches in each language
//! within a time limit, results ranked by their titles, and the best read
//! and ranked again by how much of the question they cover. Also online
//! research.

use std::future::Future;
use std::time::Duration;

use futures_util::StreamExt;

use super::passages::{relevant_text, without_harmful_advice};
use super::text::{is_stop_word, plain, search_words, starts_word, stem, words};
use super::{Assistant, Source};
use crate::kiwix::{Book, Found, Library, SearchResult};

/// Characters of each source passage read from an article.
pub(super) const SOURCE_CHARS: usize = 1400;
pub(super) const MAX_SOURCES: usize = 3;
/// Articles read before the final ranking, best first. Reading one is cheap
/// next to a search (a few hundredths of a second on a quiet disk), and the
/// title ranking alone lets a word's lookalike ("Можданице" for "moždani
/// udar") come first, so a few more are read than are used.
const FETCH: usize = 6;
/// The library part of a question: searches may take `LOOKUP_TIME`, searches
/// and reading `LIBRARY_TIME`; what is done by then is used and the rest is
/// never asked. A search takes a tenth of a second on a quiet hard disk and
/// up to half a minute when another program keeps the disk busy.
const LOOKUP_TIME: Duration = Duration::from_secs(12);
const LIBRARY_TIME: Duration = Duration::from_secs(20);
/// Library requests in flight at a time: kiwix-serve works on four at once
/// and a hard disk reads one place at a time.
const PARALLEL: usize = 2;
/// Full-text searches per language, words whose titles are looked up per
/// language, and title lookups per question.
const MAX_QUERIES: usize = 3;
const TITLE_KEYS: usize = 2;
const MAX_TITLE_LOOKUPS: usize = 6;
/// Words in the whole-topic search. kiwix wants every word in an article, so
/// a longer query finds nothing.
const TOPIC_WORDS: usize = 4;
/// Results asked for per full-text search: more for several books at once.
const TEXT_RESULTS: usize = 5;
const TEXT_RESULTS_SHARED: usize = 8;

impl Assistant {
    /// Online research: a web search for the question, and the relevant
    /// parts of the first two readable pages, numbered after the library's.
    pub(super) async fn find_web_sources(&self, question: &str, terms: &[String], first: usize) -> Vec<Passage> {
        let mut stems: Vec<String> = search_words(question).iter().map(|w| zaklon_core::translit::fold(&stem(w))).collect();
        stems.extend(terms.iter().flat_map(|t| t.split_whitespace().map(|w| zaklon_core::translit::fold(&stem(w))).collect::<Vec<_>>()));
        stems.retain(|s| s.chars().count() >= 3);
        let pages = crate::web::look_up(&self.web_http, question, 2).await;
        let mut passages: Vec<Passage> = Vec::new();
        for page in pages {
            let st = stems.clone();
            let text = tokio::task::spawn_blocking(move || relevant_text(&page.html, &st, SOURCE_CHARS)).await.unwrap_or_default();
            if text.chars().count() < 80 {
                continue;
            }
            let n = first + passages.len() + 1;
            let source = Source { n, title: page.title, web: true, url: page.url, book_title_en: page.host.clone(), book_title_sr: page.host };
            passages.push(Passage { source, text });
        }
        passages
    }
}

/// The library as the assistant searches it: kiwix-serve, or a stand-in in tests.
pub(crate) trait Shelf: Sync {
    /// Full-text search in books of one language, with one request.
    fn text(&self, books: &[&Book], query: &str, limit: usize) -> impl Future<Output = Found> + Send;
    /// Titles that start like the query, in one book.
    fn titles(&self, book: &Book, query: &str, limit: usize) -> impl Future<Output = Found> + Send;
    /// The parts of an article that matter for the stems, or None for a
    /// redirect or an almost empty page.
    fn read(&self, url: &str, stems: Vec<String>) -> impl Future<Output = Option<String>> + Send;
}

impl Shelf for Library {
    fn text(&self, books: &[&Book], query: &str, limit: usize) -> impl Future<Output = Found> + Send {
        self.search_text(books, query, limit)
    }

    fn titles(&self, book: &Book, query: &str, limit: usize) -> impl Future<Output = Found> + Send {
        self.search_titles(book, query, limit)
    }

    async fn read(&self, url: &str, stems: Vec<String>) -> Option<String> {
        let res = self.fetch(url).await.ok()?;
        if !res.status().is_success() {
            return None;
        }
        let html = res.text().await.ok()?;
        let text = tokio::task::spawn_blocking(move || relevant_text(&html, &stems, SOURCE_CHARS)).await.unwrap_or_default();
        (text.chars().count() >= 80).then_some(text)
    }
}

/// What a library search did, for measuring.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct SearchStats {
    /// Searches started (a cut one counts too), and those recent results answered.
    pub(super) lookups: u32,
    pub(super) cached: u32,
    /// Articles read (started).
    pub(super) reads: u32,
    /// The time ran out before everything planned was done.
    pub(super) cut: bool,
}

/// One request to the library, planned before any is sent.
#[derive(Debug, Clone, PartialEq)]
struct Lookup {
    /// Titles that start like the query (in one book) rather than full text.
    titles: bool,
    /// Indexes into the books. A full-text search asks all the books of one
    /// language at once.
    books: Vec<usize>,
    query: String,
    /// Which set of terms judges its results (0: the question's, 1: English).
    set: usize,
}

/// The library requests for a question, most useful first; the time limit
/// may end the list early. Books of one language are searched together, in
/// one request. In every language: titles like the word the question is
/// about (see `title_keys`; they find the article about it, also when it was
/// typed without diacritics), the whole topic, titles like the next word,
/// then the terms one by one: at most `MAX_QUERIES` full-text searches per
/// language and `MAX_TITLE_LOOKUPS` title lookups. Books in another language
/// than their terms (Serbian books for an English question) get only the
/// first full-text search.
fn plan_lookups(books: &[Book], terms: &[String], terms_en: &[String], language: &str) -> Vec<Lookup> {
    /// Books searched together: the same set of terms and the same languages.
    struct Group {
        set: usize,
        languages: Vec<String>,
        books: Vec<usize>,
        /// Written in the language of its terms.
        native: bool,
        serbian: bool,
        queries: Vec<String>,
    }
    let sets = [terms, terms_en];
    let mut groups: Vec<Group> = Vec::new();
    for (i, b) in books.iter().enumerate() {
        let set = usize::from(english_book(&b.languages) && !terms_en.is_empty());
        let mut languages = b.languages.clone();
        languages.sort();
        match groups.iter_mut().find(|g| g.set == set && g.languages == languages) {
            Some(g) => g.books.push(i),
            None => {
                let wanted = if set == 1 || language == "en" { "eng" } else { "srp" };
                let native = languages.is_empty() || languages.iter().any(|l| l == wanted);
                let serbian = languages.iter().any(|l| l == "srp");
                // Serbian books are written in Cyrillic.
                let mut queries: Vec<String> = Vec::new();
                for q in search_queries(sets[set]) {
                    let q = if serbian { zaklon_core::translit::latin_to_cyrillic(&q) } else { q };
                    if !queries.contains(&q) {
                        queries.push(q);
                    }
                }
                groups.push(Group { set, languages, books: vec![i], native, serbian, queries });
            }
        }
    }
    groups.sort_by_key(|g| !g.native);

    let mut out: Vec<Lookup> = Vec::new();
    let text = |g: &Group, round: usize| {
        g.queries.get(round).map(|q| Lookup { titles: false, books: g.books.clone(), query: q.clone(), set: g.set })
    };
    let mut titles = 0;
    let mut title_lookups = |out: &mut Vec<Lookup>, key: usize| {
        for g in groups.iter().filter(|g| g.native) {
            let Some(key) = title_keys(sets[g.set]).into_iter().nth(key) else { continue };
            for &i in g.books.iter().filter(|&&i| !dictionary(&books[i])) {
                for spelling in title_spellings(&key, g.serbian) {
                    if titles < MAX_TITLE_LOOKUPS {
                        out.push(Lookup { titles: true, books: vec![i], query: spelling, set: g.set });
                        titles += 1;
                    }
                }
            }
        }
    };
    // Title lookups read only the titles' index: they are quick even on a
    // busy disk and find the article about the word, so they go first. A
    // full-text search also reads the articles its snippets come from.
    for round in 0..MAX_QUERIES.max(TITLE_KEYS) {
        if round < TITLE_KEYS {
            title_lookups(&mut out, round);
        }
        out.extend(groups.iter().filter(|g| round == 0 || g.native).filter_map(|g| text(g, round)));
    }
    out
}

/// What titles are looked up by (kiwix finds titles by how they start): a
/// word several terms share ("moždanog" in "znakovi moždanog udara" and
/// "slog moždanog udara"), the terms of one word, then the first word of each
/// phrase, all as stems ("poskoka" finds "Поскок"). Generic words ("zamena")
/// are left out.
fn title_keys(terms: &[String]) -> Vec<String> {
    let words: Vec<Vec<String>> = terms
        .iter()
        .map(|t| {
            let mut stems: Vec<String> = Vec::new();
            for s in t.split_whitespace().filter(|w| !is_stop_word(w) && !GENERIC.contains(&plain(w).as_str())).map(stem) {
                if s.chars().count() >= 3 && !stems.contains(&s) {
                    stems.push(s);
                }
            }
            stems
        })
        .collect();
    let shared = |s: &String| words.iter().filter(|w| w.contains(s)).count();
    let mut keys: Vec<String> = Vec::new();
    let mut add = |s: &String| {
        if !keys.contains(s) {
            keys.push(s.clone());
        }
    };
    let mut common: Vec<&String> = words.iter().flatten().filter(|s| shared(s) >= 2).collect();
    // The most shared first; a stable sort keeps the order of the terms.
    common.sort_by_key(|s| std::cmp::Reverse(shared(s)));
    common.into_iter().for_each(&mut add);
    for (t, w) in terms.iter().zip(&words) {
        if t.split_whitespace().count() == 1 {
            w.iter().for_each(&mut add);
        }
    }
    words.iter().filter_map(|w| w.first()).for_each(&mut add);
    keys
}

/// How a title key may be spelled in titles: in Cyrillic for Serbian
/// books, and when it was typed without diacritics also the likeliest other
/// spelling ("osigurac" is "Осигурач").
fn title_spellings(term: &str, serbian: bool) -> Vec<String> {
    use zaklon_core::translit::{cyrillic_candidates, has_diacritics, has_serbian_latin, latin_to_cyrillic};
    if !serbian || !has_serbian_latin(term) {
        vec![term.to_string()]
    } else if has_diacritics(term) {
        vec![latin_to_cyrillic(term)]
    } else {
        cyrillic_candidates(term, 2)
    }
}

/// A dictionary: its entries only explain a word.
fn dictionary(book: &Book) -> bool {
    let name = book.name.to_lowercase();
    name.contains("wiktionary") || name.contains("dictionary")
}

/// Run the jobs in order, `PARALLEL` at a time, until `deadline`. What is
/// done by then counts; the rest is dropped or never started. Also says how
/// many were started.
async fn in_order<F: Future>(jobs: Vec<F>, deadline: tokio::time::Instant) -> (Vec<Option<F::Output>>, usize) {
    let mut out: Vec<Option<F::Output>> = jobs.iter().map(|_| None).collect();
    let mut waiting = jobs.into_iter().enumerate();
    let mut running = futures_util::stream::FuturesUnordered::new();
    let mut started = 0;
    loop {
        while running.len() < PARALLEL && tokio::time::Instant::now() < deadline {
            let Some((i, job)) = waiting.next() else { break };
            running.push(async move { (i, job.await) });
            started += 1;
        }
        match tokio::time::timeout_at(deadline, running.next()).await {
            Ok(Some((i, v))) => out[i] = Some(v),
            // Nothing left, or the time is up.
            Ok(None) | Err(_) => break,
        }
    }
    (out, started)
}

/// The best few library passages for the search terms. Books are searched in
/// their own language (the question's terms for Serbian books, `terms_en` for
/// English ones), a few requests in all (see `plan_lookups`), within
/// `LIBRARY_TIME`. Results are ranked by their titles, the best few are read,
/// and those are ranked again by how many of the terms the parts chosen from
/// them cover. Two good sources beat three with a wrong one.
pub(super) async fn find_sources<S: Shelf>(
    shelf: &S,
    books: &[Book],
    terms: &[String],
    terms_en: &[String],
    question: &str,
    language: &str,
    safety: bool,
) -> (Vec<Passage>, SearchStats) {
    let mut stats = SearchStats::default();
    if terms.is_empty() && terms_en.is_empty() {
        return (Vec::new(), stats);
    }
    let start = tokio::time::Instant::now();
    // 0: the question's own terms, 1: the English ones.
    let sets = [prepare_terms(terms), prepare_terms(terms_en)];
    let context = context_words(question, terms, terms_en);

    let lookups = plan_lookups(books, terms, terms_en, language);
    let jobs: Vec<_> = lookups.iter().map(|l| lookup(shelf, books, l)).collect();
    let (results, started) = in_order(jobs, start + LOOKUP_TIME).await;
    stats.lookups = started as u32;
    stats.cut = started < lookups.len() || results.iter().any(Option::is_none);

    let mut cands: Vec<Candidate> = Vec::new();
    for (l, found) in lookups.iter().zip(results) {
        let Some(found) = found else { continue };
        stats.cached += u32::from(found.cached);
        for r in found.results {
            let folded = zaklon_core::translit::fold(&r.title);
            // The same article, or the same title from another book, adds nothing.
            if cands.iter().any(|c| c.result.url == r.url || zaklon_core::translit::fold(&c.result.title) == folded) {
                continue;
            }
            let mut scored = score_result(&r.title, &r.snippet, &sets[l.set], &context);
            // A dictionary entry only explains the word.
            let book = r.book.to_lowercase();
            if book.contains("wiktionary") || book.contains("dictionary") {
                scored.score -= 3;
            }
            if scored.score <= 0 {
                continue;
            }
            let medical = books.iter().any(|b| b.name == r.book && medical_pack(&b.pack_id));
            let order = cands.len();
            cands.push(Candidate { result: r, set: l.set, scored, medical, order });
        }
    }
    cands.sort_by(|a, b| b.scored.score.cmp(&a.scored.score).then(a.order.cmp(&b.order)));

    let picked = pick_reads(&cands, safety);
    // Paragraphs are chosen by the terms and by the question's own words
    // ("treat", "leči"), so the practical parts of an article win.
    let stems = [passage_stems(&sets[0], question), passage_stems(&sets[1], "")];
    let reads: Vec<_> = picked.iter().map(|&i| shelf.read(&cands[i].result.url, stems[cands[i].set].clone())).collect();
    let (texts, started) = in_order(reads, start + LIBRARY_TIME).await;
    stats.reads = started as u32;
    stats.cut |= started < picked.len() || texts.iter().any(Option::is_none);
    let mut ranked: Vec<(i32, usize, String)> = Vec::new();
    for (&i, text) in picked.iter().zip(texts) {
        let Some(Some(text)) = text else { continue };
        // Outdated first aid is left out before the model ever sees it.
        let text = if safety { without_harmful_advice(&text, question) } else { text };
        if text.chars().count() < 80 {
            continue;
        }
        let c = &cands[i];
        let covered = coverage(&format!("{}\n{text}", c.result.title), &sets[c.set]);
        if !enough_coverage(covered, sets[c.set].len(), c.scored.main) {
            continue;
        }
        ranked.push((c.scored.score + 3 * covered as i32, i, text));
    }
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(cands[a.1].order.cmp(&cands[b.1].order)));
    let mut chosen: Vec<(usize, String)> = ranked.iter().take(MAX_SOURCES).map(|(_, i, t)| (*i, t.clone())).collect();
    // A health question keeps a place for a medical book: its first aid is current.
    if safety && !chosen.iter().any(|(i, _)| cands[*i].medical) {
        if let Some((_, i, t)) = ranked.iter().find(|(_, i, _)| cands[*i].medical) {
            if chosen.len() >= MAX_SOURCES {
                chosen.pop();
            }
            chosen.push((*i, t.clone()));
        }
    }
    let passages = chosen
        .into_iter()
        .enumerate()
        .map(|(k, (i, text))| {
            let r = &cands[i].result;
            let source = Source {
                n: k + 1,
                title: zaklon_core::translit::cyrillic_to_latin(&r.title),
                web: false,
                url: r.url.clone(),
                book_title_en: r.book_title_en.clone(),
                book_title_sr: r.book_title_sr.clone(),
            };
            Passage { source, text }
        })
        .collect();
    (passages, stats)
}

/// Which candidates (sorted best first) are read: the best `FETCH`, among
/// them the best found with each set of terms (a Serbian question keeps a
/// Serbian article when English ones score higher, and the other way
/// round), and for a health question the best one from a medical book too.
fn pick_reads(cands: &[Candidate], safety: bool) -> Vec<usize> {
    let mut picked: Vec<usize> = (0..2).filter_map(|set| cands.iter().position(|c| c.set == set)).collect();
    for i in 0..cands.len() {
        if picked.len() >= FETCH {
            break;
        }
        if !picked.contains(&i) {
            picked.push(i);
        }
    }
    picked.sort_unstable();
    if safety {
        if let Some(i) = cands.iter().position(|c| c.medical) {
            if !picked.contains(&i) {
                picked.push(i);
            }
        }
    }
    picked
}

/// One planned library request.
async fn lookup<S: Shelf>(shelf: &S, books: &[Book], l: &Lookup) -> Found {
    if l.titles {
        shelf.titles(&books[l.books[0]], &l.query, 8).await
    } else {
        let group: Vec<&Book> = l.books.iter().map(|&i| &books[i]).collect();
        let limit = if group.len() > 1 { TEXT_RESULTS_SHARED } else { TEXT_RESULTS };
        shelf.text(&group, &l.query, limit).await
    }
}

/// A piece of a source, as the model gets it.
#[derive(Debug, Clone)]
pub struct Passage {
    pub source: Source,
    pub text: String,
}

/// A search result on its way to becoming a source.
struct Candidate {
    result: SearchResult,
    /// Which set of terms it was found and is judged with.
    set: usize,
    scored: Scored,
    /// From a medical book (WikiMed, medicine packs).
    medical: bool,
    /// Order found, for ties.
    order: usize,
}

/// Books in English only; they are searched with the English terms.
fn english_book(languages: &[String]) -> bool {
    languages.iter().any(|l| l == "eng") && !languages.iter().any(|l| l == "srp")
}

/// Packs about medicine, whose first aid follows current guidance.
fn medical_pack(pack_id: &str) -> bool {
    let p = pack_id.to_lowercase();
    p.contains("wikimed") || p.contains("medicine") || p.contains("nhs")
}

/// Words asked about in any topic ("treatment", "symptoms"). An article with
/// one of them as its title is rarely what a question is about.
const GENERIC: &[&str] = &[
    "lecenje", "simptomi", "simptom", "zamena", "pritisak", "prva pomoc", "cena", "rok trajanja", "izvlacenje", "popravka", "upotreba",
    "vrste", "uzroci", "treatment", "symptoms", "first aid", "price", "shelf life", "repair", "use", "types", "causes",
];

/// A search term prepared for matching titles and text.
#[derive(Debug, Clone)]
pub(super) struct Term {
    /// The whole term, folded ("ујед змије").
    whole: String,
    /// The folded stems of its meaningful words ("ујед", "змиј").
    pub(super) stems: Vec<String>,
    /// Typed without diacritics, so the loose forms may match too, a little less.
    loose: bool,
    /// The main topic (the first term) or a phrase, and not a generic word:
    /// an article titled like it is about the question.
    topic: bool,
}

pub(super) fn prepare_terms(terms: &[String]) -> Vec<Term> {
    use zaklon_core::translit::{fold, has_diacritics};
    terms
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let t = t.trim();
            let words: Vec<&str> = t.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
            let stems = words.iter().filter(|w| !is_stop_word(w)).map(|w| fold(&stem(w))).filter(|s| s.chars().count() >= 3).collect();
            Term {
                whole: fold(t),
                stems,
                loose: !has_diacritics(t),
                topic: (i == 0 || words.len() > 1) && !GENERIC.contains(&plain(t).as_str()),
            }
        })
        .collect()
}

/// What kiwix is asked, for one set of terms, most useful first: the first
/// terms together (full-text search ranks articles with all of them first,
/// but finds nothing when one word is missing, so at most `TOPIC_WORDS`
/// words), then each term. At most `MAX_QUERIES`, each once.
fn search_queries(terms: &[String]) -> Vec<String> {
    use zaklon_core::translit::fold;
    let mut unique: Vec<&str> = Vec::new();
    for t in terms.iter().map(|t| t.trim()).filter(|t| !t.is_empty()) {
        if !unique.iter().any(|u| fold(u) == fold(t)) {
            unique.push(t);
        }
    }
    let terms = unique;
    let mut queries: Vec<String> = Vec::new();
    let mut topic: Vec<&str> = Vec::new();
    let mut words = 0;
    for &t in &terms {
        let n = t.split_whitespace().count();
        if words + n > TOPIC_WORDS {
            break;
        }
        topic.push(t);
        words += n;
    }
    if topic.len() > 1 {
        queries.push(topic.join(" "));
    }
    for t in terms {
        if !queries.iter().any(|q| fold(q) == fold(t)) {
            queries.push(t.to_string());
        }
    }
    queries.truncate(MAX_QUERIES);
    queries
}

/// The question's and the terms' words, loosely folded: what a title's
/// "(sense)" is checked against.
fn context_words(question: &str, terms: &[String], terms_en: &[String]) -> Vec<String> {
    let all = format!("{question} {} {}", terms.join(" "), terms_en.join(" "));
    let folded = zaklon_core::translit::fold_loose(&all);
    words(&folded).into_iter().filter(|w| w.chars().count() >= 3).map(str::to_string).collect()
}

/// Stems that choose the paragraphs of an article: the terms' and the question's own.
fn passage_stems(terms: &[Term], question: &str) -> Vec<String> {
    let mut stems: Vec<String> = Vec::new();
    let from_question = search_words(question).iter().map(|w| zaklon_core::translit::fold(&stem(w))).collect::<Vec<_>>();
    for s in terms.iter().flat_map(|t| t.stems.iter().cloned()).chain(from_question) {
        if s.chars().count() >= 3 && !stems.contains(&s) {
            stems.push(s);
        }
    }
    stems
}

/// A title that is (nearly) just this word: the article is about it.
fn about_word(title: &str, stem: &str) -> bool {
    title == stem || title.starts_with(stem) && title.chars().count() <= stem.chars().count() + 3
}

/// How well a search result matches.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Scored {
    score: i32,
    /// How many different terms it matches.
    matched: usize,
    /// Its title is the main topic itself.
    main: bool,
}

/// Score a search result by its title (and snippet, for full-text results).
/// A title equal to a topic term earns the most, then a title that is about
/// one of the words; any other word it contains adds a little. Loose matches
/// (words typed without diacritics) earn a little less. Another sense of a
/// word ("Zamena (film)") and disambiguation or list pages lose points.
fn score_result(title: &str, snippet: &str, terms: &[Term], context: &[String]) -> Scored {
    use zaklon_core::translit::{fold, loosen};
    let ft = fold(title);
    let lt = loosen(&ft);
    let text = fold(&format!("{title} {snippet}"));
    let loose_text = loosen(&text);
    let (text_words, loose_words) = (words(&text), words(&loose_text));
    let mut s = Scored::default();
    for (i, t) in terms.iter().enumerate() {
        let mut hit = false;
        if t.topic {
            if ft == t.whole {
                s.score += 6;
                hit = true;
            } else if t.loose && lt == loosen(&t.whole) {
                s.score += 5;
                hit = true;
            }
            s.main |= hit && i == 0;
        }
        let mut about = 0;
        for st in &t.stems {
            let lst = loosen(st);
            if about_word(&ft, st) {
                about += if t.topic { 4 } else { 1 };
                hit = true;
            } else if t.loose && about_word(&lt, &lst) {
                about += if t.topic { 3 } else { 1 };
                hit = true;
            } else if starts_word(&text_words, st) || t.loose && starts_word(&loose_words, &lst) {
                s.score += 1;
                hit = true;
            }
        }
        // A side word earns at most a point for the title.
        s.score += if t.topic { about } else { about.min(1) };
        if hit {
            s.matched += 1;
        }
    }
    if ft.contains("вишезначн") || ft.contains("списак") || ft.contains(&fold("disambiguation")) || ft.starts_with(&fold("list of")) {
        s.score -= 3;
    }
    // "Sterilizacija (medicina)" for a question about jars: another sense.
    if let Some(sense) = title.rfind('(').map(|i| &title[i + 1..]) {
        let sense = zaklon_core::translit::cyrillic_to_latin(sense.trim_end_matches(')'));
        let fits = sense.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).any(|w| {
            let st = zaklon_core::translit::fold_loose(&stem(w));
            st.chars().count() >= 3 && context.iter().any(|c| c.starts_with(&st))
        });
        if !fits {
            s.score -= 4;
        }
    }
    s
}

/// How many of the terms a text covers: a term counts when at least half
/// of its words start words of the text (loosely, so "vodu za pice" covers
/// "voda za piće").
fn coverage(text: &str, terms: &[Term]) -> usize {
    use zaklon_core::translit::loosen;
    let folded = zaklon_core::translit::fold_loose(text);
    let w = words(&folded);
    terms
        .iter()
        .filter(|t| {
            if t.stems.is_empty() {
                return w.contains(&loosen(&t.whole).as_str());
            }
            let hits = t.stems.iter().filter(|s| starts_word(&w, &loosen(s))).count();
            hits * 2 >= t.stems.len()
        })
        .count()
}

/// Whether an article covers enough of the question: with several terms at
/// least two of them, unless it is the article about the main topic itself.
fn enough_coverage(covered: usize, terms: usize, main: bool) -> bool {
    if terms >= 2 {
        covered >= 2 || main && covered >= 1
    } else {
        covered >= 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::passages::{trim_sources, trim_stems};
    use crate::assistant::plan::parse_plan;
    use crate::assistant::test_util::{strings, test_assistant};
    use std::path::PathBuf;
    use std::sync::Mutex;

    fn terms(list: &[&str]) -> Vec<Term> {
        prepare_terms(&list.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    fn score(title: &str, list: &[&str], question: &str) -> Scored {
        let words: Vec<String> = list.iter().map(|s| s.to_string()).collect();
        score_result(title, "", &prepare_terms(&words), &context_words(question, &words, &[]))
    }

    #[test]
    fn titles_typed_without_diacritics_still_match() {
        // lib-14: "osigurac" typed without the č used to score 0.
        let fuse = score("Осигурач", &["osigurac", "zamena", "elektrika"], "kako da zamenim osigurac u kuci");
        assert!(fuse.score >= 8 && fuse.main, "{fuse:?}");
        // lib-04: "masaza srca" now beats the generic "Pritisak".
        let cpr = score("Масажа срца", &["masaza srca", "pritisak"], "kako se radi masaza srca");
        let pressure = score("Притисак", &["masaza srca", "pritisak"], "kako se radi masaza srca");
        assert!(cpr.score > pressure.score + 4, "{cpr:?} {pressure:?}");
        // A term written with diacritics matches exactly: "piće" is not "pica".
        assert_eq!(score("Пица", &["voda za piće", "prečišćavanje vode"], "kako da precistim vodu za pice").score, 0);
        assert!(score("Вода", &["voda za piće", "prečišćavanje vode"], "kako da precistim vodu za pice").score >= 8);
    }

    #[test]
    fn generic_words_and_other_senses_rank_low() {
        let q = "kako se steriliše zimnica da se ne pokvari, tegle i poklopci";
        assert!(score("Стерилизација (медицина)", &["sterilizacija", "zimnica", "tegla"], q).score <= 0, "another sense of the word");
        assert!(score("Zimnica", &["sterilizacija", "zimnica", "tegla"], q).score >= 1);
        assert!(score("Замена (филм)", &["osigurac", "zamena", "elektrika"], "kako da zamenim osigurac").score <= 0);
        assert!(score("Поскок (змија)", &["poskok", "ujed zmije"], "kako da prepoznam poskoka i sta ako te ujede zmija").score >= 2, "a sense the question is about");
        assert!(score("Поскок", &["poskok", "ujed zmije"], "kako da prepoznam poskoka i sta ako te ujede zmija").score >= 10);
        // A side word earns a point for its title at most; the main topic much more.
        let pizza = score("Пица", &["voda", "pica", "cesma"], "kako da precistim vodu za pice");
        let water = score("Вода", &["voda", "pica", "cesma"], "kako da precistim vodu za pice");
        assert!(pizza.score <= 1 && water.score >= 10, "{pizza:?} {water:?}");
        assert_eq!(score("Лечење", &["slavina", "lečenje"], "curi mi slavina").score, 1, "a generic word never earns the title bonus");
        assert!(!score("Prva pomoć", &["prva pomoć", "ujed zmije"], "ujela me zmija").main);
    }

    #[test]
    fn read_articles_are_kept_by_how_much_of_the_question_they_cover() {
        let t = terms(&["voda", "pica", "cesma"]);
        assert_eq!(coverage("Pica je jelo od testa sa sirom i paradajzom.", &t), 1);
        assert!(!enough_coverage(1, t.len(), false), "pizza covers only itself");
        assert_eq!(coverage("Voda sa česme se pre pijenja prokuva.", &t), 2);
        assert!(enough_coverage(2, t.len(), false));
        assert!(enough_coverage(1, 3, true), "the article about the main topic itself stays");
        assert!(enough_coverage(1, 1, false));
        let phrase = terms(&["ujed zmije"]);
        assert_eq!(coverage("Zmije su gmizavci bez nogu.", &phrase), 1, "half of a phrase's words is enough");
    }

    #[test]
    fn the_whole_topic_is_searched_first() {
        let q = search_queries(&strings(&["ubod pčele", "alergija"]));
        assert_eq!(q, vec!["ubod pčele alergija", "ubod pčele", "alergija"]);
        assert_eq!(search_queries(&strings(&["hleb"])), vec!["hleb"]);
        assert!(search_queries(&[]).is_empty());
        // kiwix wants every word of a query: the topic stays short, and the
        // queries stay few.
        let q = search_queries(&strings(&["ujed zmije", "poskok", "prva pomoć kod ujeda", "otok"]));
        assert_eq!(q, vec!["ujed zmije poskok", "ujed zmije", "poskok"]);
        let q = search_queries(&strings(&["prva pomoć kod ujeda zmije", "poskok"]));
        assert_eq!(q, vec!["prva pomoć kod ujeda zmije", "poskok"], "a long first term is the topic itself");
        assert_eq!(search_queries(&strings(&["Hleb", "hleb", " "])), vec!["Hleb"], "the same query once");
        let t = terms(&["voda za piće"]);
        assert_eq!(t[0].stems.len(), 2, "\"za\" is not a search word");
    }

    #[test]
    fn english_books_and_medical_packs_are_recognised() {
        assert!(english_book(&["eng".into()]));
        assert!(!english_book(&["srp".into()]));
        assert!(!english_book(&[]));
        assert!(medical_pack("wikimed-en-mini"));
        assert!(medical_pack("zimgit-medicine-en"));
        assert!(medical_pack("nhs-medicines-en"));
        assert!(!medical_pack("wikipedia-sr-maxi"));
        let p = parse_plan(r#"{"kind":"library","safety":true,"terms":["ujed zmije"],"terms_en":["Snakebite","first aid","snakebite"]}"#);
        assert_eq!(p.terms_en, vec!["snakebite", "first aid"]);
        assert_eq!(p.safety, Some(true));
        assert_eq!(parse_plan("ujed zmije").safety, None, "a broken plan says nothing about safety");
    }

    fn book(name: &str, pack: &str, languages: &[&str]) -> Book {
        Book {
            name: name.into(),
            pack_id: pack.into(),
            title_en: name.into(),
            title_sr: name.into(),
            languages: strings(languages),
            home: String::new(),
            file: PathBuf::new(),
            rel: String::new(),
        }
    }

    /// The books on the test computer: Serbian Wikipedia and Wiktionary, English WikiMed.
    fn three_books() -> Vec<Book> {
        vec![
            book("wikipedia_sr_all_maxi_2026-09", "wikipedia-sr-maxi", &["srp"]),
            book("wiktionary_sr_all_nopic_2026-07", "wiktionary-sr", &["srp"]),
            book("wikipedia_en_medicine_maxi_2026-04", "wikimed-en", &["eng"]),
        ]
    }

    #[test]
    fn a_question_makes_few_library_requests() {
        let books = three_books();
        let terms = strings(&["ujed zmije", "poskok", "prva pomoć kod ujeda", "otok"]);
        let terms_en = strings(&["snakebite", "viper", "first aid"]);
        let l = plan_lookups(&books, &terms, &terms_en, "sr");
        // It used to be every query in every book, each with up to 13 title
        // lookups and 2 full-text searches: well over a hundred requests.
        assert!(l.len() <= 2 * MAX_QUERIES + MAX_TITLE_LOOKUPS, "{l:#?}");
        let texts: Vec<&Lookup> = l.iter().filter(|x| !x.titles).collect();
        assert_eq!(texts.len(), 2 * MAX_QUERIES, "three full-text searches per language");
        // Quick title lookups first, then the whole topic in both languages,
        // each language in one request.
        assert!(l[0].titles);
        assert_eq!(*texts[0], Lookup { titles: false, books: vec![0, 1], query: "ујед змије поскок".into(), set: 0 });
        assert_eq!(*texts[1], Lookup { titles: false, books: vec![2], query: "snakebite viper first aid".into(), set: 1 });
        // Titles like the words the question is about, not in the dictionary.
        let titles: Vec<&Lookup> = l.iter().filter(|x| x.titles).collect();
        assert!(titles.len() <= MAX_TITLE_LOOKUPS);
        assert!(titles.iter().all(|x| x.books.len() == 1 && x.books[0] != 1), "{titles:?}");
        assert!(titles.iter().any(|x| x.books == [0] && x.query == "ујед"), "{titles:?}");
        assert!(titles.iter().any(|x| x.books == [0] && x.query == "поскок"), "{titles:?}");
        assert!(titles.iter().any(|x| x.books == [2] && x.query == "snakebit"), "{titles:?}");
        // Never two languages in one request, never the same request twice.
        for x in &l {
            let languages: std::collections::HashSet<&Vec<String>> = x.books.iter().map(|&i| &books[i].languages).collect();
            assert_eq!(languages.len(), 1, "{x:?}");
            assert_eq!(l.iter().filter(|y| *y == x).count(), 1, "{x:?}");
        }
        // A word written with diacritics has one spelling.
        let l = plan_lookups(&books, &strings(&["šargarepa"]), &[], "sr");
        assert_eq!(l.iter().filter(|x| x.titles).map(|x| x.query.as_str()).collect::<Vec<_>>(), vec!["шаргареп"]);
        assert_eq!(title_spellings("osigurac", true), vec!["осигурац", "осигурач"], "typed without the č");
    }

    #[test]
    fn titles_are_looked_up_by_the_words_the_question_is_about() {
        let keys = |t: &[&str]| title_keys(&strings(t));
        // A word the terms share: the article about it is what the question is about.
        assert_eq!(keys(&["znakovi moždanog udara", "prepoznavanje moždanog udara", "slog moždanog udara"])[..2], ["moždan", "udar"]);
        assert_eq!(keys(&["stroke symptoms", "recognizing stroke", "stroke signs"])[0], "strok");
        assert_eq!(keys(&["zamena osigurača", "osigurač", "elektrika"])[..2], ["osigurač", "elektrik"], "\"zamena\" is generic");
        // Then the terms of one word, then the first word of each phrase.
        assert_eq!(keys(&["poskoka", "ujed zmije", "prva pomoć"])[..2], ["poskok", "ujed"]);
        assert_eq!(keys(&["prvi srpski ustanak", "karađorđe", "ustanak"])[..2], ["ustanak", "karađorđ"]);
        assert!(keys(&[]).is_empty());
    }

    #[test]
    fn many_books_still_make_few_requests() {
        let mut books = three_books();
        books.push(book("wikipedia_en_all_nopic_2026-01", "wikipedia-en-nopic", &["eng"]));
        books.push(book("ifixit_en_all_2025-12", "ifixit-en", &["eng"]));
        books.push(book("wikibooks_sr_all_nopic_2026-07", "wikibooks-sr", &["srp"]));
        books.push(book("wikipedia_de_all_nopic_2026-01", "wikipedia-de", &["deu"]));
        let terms = strings(&["bee sting", "allergy", "swelling"]);
        // An English question: its terms serve the English books too.
        let l = plan_lookups(&books, &terms, &terms, "en");
        let languages = 3;
        assert!(l.len() <= languages * MAX_QUERIES + MAX_TITLE_LOOKUPS, "{l:#?}");
        // The English books first, all in one request; the others get the topic only.
        assert_eq!(l.iter().find(|x| !x.titles).unwrap().books, vec![2, 3, 4]);
        assert_eq!(l.iter().filter(|x| x.books.contains(&0)).count(), 1, "Serbian books: once");
        assert_eq!(l.iter().filter(|x| x.books.contains(&6)).count(), 1, "German: once");
        assert!(l.iter().filter(|x| x.titles).all(|x| books[x.books[0]].languages == ["eng"]));
        assert!(plan_lookups(&books, &[], &[], "sr").is_empty());
    }

    /// A library where every request takes `delay`: a full-text search or a
    /// title lookup finds one article titled like the query.
    struct SlowShelf {
        delay: Duration,
        /// Say the results were remembered from a recent search.
        remembered: bool,
        asked: Mutex<Vec<String>>,
    }

    impl SlowShelf {
        fn new(delay: Duration, remembered: bool) -> Self {
            SlowShelf { delay, remembered, asked: Mutex::new(Vec::new()) }
        }

        async fn find(&self, book: String, query: String) -> Found {
            self.asked.lock().unwrap().push(query.clone());
            tokio::time::sleep(self.delay).await;
            let r = SearchResult {
                title: query.clone(),
                url: format!("/kiwix/content/{book}/{query}"),
                snippet: String::new(),
                book,
                book_title_en: String::new(),
                book_title_sr: String::new(),
                kind: "text",
            };
            Found { results: vec![r], cached: self.remembered }
        }
    }

    impl Shelf for SlowShelf {
        fn text(&self, books: &[&Book], query: &str, _limit: usize) -> impl Future<Output = Found> + Send {
            self.find(books[0].name.clone(), query.to_string())
        }

        fn titles(&self, book: &Book, query: &str, _limit: usize) -> impl Future<Output = Found> + Send {
            self.find(book.name.clone(), query.to_string())
        }

        async fn read(&self, _url: &str, _stems: Vec<String>) -> Option<String> {
            tokio::time::sleep(self.delay).await;
            Some("Ујед змије: поскок је најотровнија змија у Србији. A snakebite from a viper needs first aid and a doctor at once.".to_string())
        }
    }

    #[tokio::test(start_paused = true)]
    async fn the_library_search_keeps_to_its_time() {
        let books = three_books();
        let terms = strings(&["ujed zmije", "poskok"]);
        let terms_en = strings(&["snakebite", "viper"]);
        let question = "kako da prepoznam poskoka i sta ako te ujede zmija";
        let planned = plan_lookups(&books, &terms, &terms_en, "sr").len();

        // A quiet disk: everything planned is asked and read, quickly.
        let quick = SlowShelf::new(Duration::from_millis(100), true);
        let t = tokio::time::Instant::now();
        let (found, stats) = find_sources(&quick, &books, &terms, &terms_en, question, "sr", false).await;
        assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
        assert!(!stats.cut && !found.is_empty(), "{stats:?}");
        assert_eq!((stats.lookups as usize, quick.asked.lock().unwrap().len()), (planned, planned));
        assert_eq!(stats.cached, stats.lookups, "remembered results are counted");
        assert_eq!(stats.reads as usize, FETCH);

        // A disk another program keeps busy: 5 s a request. The question gets
        // what was done in time, and nothing more is asked.
        let slow = SlowShelf::new(Duration::from_secs(5), false);
        let t = tokio::time::Instant::now();
        let (found, stats) = find_sources(&slow, &books, &terms, &terms_en, question, "sr", false).await;
        assert!(t.elapsed() <= LIBRARY_TIME, "{:?}", t.elapsed());
        assert!(stats.cut);
        assert_eq!(stats.lookups, 6, "two at a time, started before 12 s: at 0, 5 and 10 s");
        assert_eq!(slow.asked.lock().unwrap().len(), 6);
        assert_eq!(stats.cached, 0);
        assert_eq!(stats.reads, 4, "two at 12 s, two more at 17 s");
        assert!(!found.is_empty(), "what was read in time is used");
    }

    /// The queries before this change: all terms together, each term, and the
    /// words of phrases, up to 8, each asked in every book on its own.
    fn old_queries(terms: &[String]) -> Vec<String> {
        let terms: Vec<&String> = terms.iter().take(4).collect();
        let mut q: Vec<String> = Vec::new();
        if terms.len() > 1 {
            q.push(terms.iter().map(|t| t.as_str()).collect::<Vec<_>>().join(" "));
        }
        for t in &terms {
            if !q.contains(t) {
                q.push(t.to_string());
            }
        }
        for t in terms.iter().filter(|t| t.contains(' ')) {
            for w in t.split_whitespace() {
                let st = stem(w);
                if st.chars().count() >= 4 && !is_stop_word(w) && !q.contains(&st) {
                    q.push(st);
                }
            }
        }
        q.truncate(8);
        q
    }

    /// The library step against a kiwix-serve that is already running with
    /// the three books above (read only), for measuring a real computer:
    /// `ZAKLON_KIWIX_PORT=50509 cargo test -p zaklon-hub --lib live_library -- --ignored --nocapture`
    /// (`ZAKLON_LIVE_ONLY=<part of a question>` runs just that question).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore]
    async fn live_library_search() {
        let Some(port) = std::env::var("ZAKLON_KIWIX_PORT").ok().and_then(|p| p.parse().ok()) else { return };
        let ai = test_assistant();
        ai.library.attach(port);
        let books = three_books();
        // Terms as the 9B model planned them on the test computer, and one
        // question routed by its words (no plan in time).
        let cases: [(&str, &[&str], &[&str], bool); 6] = [
            ("kako da prepoznam poskoka i sta ako te ujede zmija", &["poskoka", "ujed zmije", "prva pomoć"], &["snake bite", "first aid"], true),
            ("koji su znaci mozdanog udara, kako da prepoznam slog", &["znakovi moždanog udara", "prepoznavanje moždanog udara", "slog moždanog udara"], &["stroke symptoms", "recognizing stroke", "stroke signs"], true),
            ("kako da zamenim osigurac u kuci, izbacuje mi struju", &["zamena osigurača", "osigurač", "elektrika"], &["fuse replacement", "circuit breaker"], false),
            ("kako se steriliše zimnica da se ne pokvari, tegle i poklopci", &["sterilizacija tegli", "zimnica", "poklopci"], &["jar sterilization", "canning"], false),
            ("uhvatio me krpelj, kako da ga izvadim?", &["vađenje krpelja", "krpelj"], &["tick removal", "tick bite"], true),
            ("kad je poceo prvi srpski ustanak i ko ga je vodio", &["prvi srpski ustanak", "karađorđe", "ustanak"], &[], false),
        ];
        let only = std::env::var("ZAKLON_LIVE_ONLY").unwrap_or_default();
        for label in ["new, first time", "new, asked again"] {
            println!("--- {label}");
            for (q, terms, terms_en, safety) in cases.iter().filter(|c| c.0.contains(&only)) {
                let (terms, terms_en) = (strings(terms), strings(terms_en));
                let t = tokio::time::Instant::now();
                let (found, stats) = find_sources(&*ai.library, &books, &terms, &terms_en, q, "sr", *safety).await;
                let titles: Vec<&str> = found.iter().map(|p| p.source.title.as_str()).collect();
                println!("{:>6} ms  {stats:?}  {titles:?}  <- {q}", t.elapsed().as_millis());
            }
        }
        // The same questions, asked the way it was before (search only, no reading).
        println!("--- before: every query in every book, 4 at a time");
        for (q, terms, terms_en, _) in &cases {
            let (terms, terms_en) = (strings(terms), strings(terms_en));
            let queries = [old_queries(&terms), old_queries(&terms_en)];
            let mut jobs: Vec<(&Book, String)> = Vec::new();
            for i in 0..8 {
                for b in &books {
                    let set = usize::from(english_book(&b.languages) && !terms_en.is_empty());
                    if let Some(query) = queries[set].get(i) {
                        jobs.push((b, query.clone()));
                    }
                }
            }
            // Each search in a Serbian book: title lookups for up to 12 spellings
            // and the query, and two full-text searches; in other books one of each.
            let requests: usize = jobs
                .iter()
                .map(|(b, query)| {
                    if b.languages.iter().any(|l| l == "srp") && zaklon_core::translit::has_serbian_latin(query) {
                        zaklon_core::translit::cyrillic_candidates(query, 12).len() + 1 + 2
                    } else {
                        2
                    }
                })
                .sum();
            let t = tokio::time::Instant::now();
            let searches: Vec<_> = jobs.iter().map(|(b, query)| ai.library.search_books(books.clone(), query, Some(b.name.as_str()), 5)).collect();
            let found: Vec<Vec<SearchResult>> = futures_util::stream::iter(searches).buffered(4).collect().await;
            let results = found.iter().map(Vec::len).sum::<usize>();
            println!("{:>6} ms  {} searches, {requests} requests, {results} results  <- {q}", t.elapsed().as_millis(), jobs.len());
        }
    }

    /// The sources of a few evaluation questions before and after they are
    /// shortened, against a kiwix-serve that is already running with the
    /// three books above (read only), to see what the model gets:
    /// `ZAKLON_KIWIX_PORT=50509 ZAKLON_TRIM=1600 cargo test -p zaklon-hub --lib live_sources_trimmed -- --ignored --nocapture`
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore]
    async fn live_sources_trimmed() {
        let Some(port) = std::env::var("ZAKLON_KIWIX_PORT").ok().and_then(|p| p.parse().ok()) else { return };
        let budget: usize = std::env::var("ZAKLON_TRIM").ok().and_then(|v| v.parse().ok()).unwrap_or(1600);
        let ai = test_assistant();
        ai.library.attach(port);
        let books = three_books();
        // Terms as the 9B model planned them in an evaluation run.
        let cases: [(&str, &[&str], &[&str], bool); 5] = [
            ("koji su znaci mozdanog udara, kako da prepoznam slog", &["znakovi moždanog udara", "prepoznavanje moždanog udara"], &["stroke symptoms", "recognizing stroke"], true),
            ("kako se prepoznaje trovanje ugljen monoksidom od peci", &["trovanje ugljen-monoksidom", "ugljen-monoksid", "simptomi trovanja"], &["carbon monoxide poisoning", "carbon monoxide", "symptoms"], true),
            ("kako se radi masaza srca, koliko pritisaka pa koliko udisaja", &["masaža srca", "prva pomoć srčani zastoj"], &["cardiac massage", "cardiac arrest first aid"], true),
            ("kako se zaustavlja krvarenje iz nosa", &["zaustavljanje krvarenja iz nosa", "epistaksa"], &["nosebleed"], true),
            ("kolika je normalna telesna temperatura a od koliko je groznica", &["telesna temperatura", "groznica"], &["body temperature", "fever"], true),
        ];
        let only = std::env::var("ZAKLON_LIVE_ONLY").unwrap_or_default();
        for (q, terms, terms_en, safety) in cases.iter().filter(|c| c.0.contains(&only)) {
            let (terms, terms_en) = (strings(terms), strings(terms_en));
            let (mut found, _) = find_sources(&*ai.library, &books, &terms, &terms_en, q, "sr", *safety).await;
            println!("=== {q}");
            for p in &found {
                println!("--- [{}] {} ({} chars)\n{}", p.source.n, p.source.title, p.text.chars().count(), p.text);
            }
            trim_sources(&mut found, &trim_stems(&terms, &terms_en, q), budget);
            println!("=== trimmed to {budget}");
            for p in &found {
                println!("--- [{}] {} ({} chars)\n{}", p.source.n, p.source.title, p.text.chars().count(), p.text);
            }
        }
    }

    fn candidate(set: usize, medical: bool) -> Candidate {
        let result = SearchResult {
            title: String::new(),
            url: String::new(),
            snippet: String::new(),
            book: String::new(),
            book_title_en: String::new(),
            book_title_sr: String::new(),
            kind: "text",
        };
        Candidate { result, set, scored: Scored::default(), medical, order: 0 }
    }

    #[test]
    fn each_language_keeps_an_article_to_read() {
        // Best first: more English results than are read score above the first Serbian one.
        let mut c: Vec<Candidate> = (0..=FETCH).map(|_| candidate(1, false)).collect();
        c.push(candidate(0, false));
        c.push(candidate(1, true));
        let mut want: Vec<usize> = (0..FETCH - 1).collect();
        want.push(FETCH + 1);
        assert_eq!(pick_reads(&c, false), want);
        want.push(FETCH + 2);
        assert_eq!(pick_reads(&c, true), want, "and a medical book for a health question");
        assert_eq!(pick_reads(&c[..2], false), vec![0, 1]);
        assert!(pick_reads(&[], true).is_empty());
    }
}
