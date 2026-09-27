//! The household assistant on the hub. Runs `llama-server` (llama.cpp, MIT,
//! a separate program) on 127.0.0.1 with the chosen model, only while it is
//! being used, and answers questions from the library: it searches the
//! installed knowledge packs, gives the model the best passages, and asks it
//! to answer only from them and name its sources. Small models invent facts
//! when left alone; grounding is the point.
//!
//! Answers are produced in the background and read by polling, which works
//! the same for the laptop window and for phones.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::process::{Child, Command};
use tracing::{info, warn};
use zaklon_core::catalog::{Category, PackStatus};

use zaklon_core::memory::Note;
use zaklon_core::supplies::Item;

use crate::downloads::Downloads;
use crate::kiwix::Library;

/// Stop the engine after this long without questions, to give the memory back.
const IDLE_STOP: Duration = Duration::from_secs(20 * 60);
const START_TIMEOUT: Duration = Duration::from_secs(180);
/// Database setting that remembers the chosen model.
pub const SETTING_MODEL: &str = "assistant_model";
/// Characters of each source passage given to the model.
const SOURCE_CHARS: usize = 1800;
const MAX_SOURCES: usize = 3;
const KEEP_ANSWERS: usize = 30;
/// Questions allowed to wait for their turn (the whole household, not a crowd).
const MAX_PENDING: usize = 4;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EngineState {
    /// The AI engine add-on is not installed.
    Missing,
    /// No AI model is installed.
    NoModel,
    /// Ready to start on the first question.
    Stopped,
    Starting,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct Source {
    pub n: usize,
    pub title: String,
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
    pub proposal: Option<Proposal>,
    /// False when no library passage was found and the model answered alone.
    pub grounded: bool,
    pub language: &'static str,
    pub tokens_per_second: f64,
    pub error: Option<String>,
    #[serde(skip)]
    created: Instant,
}

/// A change to the supplies the assistant proposes. Nothing changes until
/// someone confirms it in the app, which then calls the normal supplies API.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Proposal {
    /// "add", "use" or "shopping".
    pub action: String,
    /// The existing item it applies to, if one matches.
    pub item_id: Option<String>,
    /// The item's name as stored, or the new name.
    pub name: String,
    pub quantity: f64,
    pub unit: String,
    pub category: String,
    /// How much is in stock now (existing items).
    pub current: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Turn {
    pub question: String,
    pub answer: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelChoice {
    pub id: String,
    pub title_en: String,
    pub title_sr: String,
    pub size: u64,
    pub installed: bool,
    pub recommended: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Overview {
    pub engine: EngineState,
    pub engine_installed: bool,
    pub selected: Option<String>,
    pub recommended: String,
    pub ram_total: u64,
    pub models: Vec<ModelChoice>,
    pub books: usize,
}

struct Running {
    child: Child,
    model: String,
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
    /// Questions waiting or being answered.
    pending: std::sync::atomic::AtomicUsize,
    /// Set by `stop()` to end a model load in progress.
    cancel_load: std::sync::atomic::AtomicBool,
    #[cfg(windows)]
    job: crate::kiwix::job::Job,
    http: reqwest::Client,
}

/// Which model fits this computer's memory. The model, its context and the
/// rest of the system all have to fit.
pub fn recommended_model(ram_total: u64) -> &'static str {
    const GIB: u64 = 1 << 30;
    if ram_total >= 11 * GIB {
        "qwen35-4b"
    } else if ram_total >= 5 * GIB {
        "qwen35-2b"
    } else {
        "qwen35-08b"
    }
}

/// Model size order, smallest first, for picking a fallback.
const MODEL_ORDER: [&str; 3] = ["qwen35-08b", "qwen35-2b", "qwen35-4b"];

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
            pending: std::sync::atomic::AtomicUsize::new(0),
            cancel_load: std::sync::atomic::AtomicBool::new(false),
            #[cfg(windows)]
            job: crate::kiwix::job::Job::new(),
            http: reqwest::Client::builder().no_proxy().connect_timeout(Duration::from_secs(5)).build().expect("http client"),
        })
    }

    /// Background upkeep; call once inside the runtime.
    pub fn start(self: &Arc<Self>) {
        // Give the memory back when nobody has asked anything for a while.
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                let Some(me) = weak.upgrade() else { break };
                let idle = me.epoch.elapsed().as_secs().saturating_sub(me.last_used.load(Ordering::Relaxed));
                if idle >= IDLE_STOP.as_secs() && me.running.lock().await.is_some() {
                    info!("assistant idle; stopping the AI engine");
                    me.stop().await;
                }
            }
        });
    }

    fn exe(&self) -> PathBuf {
        self.engine_dir.join(if cfg!(windows) { "llama-server.exe" } else { "llama-server" })
    }

    fn set_state(&self, s: EngineState) {
        *self.state.lock().unwrap_or_else(|p| p.into_inner()) = s;
    }

    fn installed_models(&self) -> Vec<String> {
        MODEL_ORDER.iter().filter(|id| self.downloads.is_installed(id)).map(|s| s.to_string()).collect()
    }

    /// The chosen model if installed, else the recommended one, else the
    /// biggest installed model not above the recommendation, else any.
    pub fn selected(&self) -> Option<String> {
        let installed = self.installed_models();
        if let Some(id) = self.chosen.lock().unwrap_or_else(|p| p.into_inner()).clone() {
            if installed.contains(&id) {
                return Some(id);
            }
        }
        let rec = recommended_model(crate::machine::hardware().ram_total);
        if installed.iter().any(|m| m == rec) {
            return Some(rec.to_string());
        }
        let rec_rank = MODEL_ORDER.iter().position(|m| *m == rec).unwrap_or(0);
        installed
            .iter()
            .rfind(|m| MODEL_ORDER.iter().position(|x| x == m).unwrap_or(0) <= rec_rank)
            .or(installed.first())
            .cloned()
    }

    pub fn select(&self, id: &str) -> Result<(), String> {
        if !MODEL_ORDER.contains(&id) {
            return Err("unknown model".into());
        }
        *self.chosen.lock().unwrap_or_else(|p| p.into_inner()) = Some(id.to_string());
        Ok(())
    }

    pub fn engine_state(&self) -> EngineState {
        if !self.exe().is_file() {
            return EngineState::Missing;
        }
        if self.installed_models().is_empty() {
            return EngineState::NoModel;
        }
        *self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn overview(&self) -> Overview {
        let ram_total = crate::machine::hardware().ram_total;
        let rec = recommended_model(ram_total).to_string();
        let catalog = self.downloads.catalog();
        let models = MODEL_ORDER
            .iter()
            .filter_map(|id| catalog.pack(id))
            .map(|p| ModelChoice {
                id: p.id.clone(),
                title_en: p.title.en.clone(),
                title_sr: p.title.sr.clone(),
                size: p.size,
                installed: self.downloads.is_installed(&p.id),
                recommended: p.id == rec,
            })
            .collect();
        Overview {
            engine: self.engine_state(),
            engine_installed: self.exe().is_file(),
            selected: self.selected(),
            recommended: rec,
            ram_total,
            models,
            books: self.library.books().len(),
        }
    }

    fn model_path(&self, id: &str) -> Option<PathBuf> {
        let pack = self.downloads.catalog().pack(id)?;
        if pack.category != Category::Model || self.downloads.state_of(id).map(|s| s.status) != Some(PackStatus::Installed) {
            return None;
        }
        Some(self.downloads.library_dir().join(&pack.files.first()?.path))
    }

    pub async fn stop(&self) {
        // A model that is still loading holds the lock; ask it to give up first.
        self.cancel_load.store(true, Ordering::SeqCst);
        if let Some(mut r) = self.running.lock().await.take() {
            let _ = r.child.kill().await;
        }
        self.set_state(EngineState::Stopped);
    }

    /// Start the engine with the selected model (or keep it if it already runs it).
    async fn ensure_running(&self) -> Result<u16, String> {
        let model = self.selected().ok_or("no AI model is installed")?;
        let path = self.model_path(&model).ok_or("no AI model is installed")?;
        if !self.exe().is_file() {
            return Err("the AI engine is not installed".into());
        }
        let mut running = self.running.lock().await;
        if let Some(r) = running.as_mut() {
            let alive = matches!(r.child.try_wait(), Ok(None));
            if alive && r.model == model {
                return Ok(self.port.load(Ordering::Relaxed));
            }
            let _ = r.child.kill().await;
            *running = None;
        }
        let port = {
            let l = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
            l.local_addr().map_err(|e| e.to_string())?.port()
        };
        let mut cmd = Command::new(self.exe());
        cmd.arg("-m")
            .arg(&path)
            .args(["--host", "127.0.0.1", "--port", &port.to_string(), "-c", "8192", "-np", "1", "--jinja"])
            .current_dir(&self.engine_dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        self.set_state(EngineState::Starting);
        let child = cmd.spawn().map_err(|e| {
            self.set_state(EngineState::Failed);
            format!("could not start the AI engine: {e}")
        })?;
        #[cfg(windows)]
        if let Some(pid) = child.id() {
            self.job.adopt(pid);
        }
        info!(model = %model, port, "AI engine starting");
        *running = Some(Running { child, model });
        self.port.store(port, Ordering::Relaxed);

        // Loading a model takes from seconds to a minute or two.
        let deadline = Instant::now() + START_TIMEOUT;
        self.cancel_load.store(false, Ordering::SeqCst);
        loop {
            if self.cancel_load.load(Ordering::SeqCst) {
                if let Some(mut r) = running.take() {
                    let _ = r.child.kill().await;
                }
                self.set_state(EngineState::Stopped);
                return Err("the AI engine was stopped".into());
            }
            if let Some(r) = running.as_mut() {
                if !matches!(r.child.try_wait(), Ok(None)) {
                    *running = None;
                    self.set_state(EngineState::Failed);
                    return Err("the AI engine stopped while loading the model (not enough memory?)".into());
                }
            }
            let ok = self
                .http
                .get(format!("http://127.0.0.1:{port}/health"))
                .timeout(Duration::from_secs(3))
                .send()
                .await
                .is_ok_and(|r| r.status().is_success());
            if ok {
                self.set_state(EngineState::Ready);
                info!("AI engine ready");
                return Ok(port);
            }
            if Instant::now() > deadline {
                if let Some(mut r) = running.take() {
                    let _ = r.child.kill().await;
                }
                self.set_state(EngineState::Failed);
                return Err("the AI engine did not start in time".into());
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    pub fn answer(&self, id: &str) -> Option<Answer> {
        self.answers.lock().unwrap_or_else(|p| p.into_inner()).get(id).cloned()
    }

    fn update(&self, id: &str, f: impl FnOnce(&mut Answer)) {
        if let Some(a) = self.answers.lock().unwrap_or_else(|p| p.into_inner()).get_mut(id) {
            f(a);
        }
    }

    /// Start answering; the answer is read with `answer(id)`.
    pub fn ask(self: &Arc<Self>, question: &str, app_language: &str, history: Vec<Turn>, items: Vec<Item>, notes: Vec<Note>) -> Result<String, String> {
        let question = question.trim().to_string();
        if question.is_empty() {
            return Err("ask something first".into());
        }
        if question.chars().count() > 2000 {
            return Err("the question is too long".into());
        }
        if self.pending.load(Ordering::SeqCst) >= MAX_PENDING {
            return Err("the assistant is busy with other questions; try again in a moment".into());
        }
        let language = zaklon_core::lang::question_language(&question).unwrap_or(if app_language == "sr" { "sr" } else { "en" });
        let id = uuid::Uuid::new_v4().to_string();
        {
            let mut answers = self.answers.lock().unwrap_or_else(|p| p.into_inner());
            if answers.len() >= KEEP_ANSWERS {
                if let Some(oldest) = answers.values().min_by_key(|a| a.created).map(|a| a.id.clone()) {
                    answers.remove(&oldest);
                }
            }
            answers.insert(
                id.clone(),
                Answer {
                    id: id.clone(),
                    question: question.clone(),
                    status: AnswerStatus::Searching,
                    text: String::new(),
                    sources: Vec::new(),
                    searched: Vec::new(),
                    from_supplies: false,
                    proposal: None,
                    grounded: false,
                    language,
                    tokens_per_second: 0.0,
                    error: None,
                    created: Instant::now(),
                },
            );
        }
        self.last_used.store(self.epoch.elapsed().as_secs(), Ordering::Relaxed);
        let me = self.clone();
        let id2 = id.clone();
        self.pending.fetch_add(1, Ordering::SeqCst);
        tokio::spawn(async move {
            let _turn = me.turn.lock().await;
            if let Err(e) = me.run(&id2, &question, language, &history, &items, &notes).await {
                warn!("assistant: {e}");
                me.update(&id2, |a| {
                    a.status = AnswerStatus::Failed;
                    a.error = Some(e);
                });
            }
            me.last_used.store(me.epoch.elapsed().as_secs(), Ordering::Relaxed);
            me.pending.fetch_sub(1, Ordering::SeqCst);
        });
        Ok(id)
    }

    async fn run(&self, id: &str, question: &str, language: &'static str, history: &[Turn], items: &[Item], notes: &[Note]) -> Result<(), String> {
        // 1. Make sure the engine runs (it also decides what the question is about).
        self.update(id, |a| a.status = AnswerStatus::Starting);
        let port = self.ensure_running().await?;
        self.update(id, |a| a.status = AnswerStatus::Searching);
        let mut plan = self.plan(port, question, language).await;
        if plan.kind == "library" && mentions_supplies(question) {
            plan.kind = "supplies_question".into();
        }
        if let Some(note) = remember_request(question) {
            if plan.kind != "remember" || plan.note.trim().is_empty() {
                plan.kind = "remember".into();
                plan.note = note;
            }
        }

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
        let known = relevant_notes(notes, question, &plan.terms);

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
            let context = supplies_context(items, &plan.terms, language);
            self.update(id, |a| {
                a.from_supplies = true;
                a.grounded = true;
                a.searched = plan.terms.clone();
                a.status = AnswerStatus::Thinking;
            });
            let messages = with_notes(supplies_messages(question, language, &context, history), &known, language);
            return self.stream_answer(id, port, messages, language).await;
        }

        // 2c. Everything else: find passages in the library, if there is one.
        let (sources, passages) = if self.library.books().is_empty() {
            (Vec::new(), Vec::new())
        } else {
            let mut terms = plan.terms.clone();
            if terms.is_empty() {
                // The model gave nothing usable: fall back to the question's own words.
                terms = search_words(question).iter().map(|w| stem(w)).collect();
            }
            let shown = terms.clone();
            self.update(id, |a| a.searched = shown);
            self.find_sources(&terms, question).await
        };
        let grounded = !sources.is_empty();
        self.update(id, |a| {
            a.sources = sources.clone();
            a.grounded = grounded;
            a.status = AnswerStatus::Thinking;
        });
        let messages = with_notes(build_messages(question, language, &passages, history), &known, language);
        self.stream_answer(id, port, messages, language).await
    }

    /// Ask the model, streaming its text into the answer as it comes.
    async fn stream_answer(&self, id: &str, port: u16, messages: Vec<serde_json::Value>, language: &'static str) -> Result<(), String> {
        let body = serde_json::json!({
            "messages": messages,
            "stream": true,
            "max_tokens": 450,
            "temperature": 0.3,
            "repeat_penalty": 1.1,
            "chat_template_kwargs": { "enable_thinking": false },
        });
        let res = self
            .http
            .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("AI engine: {e}"))?;
        if !res.status().is_success() {
            return Err(format!("AI engine replied {}", res.status()));
        }
        let mut stream = res.bytes_stream();
        // Bytes, not text: a letter like "č" can be split between two chunks.
        let mut buf: Vec<u8> = Vec::new();
        let mut tokens = 0u64;
        let mut first: Option<Instant> = None;
        let stall = Duration::from_secs(120);
        loop {
            let next = tokio::time::timeout(stall, stream.next()).await.map_err(|_| "the AI engine stopped answering".to_string())?;
            let Some(chunk) = next else { break };
            let chunk = chunk.map_err(|e| format!("AI engine: {e}"))?;
            buf.extend_from_slice(&chunk);
            while let Some(nl) = buf.iter().position(|b| *b == b'\n') {
                let raw: Vec<u8> = buf.drain(..=nl).collect();
                let line = String::from_utf8_lossy(&raw);
                let Some(data) = line.trim().strip_prefix("data:") else { continue };
                let data = data.trim();
                if data == "[DONE]" {
                    continue;
                }
                let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else { continue };
                if let Some(piece) = v["choices"][0]["delta"]["content"].as_str() {
                    if !piece.is_empty() {
                        first.get_or_insert_with(Instant::now);
                        tokens += 1;
                        let piece = piece.to_string();
                        let rate = first.map(|f| tokens as f64 / f.elapsed().as_secs_f64().max(0.001)).unwrap_or(0.0);
                        self.update(id, |a| {
                            a.text.push_str(&piece);
                            a.tokens_per_second = rate;
                        });
                    }
                }
            }
        }
        self.update(id, |a| {
            a.text = finish_text(&a.text, language);
            // Show only the sources the answer actually used, when it cites any.
            let cited = cited_numbers(&a.text);
            if !cited.is_empty() {
                a.sources.retain(|s| cited.contains(&s.n));
            }
            a.status = AnswerStatus::Done;
        });
        Ok(())
    }

    /// What the question is about, decided by the model in a fixed JSON
    /// shape (the engine enforces the schema): a library question with its
    /// search terms in basic form ("konzerva, pasulj, rok trajanja"), a
    /// question about the supplies, or a change to them.
    async fn plan(&self, port: u16, question: &str, language: &str) -> Plan {
        let sr = language == "sr";
        let prompt = if sr {
            "Odluči o čemu je poruka i odgovori samo JSON-om.\n\
kind: \"library\" za opšta pitanja (zdravlje, hrana, popravke, priroda...), \"supplies_question\" za pitanja o zalihama u kući \
(šta imam, koliko imam, šta ističe, šta treba kupiti), \"supplies_change\" kad treba dodati, potrošiti ili staviti na listu za kupovinu.\n\
\"remember\" kad treba nešto zapamtiti (note je ta činjenica kao rečenica o domaćinstvu).\n\
terms: 2 do 4 pojma za pretragu enciklopedije ili zaliha, imenice u osnovnom obliku, na srpskom, latinicom.\n\
change (samo za supplies_change): action je \"add\" (dodaj u zalihe), \"use\" (potrošeno) ili \"shopping\" (na listu za kupovinu); \
name je naziv stvari u osnovnom obliku; quantity je broj (0 ako nije rečeno); unit je pcs, kg, g, l, ml ili pack; \
category je food, drink, medicine, hygiene, equipment, fuel ili other."
        } else {
            "Decide what the message is about and answer only with JSON.\n\
kind: \"library\" for general questions (health, food, repairs, nature...), \"supplies_question\" for questions about the household's supplies \
(what do I have, how much, what expires, what to buy), \"supplies_change\" to add, use up or put something on the shopping list.\n\
\"remember\" when something should be remembered (note is that fact as a sentence about the household).\n\
terms: 2 to 4 terms to search the encyclopedia or the supplies, nouns in their basic form.\n\
change (only for supplies_change): action is \"add\", \"use\" or \"shopping\"; name is the thing in its basic form; \
quantity is a number (0 if not said); unit is pcs, kg, g, l, ml or pack; category is food, drink, medicine, hygiene, equipment, fuel or other."
        };
        let examples: [(&str, &str); 7] = if sr {
            [
                ("Koliko dugo traje hleb?", r#"{"kind":"library","terms":["hleb","rok trajanja"]}"#),
                ("Kako da izlečim prehladu kod deteta?", r#"{"kind":"library","terms":["prehlada","lečenje","dete"]}"#),
                ("Koliko imam brašna?", r#"{"kind":"supplies_question","terms":["brašno"]}"#),
                ("Šta imam u zalihama?", r#"{"kind":"supplies_question","terms":[]}"#),
                ("Dodaj 2 litra mleka", r#"{"kind":"supplies_change","terms":["mleko"],"change":{"action":"add","name":"mleko","quantity":2,"unit":"l","category":"drink"}}"#),
                ("Potrošili smo 3 konzerve pasulja", r#"{"kind":"supplies_change","terms":["pasulj"],"change":{"action":"use","name":"pasulj","quantity":3,"unit":"pcs","category":"food"}}"#),
                ("Zapamti da je Marko alergičan na orahe", r#"{"kind":"remember","terms":[],"note":"Marko je alergičan na orahe."}"#),
            ]
        } else {
            [
                ("How long does bread last?", r#"{"kind":"library","terms":["bread","shelf life"]}"#),
                ("How do I treat a cold in a child?", r#"{"kind":"library","terms":["common cold","treatment","child"]}"#),
                ("How much flour do we have?", r#"{"kind":"supplies_question","terms":["flour"]}"#),
                ("What do we have in the supplies?", r#"{"kind":"supplies_question","terms":[]}"#),
                ("Add 2 liters of milk", r#"{"kind":"supplies_change","terms":["milk"],"change":{"action":"add","name":"milk","quantity":2,"unit":"l","category":"drink"}}"#),
                ("We used 3 cans of beans", r#"{"kind":"supplies_change","terms":["beans"],"change":{"action":"use","name":"beans","quantity":3,"unit":"pcs","category":"food"}}"#),
                ("Remember that Mark is allergic to walnuts", r#"{"kind":"remember","terms":[],"note":"Mark is allergic to walnuts."}"#),
            ]
        };
        let mut messages = vec![serde_json::json!({ "role": "system", "content": prompt })];
        for (q, a) in examples {
            messages.push(serde_json::json!({ "role": "user", "content": q }));
            messages.push(serde_json::json!({ "role": "assistant", "content": a }));
        }
        messages.push(serde_json::json!({ "role": "user", "content": question }));
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "kind": { "type": "string", "enum": ["library", "supplies_question", "supplies_change", "remember"] },
                "note": { "type": "string" },
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
            "required": ["kind", "terms"]
        });
        let body = serde_json::json!({
            "messages": messages,
            "max_tokens": 120,
            "temperature": 0.1,
            "chat_template_kwargs": { "enable_thinking": false },
            "response_format": { "type": "json_schema", "json_schema": { "name": "plan", "schema": schema } },
        });
        let reply = self
            .http
            .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
            .timeout(Duration::from_secs(90))
            .json(&body)
            .send()
            .await;
        let text = match reply {
            Ok(r) => r.json::<serde_json::Value>().await.ok().and_then(|v| v["choices"][0]["message"]["content"].as_str().map(str::to_string)),
            Err(_) => None,
        }
        .unwrap_or_default();
        parse_plan(&text)
    }

    /// The best few library passages for some search terms. Each term is
    /// looked up on its own, and articles are ranked by how well their title
    /// and text match the terms.
    async fn find_sources(&self, terms: &[String], question: &str) -> (Vec<Source>, Vec<String>) {
        if terms.is_empty() {
            return (Vec::new(), Vec::new());
        }
        let stems: Vec<String> = terms
            .iter()
            .flat_map(|t| t.split_whitespace().map(|w| zaklon_core::translit::fold(&stem(w))).collect::<Vec<_>>())
            .filter(|w| w.chars().count() >= 3)
            .collect();
        let whole: Vec<String> = terms.iter().map(|t| zaklon_core::translit::fold(t)).collect();
        // Passages are chosen by the terms and by the question's own words
        // ("treat", "leči"), so the practical parts of an article win.
        let mut passage_stems = stems.clone();
        for w in search_words(question) {
            let f = zaklon_core::translit::fold(&stem(&w));
            if f.chars().count() >= 3 && !passage_stems.contains(&f) {
                passage_stems.push(f);
            }
        }
        let mut queries: Vec<String> = terms.iter().take(4).cloned().collect();
        // "ubod pčele" is also looked up as "ubod" and "pčel(a)".
        for t in terms.iter().take(4) {
            let words: Vec<&str> = t.split_whitespace().collect();
            if words.len() > 1 {
                for w in words {
                    let st = stem(w);
                    if st.chars().count() >= 4 && !queries.contains(&st) {
                        queries.push(st);
                    }
                }
            }
        }
        queries.truncate(8);

        // (score, order found, result)
        let mut found: Vec<(i32, usize, crate::kiwix::SearchResult)> = Vec::new();
        for q in &queries {
            for r in self.library.search(q, None, 5).await {
                if found.iter().any(|(_, _, f)| f.url == r.url) {
                    continue;
                }
                let title = zaklon_core::translit::fold(&r.title);
                let text = zaklon_core::translit::fold(&format!("{} {}", r.title, r.snippet));
                let mut score = 0;
                // The title is one of the terms: the article is about exactly this.
                if whole.contains(&title) {
                    score += 6;
                }
                for st in &stems {
                    if title == *st || title.starts_with(st.as_str()) && title.chars().count() <= st.chars().count() + 3 {
                        score += 4; // the article is about this word
                    } else if text.contains(st.as_str()) {
                        score += 1;
                    }
                }
                // Disambiguation and list pages rarely help, and a dictionary
                // entry only explains the word.
                if title.contains("вишезначн") || title.contains("списак") {
                    score -= 3;
                }
                let book = r.book.to_lowercase();
                if book.contains("wiktionary") || book.contains("dictionary") {
                    score -= 3;
                }
                // The same title from another book adds nothing.
                if found.iter().any(|(_, _, f)| zaklon_core::translit::fold(&f.title) == title) {
                    continue;
                }
                let order = found.len();
                found.push((score, order, r));
            }
        }
        found.retain(|(score, _, _)| *score > 0);
        found.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

        let mut sources = Vec::new();
        let mut passages = Vec::new();
        for (_, _, r) in found {
            if sources.len() >= MAX_SOURCES {
                break;
            }
            let Ok(res) = self.library.fetch(&r.url).await else { continue };
            if !res.status().is_success() {
                continue;
            }
            let Ok(html) = res.text().await else { continue };
            let st = passage_stems.clone();
            let text = tokio::task::spawn_blocking(move || relevant_text(&html, &st, SOURCE_CHARS)).await.unwrap_or_default();
            if text.chars().count() < 80 {
                continue; // a redirect or an almost empty page
            }
            let n = sources.len() + 1;
            let title = zaklon_core::translit::cyrillic_to_latin(&r.title);
            passages.push(format!("[{n}] {title}\n{text}"));
            sources.push(Source { n, title, url: r.url, book_title_en: r.book_title_en, book_title_sr: r.book_title_sr });
        }
        (sources, passages)
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
    #[serde(default)]
    pub change: Option<PlannedChange>,
    /// For "remember": the fact, as a sentence about the household.
    #[serde(default)]
    pub note: String,
}

/// The model's JSON; anything unusable becomes a library question with the
/// words it wrote as search terms.
pub fn parse_plan(text: &str) -> Plan {
    let t = text.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
    match serde_json::from_str::<Plan>(t) {
        Ok(mut p) => {
            p.terms = p
                .terms
                .iter()
                .map(|x| zaklon_core::translit::cyrillic_to_latin(x.trim()).to_lowercase())
                .filter(|x| !x.is_empty() && x.chars().count() <= 40)
                .fold(Vec::new(), |mut acc: Vec<String>, x| {
                    if !acc.contains(&x) {
                        acc.push(x);
                    }
                    acc
                });
            p.terms.truncate(4);
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
        Err(_) => Plan { kind: "library".into(), terms: parse_keywords(text), change: None, note: String::new() },
    }
}

/// "Zapamti da je Ana alergična na penicilin" -> "Ana je alergična na penicilin."
pub fn remember_request(question: &str) -> Option<String> {
    let q = question.trim();
    let lower = q.to_lowercase();
    const STARTS: &[&str] = &["zapamti da ", "zapamti: ", "zapamti ", "upamti da ", "upamti ", "remember that ", "remember: ", "remember "];
    for s in STARTS {
        if lower.starts_with(s) {
            let rest = q[s.len()..].trim().trim_end_matches(['.', '!']);
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

/// The notes that matter for a question: those sharing a word with it, or
/// all of them when there are only a few. At most eight.
pub fn relevant_notes(notes: &[Note], question: &str, terms: &[String]) -> Vec<String> {
    if notes.len() <= 8 {
        return notes.iter().map(|n| n.text.clone()).collect();
    }
    let mut words: Vec<String> = search_words(question).iter().map(|w| stem(&plain(w))).collect();
    words.extend(terms.iter().flat_map(|t| t.split_whitespace().map(|w| stem(&plain(w))).collect::<Vec<_>>()));
    words.retain(|w| w.chars().count() >= 3);
    notes
        .iter()
        .filter(|n| {
            let text = plain(&n.text);
            words.iter().any(|w| text.contains(w.as_str()))
        })
        .take(8)
        .map(|n| n.text.clone())
        .collect()
}

/// Put what the household asked to remember in front of the question, with
/// a rule to take it into account first. Small models skip a note that sits
/// quietly in the instructions ("Ana is allergic to penicillin" matters more
/// than any encyclopedia article about penicillin).
fn with_notes(mut messages: Vec<serde_json::Value>, notes: &[String], language: &str) -> Vec<serde_json::Value> {
    if notes.is_empty() {
        return messages;
    }
    let sr = language == "sr";
    let rule = if sr {
        "Beleške domaćinstva su proverene činjenice o ovoj porodici. Ako se neka beleška tiče pitanja, uzmi je u obzir pre svega i pomeni je na početku odgovora."
    } else {
        "The household notes are checked facts about this family. If a note matters for the question, take it into account before anything else and mention it at the start of the answer."
    };
    let head = if sr { "Beleške domaćinstva:" } else { "Household notes:" };
    let list = notes.iter().map(|n| format!("- {n}")).collect::<Vec<_>>().join("
");
    if let Some(sys) = messages.first_mut() {
        let content = sys["content"].as_str().unwrap_or_default().to_string();
        sys["content"] = serde_json::Value::String(format!("{content}
{rule}"));
    }
    if let Some(user) = messages.last_mut() {
        let content = user["content"].as_str().unwrap_or_default().to_string();
        user["content"] = serde_json::Value::String(format!("{head}
{list}

{content}"));
    }
    messages
}

/// Questions that are clearly about the household's own supplies, whatever
/// the model thought ("Šta imam u zalihama?", "What do we have?").
pub fn mentions_supplies(question: &str) -> bool {
    let q = plain(question);
    const MARKS: &[&str] = &[
        "zalih", "u kuci imam", "sta imam", "koliko imam", "imamo li", "da li imam", "sta imamo", "koliko imamo", "istice", "isticu", "istekl",
        "lista za kupovinu", "listu za kupovinu", "listi za kupovinu", "ponestaje", "supplies", "pantry", "do we have", "do i have", "how much do we",
        "shopping list", "expire", "running low",
    ];
    MARKS.iter().any(|m| q.contains(m))
}

/// The stored item a spoken name most likely means ("mleka" -> "Mleko 2,8%").
pub fn match_item<'a>(name: &str, items: &'a [Item]) -> Option<&'a Item> {
    let want = plain(&stem(&plain(name)));
    if want.chars().count() < 2 {
        return None;
    }
    let full = plain(name);
    items
        .iter()
        .filter_map(|i| {
            let n = plain(&i.name);
            let score = if n == full {
                3
            } else if n.split_whitespace().any(|w| w.starts_with(&want)) {
                2
            } else if n.contains(&want) {
                1
            } else {
                0
            };
            (score > 0).then_some((score, i))
        })
        .max_by_key(|(score, i)| (*score, std::cmp::Reverse(i.name.len())))
        .map(|(_, i)| i)
}

/// Lower-case Latin without diacritics, for matching what people type
/// ("brasno") with what is stored ("Brašno", "Брашно").
fn plain(s: &str) -> String {
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

fn unit_text(unit: &str, sr: bool) -> &str {
    match (unit, sr) {
        ("pcs", true) => "kom",
        ("pcs", false) => "pcs",
        ("pack", true) => "pak.",
        ("pack", false) => "packs",
        (u, _) => u,
    }
}

fn qty_text(q: f64) -> String {
    if (q - q.round()).abs() < 1e-9 {
        format!("{}", q.round() as i64)
    } else {
        format!("{q:.2}").trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// What the assistant offers to change, in words, and the change itself.
pub fn propose(change: &PlannedChange, items: &[Item], language: &str) -> (String, Option<Proposal>) {
    let sr = language == "sr";
    let found = match_item(&change.name, items);
    let qty = if change.quantity > 0.0 { change.quantity } else { 1.0 };
    match change.action.as_str() {
        "use" => {
            let Some(item) = found else {
                let text = if sr {
                    format!("U zalihama nemam ništa što liči na „{}“.", change.name)
                } else {
                    format!("I found nothing like \"{}\" in the supplies.", change.name)
                };
                return (text, None);
            };
            let used = qty.min(item.quantity);
            let unit = unit_text(&item.unit, sr);
            let text = if sr {
                format!("Da skinem {} {unit} sa „{}“? Sada ima {} {unit}.", qty_text(used), item.name, qty_text(item.quantity))
            } else {
                format!("Take {} {unit} off \"{}\"? There are {} {unit} now.", qty_text(used), item.name, qty_text(item.quantity))
            };
            let p = Proposal {
                action: "use".into(),
                item_id: Some(item.id.clone()),
                name: item.name.clone(),
                quantity: used,
                unit: item.unit.clone(),
                category: item.category.clone(),
                current: Some(item.quantity),
            };
            (text, Some(p))
        }
        "add" => {
            let (item_id, name, unit, category, current) = match found {
                Some(i) => (Some(i.id.clone()), i.name.clone(), i.unit.clone(), i.category.clone(), Some(i.quantity)),
                None => (None, capitalize(&change.name), change.unit.clone(), change.category.clone(), None),
            };
            let u = unit_text(&unit, sr);
            let text = match (current, sr) {
                (Some(c), true) => format!("Da dodam {} {u} u „{name}“? Sada ima {} {u}.", qty_text(qty), qty_text(c)),
                (Some(c), false) => format!("Add {} {u} to \"{name}\"? There are {} {u} now.", qty_text(qty), qty_text(c)),
                (None, true) => format!("Da dodam novu stavku „{name}“, {} {u}?", qty_text(qty)),
                (None, false) => format!("Add a new item \"{name}\", {} {u}?", qty_text(qty)),
            };
            let p = Proposal { action: "add".into(), item_id, name, quantity: qty, unit, category, current };
            (text, Some(p))
        }
        _ => {
            let (item_id, name, unit) = match found {
                Some(i) => (Some(i.id.clone()), i.name.clone(), i.unit.clone()),
                None => (None, capitalize(&change.name), change.unit.clone()),
            };
            let u = unit_text(&unit, sr);
            let amount = if change.quantity > 0.0 { format!(", {} {u}", qty_text(change.quantity)) } else { String::new() };
            let text = if sr {
                format!("Da stavim „{name}“{amount} na listu za kupovinu?")
            } else {
                format!("Put \"{name}\"{amount} on the shopping list?")
            };
            let p = Proposal {
                action: "shopping".into(),
                item_id,
                name,
                quantity: change.quantity,
                unit,
                category: change.category.clone(),
                current: None,
            };
            (text, Some(p))
        }
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// The supplies, as the model sees them: matching items first, then what
/// expires, what runs low, and the rest, one line each.
pub fn supplies_context(items: &[Item], terms: &[String], language: &str) -> String {
    let sr = language == "sr";
    let today = chrono_today();
    let stems: Vec<String> = terms.iter().map(|t| stem(&plain(t))).filter(|t| t.chars().count() >= 2).collect();
    let matches = |i: &Item| {
        let n = plain(&i.name);
        stems.iter().any(|s| n.contains(s.as_str()))
    };
    let mut ordered: Vec<&Item> = items.iter().filter(|i| matches(i)).collect();
    let mut rest: Vec<&Item> = items.iter().filter(|i| !matches(i)).collect();
    rest.sort_by(|a, b| a.expiry.clone().unwrap_or_else(|| "9999".into()).cmp(&b.expiry.clone().unwrap_or_else(|| "9999".into())));
    ordered.extend(rest);
    let mut lines = vec![if sr { format!("Danas je {today}. Zalihe ({} stavki):", items.len()) } else { format!("Today is {today}. Supplies ({} items):", items.len()) }];
    for i in ordered.into_iter().take(80) {
        let mut l = format!("- {}: {} {}", i.name, qty_text(i.quantity), unit_text(&i.unit, sr));
        if let Some(e) = &i.expiry {
            l.push_str(&if sr { format!(", rok {e}") } else { format!(", expires {e}") });
        }
        if let Some(p) = &i.place {
            l.push_str(&if sr { format!(", mesto: {p}") } else { format!(", place: {p}") });
        }
        if let Some(m) = i.min_quantity {
            if i.quantity < m {
                l.push_str(if sr { ", ponestaje" } else { ", running low" });
            }
        }
        lines.push(l);
    }
    if items.is_empty() {
        lines.push(if sr { "(u zalihama još nema ničega)".into() } else { "(nothing in the supplies yet)".into() });
    }
    lines.join("\n")
}

fn chrono_today() -> String {
    // Days since 1970-01-01 to a civil date (no time zone database needed).
    let days = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) / 86_400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn supplies_messages(question: &str, language: &str, context: &str, history: &[Turn]) -> Vec<serde_json::Value> {
    let system = if language == "sr" {
        "Ti si Zaklon, pomoćnik za domaćinstvo. Odgovaraj na srpskom, latinicom, kratko i jasno. \
Koristi samo spisak zaliha ispod; ne izmišljaj stavke ni količine. Ako nečega nema na spisku, reci da toga nema u zalihama."
    } else {
        "You are Zaklon, a household assistant. Answer briefly and clearly. \
Use only the supplies list below; do not invent items or amounts. If something is not on the list, say it is not in the supplies."
    };
    let mut messages = vec![serde_json::json!({ "role": "system", "content": system })];
    for t in history.iter().rev().take(2).rev() {
        messages.push(serde_json::json!({ "role": "user", "content": t.question }));
        messages.push(serde_json::json!({ "role": "assistant", "content": t.answer }));
    }
    messages.push(serde_json::json!({ "role": "user", "content": format!("{context}\n\n{question}") }));
    messages
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
];
const STOP_EN: &[&str] = &[
    "the", "is", "are", "how", "what", "does", "do", "can", "why", "where", "which", "of", "to", "in", "and", "a", "an", "it", "should", "i", "my",
    "you", "when", "much", "many", "for", "with", "be", "on", "at", "by", "or", "if", "me", "we", "our", "this", "that", "there", "best", "way",
];

/// The meaningful words of a question, for the library search.
pub fn search_terms(question: &str) -> String {
    search_words(question).join(" ")
}

pub fn search_words(question: &str) -> Vec<String> {
    question
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 3 || w.chars().all(|c| c.is_ascii_digit()) && !w.is_empty())
        .filter(|w| {
            let l = w.to_lowercase();
            !STOP_SR.contains(&l.as_str()) && !STOP_EN.contains(&l.as_str())
        })
        .take(6)
        .map(str::to_string)
        .collect()
}

fn build_messages(question: &str, language: &str, passages: &[String], history: &[Turn]) -> Vec<serde_json::Value> {
    let sr = language == "sr";
    let system = if passages.is_empty() {
        if sr {
            "Ti si Zaklon, pomoćnik za domaćinstvo koji radi bez interneta. Odgovaraj na srpskom jeziku, latinicom, kratko i jasno. \
U biblioteci nije pronađen tekst o ovom pitanju, pa odgovaraš iz opšteg znanja: budi oprezan, ne izmišljaj brojeve i imena, \
i ako nisi siguran reci to. Za zdravlje i bezbednost savetuj proveru kod stručnjaka."
        } else {
            "You are Zaklon, a household assistant that works without internet. Answer briefly and clearly. \
Nothing about this was found in the library, so you answer from general knowledge: be careful, do not invent numbers or names, \
and say so when you are not sure. For health and safety, advise checking with a professional."
        }
    } else if sr {
        "Ti si Zaklon, pomoćnik za domaćinstvo koji radi bez interneta. Odgovaraj na srpskom jeziku, latinicom, kratko i jasno (najviše 6 rečenica). \
Koristi samo činjenice iz izvora ispod. Posle rečenice koja koristi izvor napiši njegov broj u uglastim zagradama, npr. [1]. \
Izvori koji nisu o pitanju se ne koriste. Ako izvori ne odgovaraju na pitanje, reci samo: „U biblioteci nisam našao pouzdan odgovor.“ \
Ne izmišljaj i ne tvrdi da nešto ne postoji ili ne može samo zato što toga nema u izvorima."
    } else {
        "You are Zaklon, a household assistant that works without internet. Answer briefly and clearly (at most 6 sentences). \
Use only facts from the sources below. After a sentence that uses a source, write its number in square brackets, like [1]. \
Ignore sources that are not about the question. If the sources do not answer the question, say only: \"I did not find a reliable answer in the library.\" \
Do not make things up, and do not claim something is impossible or does not exist just because the sources do not mention it."
    };
    let mut messages = vec![serde_json::json!({ "role": "system", "content": system })];
    for t in history.iter().rev().take(2).rev() {
        messages.push(serde_json::json!({ "role": "user", "content": t.question }));
        messages.push(serde_json::json!({ "role": "assistant", "content": t.answer }));
    }
    let user = if passages.is_empty() {
        question.to_string()
    } else {
        let label = if sr { "Izvori" } else { "Sources" };
        let q = if sr { "Pitanje" } else { "Question" };
        format!("{label}:\n\n{}\n\n{q}: {question}", passages.join("\n\n"))
    };
    messages.push(serde_json::json!({ "role": "user", "content": user }));
    messages
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

/// Serbian answers in Latin script, whatever the model wrote.
fn finish_text(text: &str, language: &str) -> String {
    // Drop lines that are only citation marks ("[1]") left at the end.
    let kept: Vec<&str> = text.lines().filter(|l| !l.trim().chars().all(|c| c == '[' || c == ']' || c == ',' || c == ' ' || c.is_ascii_digit()) || l.trim().is_empty()).collect();
    let joined = kept.join("
");
    let t = joined.trim();
    if language == "sr" && zaklon_core::translit::has_cyrillic(t) {
        zaklon_core::translit::cyrillic_to_latin(t)
    } else {
        t.to_string()
    }
}

/// The parts of an article that matter for the search words: the first
/// paragraph (what the thing is), then the paragraphs that mention the words
/// most, in article order, up to `max` characters, in Latin script.
pub fn relevant_text(html: &str, folded_stems: &[String], max: usize) -> String {
    let paras = paragraphs(html);
    if paras.is_empty() {
        return String::new();
    }
    let hits = |p: &str| {
        let f = zaklon_core::translit::fold(p);
        folded_stems.iter().filter(|st| f.contains(st.as_str())).count()
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

fn clip(text: &str, max: usize) -> String {
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
fn paragraphs(html: &str) -> Vec<String> {
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
    fn model_by_memory() {
        const GIB: u64 = 1 << 30;
        assert_eq!(recommended_model(16 * GIB), "qwen35-4b");
        assert_eq!(recommended_model(12 * GIB - GIB / 2), "qwen35-4b");
        assert_eq!(recommended_model(8 * GIB), "qwen35-2b");
        assert_eq!(recommended_model(4 * GIB), "qwen35-08b");
    }

    #[test]
    fn search_terms_keep_the_meaning() {
        assert_eq!(search_terms("Koliko dugo traje konzerva pasulja?"), "dugo traje konzerva pasulja");
        assert_eq!(search_terms("How long does canned food last?"), "long canned food last");
        assert_eq!(search_terms("Šta je?"), "");
    }

    #[test]
    fn keywords_are_parsed_from_what_the_model_writes() {
        assert_eq!(parse_keywords("konzerva, pasulj, rok trajanja"), vec!["konzerva", "pasulj", "rok trajanja"]);
        assert_eq!(parse_keywords("1. Voda\n2. Prečišćavanje vode\n"), vec!["voda", "prečišćavanje vode"]);
        assert_eq!(parse_keywords("\"Вода\", \"филтер\"."), vec!["voda", "filter"]);
        assert!(parse_keywords("").is_empty());
        assert_eq!(parse_keywords("so, so, so"), vec!["so"]);
        assert!(parse_keywords("This is a very long sentence that is not a keyword at all").is_empty());
    }

    fn item(name: &str, qty: f64, unit: &str) -> Item {
        serde_json::from_value(serde_json::json!({
            "id": format!("id-{name}"), "name": name, "quantity": qty, "unit": unit, "category": "food",
            "place": null, "expiry": null, "barcode": null, "min_quantity": null, "notes": null,
            "updated_at": "2026-09-28T00:00:00Z", "updated_by": null, "batches": []
        }))
        .unwrap()
    }

    #[test]
    fn remember_requests_become_notes() {
        assert_eq!(remember_request("Zapamti da je Ana alergična na penicilin").as_deref(), Some("Ana je alergična na penicilin."));
        assert_eq!(remember_request("zapamti: ključ od podruma je kod komšije").as_deref(), Some("Ključ od podruma je kod komšije."));
        assert_eq!(remember_request("Remember that the water tank holds 200 liters.").as_deref(), Some("The water tank holds 200 liters."));
        assert!(remember_request("Kako da zapamtim brojeve?").is_none());
    }

    #[test]
    fn notes_reach_the_prompt() {
        let note = |t: &str| Note { id: t.into(), text: t.into(), created_at: String::new(), created_by: None };
        let few = vec![note("Ana je alergična na penicilin.")];
        assert_eq!(relevant_notes(&few, "Šta da dam Ani za temperaturu?", &[]).len(), 1, "few notes: all of them");
        let many: Vec<Note> = (0..20).map(|i| note(&format!("Beleška broj {i} o nečemu."))).chain([note("Ana je alergična na penicilin.")]).collect();
        let r = relevant_notes(&many, "Da li Ana sme penicilin?", &[]);
        assert_eq!(r, vec!["Ana je alergična na penicilin."]);
        let m = with_notes(vec![serde_json::json!({"role":"system","content":"Base."}), serde_json::json!({"role":"user","content":"Pitanje?"})], &r, "sr");
        assert!(m[0]["content"].as_str().unwrap().contains("Beleške domaćinstva su proverene"));
        assert!(m[1]["content"].as_str().unwrap().starts_with("Beleške domaćinstva:
- Ana je alergična na penicilin.

Pitanje?"));
    }

    #[test]
    fn supply_words_are_recognised() {
        assert!(mentions_supplies("Šta imam u zalihama?"));
        assert!(mentions_supplies("sta mi istice ove nedelje"));
        assert!(mentions_supplies("What's on the shopping list?"));
        assert!(!mentions_supplies("Kako se leči ubod pčele?"));
        assert!(!mentions_supplies("How do I treat a burn?"));
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
    fn spoken_names_find_stored_items() {
        let items = vec![item("Mleko 2,8%", 1.0, "l"), item("Pasulj tetovac", 2.0, "kg"), item("Brašno", 5.0, "kg")];
        assert_eq!(match_item("mleka", &items).unwrap().name, "Mleko 2,8%");
        assert_eq!(match_item("pasulj", &items).unwrap().name, "Pasulj tetovac");
        assert_eq!(match_item("brasno", &items).unwrap().name, "Brašno");
        assert!(match_item("šećer", &items).is_none());
    }

    #[test]
    fn proposals_say_what_will_change() {
        let items = vec![item("Mleko", 1.0, "l"), item("Pasulj", 2.0, "pcs")];
        let add = PlannedChange { action: "add".into(), name: "mleko".into(), quantity: 2.0, unit: "l".into(), category: "drink".into() };
        let (text, p) = propose(&add, &items, "sr");
        assert_eq!(text, "Da dodam 2 l u „Mleko“? Sada ima 1 l.");
        assert_eq!(p.unwrap().item_id.as_deref(), Some("id-Mleko"));
        let used = PlannedChange { action: "use".into(), name: "pasulja".into(), quantity: 3.0, unit: "pcs".into(), category: "food".into() };
        let (text, p) = propose(&used, &items, "sr");
        assert_eq!(text, "Da skinem 2 kom sa „Pasulj“? Sada ima 2 kom.");
        assert_eq!(p.unwrap().quantity, 2.0, "never more than there is");
        let new = PlannedChange { action: "add".into(), name: "šećer".into(), quantity: 0.0, unit: "kg".into(), category: "food".into() };
        let (text, p) = propose(&new, &items, "en");
        assert_eq!(text, "Add a new item \"Šećer\", 1 kg?");
        assert!(p.unwrap().item_id.is_none());
        let missing = PlannedChange { action: "use".into(), name: "so".into(), quantity: 1.0, unit: "kg".into(), category: "food".into() };
        assert!(propose(&missing, &items, "sr").1.is_none());
        let shop = PlannedChange { action: "shopping".into(), name: "hleb".into(), quantity: 0.0, unit: "pcs".into(), category: "food".into() };
        assert_eq!(propose(&shop, &items, "sr").0, "Da stavim „Hleb“ na listu za kupovinu?");
    }

    #[test]
    fn supplies_context_lists_matches_first() {
        let items = vec![item("Brašno", 5.0, "kg"), item("Mleko", 1.0, "l")];
        let c = supplies_context(&items, &["mleko".into()], "sr");
        let lines: Vec<&str> = c.lines().collect();
        assert!(lines[0].starts_with("Danas je 20"), "{c}");
        assert_eq!(lines[1], "- Mleko: 1 l");
        assert_eq!(lines[2], "- Brašno: 5 kg");
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
    fn prompt_carries_sources_and_rules() {
        let m = build_messages("Koliko traje pasulj?", "sr", &["[1] Pasulj\nTekst.".into()], &[]);
        assert_eq!(m.len(), 2);
        assert!(m[0]["content"].as_str().unwrap().contains("samo činjenice iz izvora"));
        assert!(m[1]["content"].as_str().unwrap().starts_with("Izvori:"));
        let alone = build_messages("How?", "en", &[], &[Turn { question: "q".into(), answer: "a".into() }]);
        assert_eq!(alone.len(), 4);
        assert!(alone[0]["content"].as_str().unwrap().contains("general knowledge"));
    }

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
    }
}
