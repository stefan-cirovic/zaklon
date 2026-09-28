//! The household assistant on the hub. Runs `llama-server` (llama.cpp, MIT,
//! a separate program) on 127.0.0.1 with the chosen model, only while it is
//! being used, and answers questions from the library: it searches the
//! installed knowledge packs, gives the model the best passages, and asks it
//! to answer only from them and name its sources. Small models invent facts
//! when left alone; grounding is the point.
//!
//! Answers are produced in the background and read by polling, which works
//! the same for the laptop window and for phones. Every step has a time
//! limit, and a question nobody waits for any more gives way to the next.
//!
//! This file has the questions and their answers, and the way a question is
//! answered (`Assistant::run`). The rest: the AI engine (`engine`), asking it
//! (`llm`), deciding what a question is about (`plan`), finding sources in
//! the library (`sources`) and what of them the model gets (`passages`),
//! prompts (`prompts`), checking a written answer (`finish`), the supplies
//! (`supplies`), the household's notes (`notes`), and words (`text`).

mod engine;
mod finish;
mod llm;
mod notes;
mod passages;
mod plan;
mod prompts;
mod sources;
mod supplies;
#[cfg(test)]
mod test_util;
mod text;

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use zaklon_core::memory::Note;
use zaklon_core::supplies::Item;

use crate::downloads::Downloads;
use crate::kiwix::Library;

pub use engine::{recommended_model, EngineState, ModelChoice, Overview};
pub use notes::{relevant_notes, with_health_notes};
pub use passages::relevant_text;
pub use plan::{
    health_question, mentions_supplies, parse_keywords, parse_plan, plain_supplies_question, remember_request, route, supplies_override, Plan,
    PlannedChange,
};
pub use prompts::{clean_history, HISTORY_TURNS};
pub use sources::Passage;
pub use supplies::{convert, match_item, match_items, propose, supplies_list, supplies_named, ItemMatch, Proposal};
pub use text::{article_text, search_terms, search_words, stem, strip_html};

use engine::{Running, SLOT_ANSWER, SLOT_HEALTH, SLOT_SUPPLIES};
use finish::{fixed_reply, Finish};
use notes::with_notes;
use passages::{trim_sources, trim_stems};
use plan::supplies_plan;
use prompts::{build_messages, fit_passages, PROMPT_CHARS};
use sources::{find_sources, SearchStats};
use supplies::supplies_messages;

/// Database setting that remembers the chosen model.
pub const SETTING_MODEL: &str = "assistant_model";
const KEEP_ANSWERS: usize = 30;
/// Questions allowed to wait for their turn (the whole household, not a crowd).
const MAX_PENDING: usize = 4;
/// A question's time from its turn (with the model loaded) to the end of its
/// answer. Its engine request is stopped then, and the next question goes.
const QUESTION_TIME: Duration = Duration::from_secs(6 * 60);
/// A question nobody has looked at for this long is abandoned: it gives way
/// to a question somebody waits for. (The app does not ask while it is in the
/// background, so an answer is still written for someone who comes back.)
const ABANDONED: Duration = Duration::from_secs(60);
/// How often a waiting or running question checks whether to stop.
const CHECK_EVERY: Duration = Duration::from_millis(500);
/// The error of an answer stopped at `QUESTION_TIME`.
const TOO_LONG: &str = "the answer took too long and was stopped";
/// The error of an answer stopped before it had any text (the app shows "Canceled").
const CANCELLED: &str = "cancelled";

#[derive(Debug, Clone, Serialize)]
pub struct Source {
    pub n: usize,
    pub title: String,
    /// From the internet (online research) rather than the library.
    pub web: bool,
    /// Path of the article, relative to the hub (`/kiwix/content/...`).
    pub url: String,
    pub book_title_en: String,
    pub book_title_sr: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnswerStatus {
    Searching,
    Starting,
    Thinking,
    Done,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct Answer {
    pub id: String,
    pub question: String,
    pub status: AnswerStatus,
    pub text: String,
    pub sources: Vec<Source>,
    /// What was looked up in the library.
    pub searched: Vec<String>,
    /// Answered from the household's supplies.
    pub from_supplies: bool,
    /// Online research was on for this question.
    pub used_internet: bool,
    /// The answer names at least one of its sources ([1]...).
    pub cited: bool,
    pub proposal: Option<Proposal>,
    /// False when the model answered alone: nothing was found in the
    /// library, or the answer cites none of the sources it was given.
    pub grounded: bool,
    /// A health or first-aid question: stricter rules were applied.
    pub safety: bool,
    /// A fixed reply written by the hub, not by the model (a health question
    /// with no checked source in the library).
    pub fixed: bool,
    pub language: &'static str,
    pub tokens_per_second: f64,
    pub error: Option<String>,
    /// How long the steps took, in milliseconds (for measuring): waiting for
    /// an earlier question, starting the AI engine, deciding what the question
    /// is about, the library, the answer's first word (from asking the engine),
    /// and everything from its turn to the end.
    pub wait_ms: u64,
    pub engine_ms: u64,
    pub plan_ms: u64,
    pub search_ms: u64,
    pub first_token_ms: u64,
    pub total_ms: u64,
    /// The plan did not come in time; the question was routed by its words.
    pub plan_fallback: bool,
    /// A plain question about the supplies: routed by its words without
    /// asking the model, which would have come to the same.
    pub plan_skipped: bool,
    /// Library searches started, how many of them recent results answered,
    /// and articles read.
    pub lookups: u32,
    pub lookups_cached: u32,
    pub articles_read: u32,
    /// The library's time ran out before every search or read was done.
    pub search_cut: bool,
    /// Characters of source text the answer was given.
    pub source_chars: u32,
    /// Prompt tokens the engine read for the answer, and the ones before
    /// them it still had from an earlier prompt.
    pub prompt_tokens: u32,
    pub cached_tokens: u32,
    #[serde(skip)]
    created: Instant,
    /// Someone asked to stop this answer.
    #[serde(skip)]
    cancel: bool,
    /// When someone last asked for this answer.
    #[serde(skip)]
    seen: Instant,
    /// It has had its turn (it is not waiting for another question).
    #[serde(skip)]
    started: bool,
    /// When it is stopped however far it got (set once the engine runs).
    #[serde(skip)]
    deadline: Option<Instant>,
}

impl Answer {
    /// A question just asked, waiting for its turn.
    fn new(id: String, question: String, language: &'static str, online: bool, now: Instant) -> Self {
        Answer {
            id,
            question,
            status: AnswerStatus::Searching,
            text: String::new(),
            sources: Vec::new(),
            searched: Vec::new(),
            from_supplies: false,
            used_internet: online,
            cited: false,
            proposal: None,
            grounded: false,
            safety: false,
            fixed: false,
            language,
            tokens_per_second: 0.0,
            error: None,
            wait_ms: 0,
            engine_ms: 0,
            plan_ms: 0,
            search_ms: 0,
            first_token_ms: 0,
            total_ms: 0,
            plan_fallback: false,
            plan_skipped: false,
            lookups: 0,
            lookups_cached: 0,
            articles_read: 0,
            search_cut: false,
            source_chars: 0,
            prompt_tokens: 0,
            cached_tokens: 0,
            created: now,
            cancel: false,
            seen: now,
            started: false,
            deadline: None,
        }
    }
}

/// What a question comes with: the conversation so far, the supplies, the
/// household's notes, and whether online research is on.
#[derive(Debug, Clone, Default)]
pub struct AskContext {
    pub history: Vec<Turn>,
    pub items: Vec<Item>,
    pub notes: Vec<Note>,
    pub online: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Turn {
    pub question: String,
    pub answer: String,
}

pub struct Assistant {
    downloads: Arc<Downloads>,
    library: Arc<Library>,
    /// The model chosen in the app, if any (saved in the database by the API).
    chosen: Mutex<Option<String>>,
    engine_dir: PathBuf,
    running: tokio::sync::Mutex<Option<Running>>,
    state: Mutex<EngineState>,
    port: AtomicU16,
    /// Seconds since `epoch` of the last question.
    last_used: AtomicU64,
    epoch: Instant,
    answers: Mutex<HashMap<String, Answer>>,
    /// One question at a time: a small computer runs one model at a time.
    turn: tokio::sync::Mutex<()>,
    /// Set by `stop()` to end a model load in progress.
    cancel_load: AtomicBool,
    /// The running engine has read the plan's instructions once (they stay cached).
    plan_warm: AtomicBool,
    /// How fast the engine reads a prompt, in tokens a second, and for which
    /// model: measured on every long enough prompt, and what decides how
    /// much source text an answer gets (see `source_budget`).
    read_speed: Mutex<Option<(String, f64)>>,
    #[cfg(windows)]
    job: crate::kiwix::job::Job,
    http: reqwest::Client,
    /// For online research only.
    web_http: reqwest::Client,
}

impl Assistant {
    pub fn new(downloads: Arc<Downloads>, library: Arc<Library>, chosen: Option<String>) -> Arc<Self> {
        let engine_dir = downloads.library_dir().join("bin").join("llama");
        Arc::new(Self {
            downloads,
            library,
            chosen: Mutex::new(chosen),
            engine_dir,
            running: tokio::sync::Mutex::new(None),
            state: Mutex::new(EngineState::Stopped),
            port: AtomicU16::new(0),
            last_used: AtomicU64::new(0),
            epoch: Instant::now(),
            answers: Mutex::new(HashMap::new()),
            turn: tokio::sync::Mutex::new(()),
            cancel_load: AtomicBool::new(false),
            plan_warm: AtomicBool::new(false),
            read_speed: Mutex::new(None),
            #[cfg(windows)]
            job: crate::kiwix::job::Job::new(),
            http: reqwest::Client::builder().no_proxy().connect_timeout(Duration::from_secs(5)).build().expect("http client"),
            web_http: crate::web::client(),
        })
    }

    pub fn answer(&self, id: &str) -> Option<Answer> {
        self.answers.lock().unwrap_or_else(|p| p.into_inner()).get(id).cloned()
    }

    /// An answer as the one who asked reads it. Reading it shows that
    /// somebody still waits for it.
    pub fn poll(&self, id: &str) -> Option<Answer> {
        let mut answers = self.answers.lock().unwrap_or_else(|p| p.into_inner());
        let a = answers.get_mut(id)?;
        a.seen = Instant::now();
        Some(a.clone())
    }

    fn update(&self, id: &str, f: impl FnOnce(&mut Answer)) {
        if let Some(a) = self.answers.lock().unwrap_or_else(|p| p.into_inner()).get_mut(id) {
            f(a);
        }
    }

    /// Start answering; the answer is read with `poll(id)`.
    pub fn ask(self: &Arc<Self>, question: &str, app_language: &str, ctx: AskContext) -> Result<String, String> {
        let question = question.trim().to_string();
        if question.is_empty() {
            return Err("ask something first".into());
        }
        if question.chars().count() > 2000 {
            return Err("the question is too long".into());
        }
        let language = zaklon_core::lang::question_language(&question).unwrap_or(if app_language == "sr" { "sr" } else { "en" });
        let id = uuid::Uuid::new_v4().to_string();
        {
            let mut answers = self.answers.lock().unwrap_or_else(|p| p.into_inner());
            // Only questions somebody still waits for count: an abandoned one
            // gives way to this one as soon as it is in line.
            let now = Instant::now();
            if answers.values().filter(|a| waited_for(a, now)).count() >= MAX_PENDING {
                return Err("the assistant is busy with other questions; try again in a moment".into());
            }
            if answers.len() >= KEEP_ANSWERS {
                if let Some(oldest) = answers.values().min_by_key(|a| a.created).map(|a| a.id.clone()) {
                    answers.remove(&oldest);
                }
            }
            answers.insert(id.clone(), Answer::new(id.clone(), question.clone(), language, ctx.online, now));
        }
        self.last_used.store(self.epoch.elapsed().as_secs(), Ordering::Relaxed);
        let me = self.clone();
        let id2 = id.clone();
        let mut ctx = ctx;
        ctx.history = clean_history(&ctx.history);
        tokio::spawn(async move {
            let asked = Instant::now();
            let r = match me.wait_turn(&id2).await {
                Ok(_turn) => {
                    let started = Instant::now();
                    me.update(&id2, |a| {
                        a.started = true;
                        a.wait_ms = ms(asked);
                    });
                    let r = match me.stop_requested(&id2) {
                        Ok(()) => me.run(&id2, &question, language, &ctx).await,
                        Err(e) => Err(e),
                    };
                    me.update(&id2, |a| a.total_ms = ms(started));
                    r
                }
                // It left the line without its turn.
                Err(e) => {
                    me.update(&id2, |a| a.wait_ms = ms(asked));
                    Err(e)
                }
            };
            if let Err(e) = r {
                if e != CANCELLED {
                    warn!("assistant: {e}");
                }
                me.update(&id2, |a| {
                    a.status = AnswerStatus::Failed;
                    a.error = Some(e);
                });
            }
            me.last_used.store(me.epoch.elapsed().as_secs(), Ordering::Relaxed);
        });
        Ok(id)
    }

    /// Ask to stop an answer that is waiting or being written. What was
    /// written so far is kept.
    pub fn cancel(&self, id: &str) -> bool {
        let mut answers = self.answers.lock().unwrap_or_else(|p| p.into_inner());
        match answers.get_mut(id) {
            Some(a) if !finished(a) => {
                a.cancel = true;
                true
            }
            Some(_) => true,
            None => false,
        }
    }

    /// `Err` with the reason once this answer should stop (see `stop_reason`).
    fn stop_requested(&self, id: &str) -> Result<(), String> {
        let answers = self.answers.lock().unwrap_or_else(|p| p.into_inner());
        match stop_reason(&answers, id, Instant::now()) {
            Some(why) => Err(why.into()),
            None => Ok(()),
        }
    }

    /// Wait for this question's turn, keeping its place in line; it leaves
    /// the line when it should stop.
    async fn wait_turn(&self, id: &str) -> Result<tokio::sync::MutexGuard<'_, ()>, String> {
        self.unless_stopped(id, self.turn.lock()).await
    }

    /// Wait for `work`, but drop it as soon as this answer should stop.
    /// Dropping a request closes its connection, and the AI engine stops
    /// working on it.
    async fn unless_stopped<T>(&self, id: &str, work: impl Future<Output = T>) -> Result<T, String> {
        tokio::pin!(work);
        loop {
            tokio::select! {
                done = &mut work => return Ok(done),
                _ = tokio::time::sleep(CHECK_EVERY) => self.stop_requested(id)?,
            }
        }
    }

    async fn run(&self, id: &str, question: &str, language: &'static str, ctx: &AskContext) -> Result<(), String> {
        let (history, items, notes, online) = (&ctx.history[..], &ctx.items[..], &ctx.notes[..], ctx.online);
        // 1. Make sure the engine runs (it also decides what the question is about).
        self.update(id, |a| a.status = AnswerStatus::Starting);
        let t = Instant::now();
        let port = self.ensure_running().await?;
        // The question's own time starts once the model is loaded.
        self.update(id, |a| {
            a.engine_ms = ms(t);
            a.deadline = Some(Instant::now() + QUESTION_TIME);
        });
        self.stop_requested(id)?;
        self.update(id, |a| a.status = AnswerStatus::Searching);
        let t0 = Instant::now();
        // A plain question about the supplies ends up there whatever the
        // model says (see `route`): it is not asked, which saves seconds.
        let skipped = plain_supplies_question(question);
        let planned = if skipped { Some(supplies_plan(question)) } else { self.unless_stopped(id, self.plan(port, question, language)).await? };
        let plan_ms = ms(t0);
        let fallback = planned.is_none();
        // No plan in time: a library question searched by its own words, with
        // the keyword checks below for supplies, notes and health.
        let mut plan = planned.unwrap_or_else(|| parse_plan(""));
        if fallback {
            info!(ms = plan_ms, "assistant: no plan in time; routing by the question's words");
        } else if skipped {
            info!("assistant: a plain question about the supplies; no plan needed");
        } else {
            info!(ms = plan_ms, kind = %plan.kind, "assistant: plan");
        }
        self.update(id, |a| {
            a.plan_ms = plan_ms;
            a.plan_fallback = fallback;
            a.plan_skipped = skipped;
        });
        self.stop_requested(id)?;
        route(&mut plan, question, items);

        // 2. Something to remember: propose it, keep nothing yet.
        if plan.kind == "remember" && !plan.note.trim().is_empty() {
            let note: String = plan.note.trim().chars().take(zaklon_core::memory::MAX_TEXT).collect();
            let text = if language == "sr" { format!("Da zapamtim: „{note}“?") } else { format!("Remember this: \"{note}\"?") };
            let proposal = Proposal {
                action: "remember".into(),
                item_id: None,
                name: note,
                quantity: 0.0,
                unit: String::new(),
                category: String::new(),
                current: None,
            };
            self.update(id, |a| {
                a.text = text;
                a.proposal = Some(proposal);
                a.grounded = true;
                a.status = AnswerStatus::Done;
            });
            return Ok(());
        }
        let safety = plan.safety.unwrap_or(false) || health_question(question);
        // Notes that share a word with the question; for a health question
        // also every note about health (an allergy matters whatever the words).
        let direct = relevant_notes(notes, question, &plan.terms);
        let known = if safety { with_health_notes(&direct, notes) } else { direct.clone() };

        // 2a. A change to the supplies: propose it, change nothing.
        if plan.kind == "supplies_change" {
            if let Some(change) = plan.change.as_ref() {
                let (text, proposal) = propose(change, items, language);
                self.update(id, |a| {
                    a.text = text;
                    a.proposal = proposal;
                    a.from_supplies = true;
                    a.grounded = true;
                    a.status = AnswerStatus::Done;
                });
                return Ok(());
            }
        }

        // 2b. A question about the supplies: answer from the list.
        if plan.kind == "supplies_question" {
            let list = supplies_list(items, language);
            let named = supplies_named(items, &plan.terms, question, language);
            self.update(id, |a| {
                a.from_supplies = true;
                a.grounded = true;
                a.searched = plan.terms.clone();
                a.status = AnswerStatus::Thinking;
            });
            let messages = with_notes(supplies_messages(question, language, &list, &named, history), &known, language, true);
            return self.stream_answer(id, port, SLOT_SUPPLIES, messages, language, Finish::default()).await;
        }

        // 2c. Everything else: find passages in the library, if there is one.
        self.update(id, |a| a.safety = safety);
        let mut terms = plan.terms.clone();
        if terms.is_empty() {
            // The model gave nothing usable: fall back to the question's own words.
            terms = search_words(question).iter().map(|w| stem(w)).collect();
        }
        // English books are searched with English terms.
        let terms_en: Vec<String> = if language == "en" { terms.clone() } else { plan.terms_en.clone() };
        let mut shown = terms.clone();
        shown.extend(terms_en.iter().filter(|t| !terms.contains(t)).cloned());
        self.update(id, |a| a.searched = shown);
        let t1 = Instant::now();
        let books = self.library.books();
        let (mut passages, stats) = if books.is_empty() {
            (Vec::new(), SearchStats::default())
        } else {
            let search = find_sources(&*self.library, &books, &terms, &terms_en, question, language, safety);
            self.unless_stopped(id, search).await?
        };
        self.update(id, |a| {
            a.lookups = stats.lookups;
            a.lookups_cached = stats.cached;
            a.articles_read = stats.reads;
            a.search_cut = stats.cut;
        });
        // For a health question the library's text is preferred to unchecked web pages.
        if online && !(safety && !passages.is_empty()) {
            let web = self.unless_stopped(id, self.find_web_sources(question, &plan.terms, passages.len())).await?;
            passages.extend(web);
        }
        let search_ms = ms(t1);
        info!(ms = search_ms, sources = passages.len(), lookups = stats.lookups, cached = stats.cached, reads = stats.reads, cut = stats.cut, "assistant: library search");
        self.stop_requested(id)?;

        // A health question with nothing checked to go on gets a fixed, safe
        // reply rather than one from the model's memory.
        if safety && passages.is_empty() {
            let text = fixed_reply(language, &direct);
            self.update(id, |a| {
                a.search_ms = search_ms;
                a.text = text;
                a.fixed = true;
                a.grounded = false;
                a.status = AnswerStatus::Done;
            });
            return Ok(());
        }
        let sources: Vec<Source> = passages.iter().map(|p| p.source.clone()).collect();
        self.update(id, |a| {
            a.search_ms = search_ms;
            // Decided again from the citations once the answer is written.
            a.grounded = !sources.is_empty();
            a.sources = sources;
            a.status = AnswerStatus::Thinking;
        });
        // Reading the sources is most of the wait for the first word: they
        // get as much text as this computer reads in `READ_TIME`, the best
        // source the most, each its paragraphs about the question first.
        trim_sources(&mut passages, &trim_stems(&terms, &terms_en, question), self.source_budget());
        // Keep the whole prompt inside the engine's context.
        let used = question.chars().count()
            + history.iter().map(|t| t.question.chars().count() + t.answer.chars().count()).sum::<usize>()
            + known.iter().map(|n| n.chars().count() + 3).sum::<usize>()
            + 1800;
        fit_passages(&mut passages, PROMPT_CHARS.saturating_sub(used));
        let source_chars = passages.iter().map(|p| p.text.chars().count()).sum::<usize>();
        self.update(id, |a| a.source_chars = source_chars as u32);
        let finish = Finish {
            library: !passages.is_empty(),
            safety,
            web_only: !passages.is_empty() && passages.iter().all(|p| p.source.web),
            notes: if safety { direct } else { Vec::new() },
        };
        let messages = with_notes(build_messages(question, language, &passages, history, safety), &known, language, !safety);
        let t2 = Instant::now();
        let slot = if safety { SLOT_HEALTH } else { SLOT_ANSWER };
        let r = self.stream_answer(id, port, slot, messages, language, finish).await;
        info!(ms = t2.elapsed().as_millis() as u64, "assistant: answer written");
        r
    }
}

/// When a question should stop, and why: someone cancelled it, its time is
/// up, or nobody has looked at it for a while and another question that
/// somebody waits for is in line. A question whose asker went away (a closed
/// window, an evaluation that gave up) never holds up the next one, and an
/// answer for someone who only switched away for a moment is still written.
fn stop_reason(answers: &HashMap<String, Answer>, id: &str, now: Instant) -> Option<&'static str> {
    let Some(a) = answers.get(id) else { return Some(CANCELLED) };
    if a.cancel {
        return Some(CANCELLED);
    }
    if a.deadline.is_some_and(|d| now >= d) {
        return Some(TOO_LONG);
    }
    if abandoned(a, now) && answers.values().any(|o| o.id != a.id && !o.started && waited_for(o, now)) {
        return Some(CANCELLED);
    }
    None
}

fn finished(a: &Answer) -> bool {
    matches!(a.status, AnswerStatus::Done | AnswerStatus::Failed)
}

fn abandoned(a: &Answer, now: Instant) -> bool {
    now.saturating_duration_since(a.seen) >= ABANDONED
}

/// Not finished, not being stopped, and somebody still looks at it.
fn waited_for(a: &Answer, now: Instant) -> bool {
    !finished(a) && !a.cancel && !abandoned(a, now)
}

/// Milliseconds since `t`.
fn ms(t: Instant) -> u64 {
    t.elapsed().as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::test_util::test_assistant;

    #[test]
    fn questions_stop_when_cancelled_late_or_abandoned_for_someone_else() {
        let t0 = Instant::now();
        let at = |s: u64| t0 + Duration::from_secs(s);
        let mut answers: HashMap<String, Answer> = HashMap::new();
        let mut running = Answer::new("a".into(), "q".into(), "sr", false, t0);
        running.started = true;
        running.deadline = Some(at(360));
        answers.insert("a".into(), running);
        assert_eq!(stop_reason(&answers, "a", at(10)), None);
        assert_eq!(stop_reason(&answers, "gone", at(10)), Some(CANCELLED), "an answer no longer kept");
        // Nobody has looked at it for a minute, but nobody else waits: it goes
        // on (the app does not ask while it is in the background).
        assert_eq!(stop_reason(&answers, "a", at(61)), None);
        // Someone asks another question: the abandoned one gives way...
        answers.insert("b".into(), Answer::new("b".into(), "q2".into(), "sr", false, at(70)));
        assert_eq!(stop_reason(&answers, "a", at(71)), Some(CANCELLED));
        // ...but not while its asker still looks at it.
        answers.get_mut("a").unwrap().seen = at(65);
        assert_eq!(stop_reason(&answers, "a", at(71)), None);
        assert_eq!(stop_reason(&answers, "b", at(200)), None, "nobody waits behind the waiting one");
        assert_eq!(stop_reason(&answers, "a", at(360)), Some(TOO_LONG), "its time is up");
        answers.get_mut("b").unwrap().cancel = true;
        assert_eq!(stop_reason(&answers, "b", at(71)), Some(CANCELLED));
        assert_eq!(stop_reason(&answers, "a", at(200)), None, "a question being stopped is not waited for");
    }

    /// The answer once it is finished (a few seconds at most here).
    async fn finished_answer(ai: &Assistant, id: &str) -> Answer {
        for _ in 0..100 {
            let a = ai.answer(id).unwrap();
            if finished(&a) {
                return a;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("{id} did not finish");
    }

    #[tokio::test]
    async fn abandoned_questions_give_way_and_never_make_the_assistant_busy() {
        let ai = test_assistant();
        let long_ago = || Instant::now() - ABANDONED - Duration::from_secs(1);
        // A question is being answered.
        let turn = ai.turn.lock().await;
        let ask = |q: &str| ai.ask(q, "sr", AskContext::default());
        let first = ask("Koliko traje hleb?").unwrap();
        let second = ask("Kako se čuva mleko?").unwrap();
        // Nobody asks about the first any more (a window closed, an evaluation gave up).
        ai.update(&first, |a| a.seen = long_ago());
        let left = finished_answer(&ai, &first).await;
        assert_eq!((left.status, left.error.as_deref()), (AnswerStatus::Failed, Some(CANCELLED)), "it left the line");
        assert!(!finished(&ai.poll(&second).unwrap()), "the other one still waits for its turn");

        // Only questions somebody waits for make the assistant busy.
        let more: Vec<String> = ["a?", "b?", "c?"].iter().map(|q| ask(q).unwrap()).collect();
        assert!(ask("d?").unwrap_err().contains("busy"));
        ai.update(&more[2], |a| a.seen = long_ago());
        let fresh = ask("d?").expect("an abandoned question does not count");

        // A cancelled question leaves the line at once, not when its turn comes.
        assert!(ai.cancel(&more[0]));
        assert_eq!(finished_answer(&ai, &more[0]).await.error.as_deref(), Some(CANCELLED));
        assert_eq!(finished_answer(&ai, &more[2]).await.error.as_deref(), Some(CANCELLED), "abandoned, with others in line");

        // The rest get their turn, in order (and fail at once: there is no AI model here).
        drop(turn);
        for id in [&second, &more[1], &fresh] {
            let a = finished_answer(&ai, id).await;
            assert_eq!(a.error.as_deref(), Some("no AI model is installed"));
        }
    }
}
