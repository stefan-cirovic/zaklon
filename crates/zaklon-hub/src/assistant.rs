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
    /// False when no library passage was found and the model answered alone.
    pub grounded: bool,
    pub language: &'static str,
    pub tokens_per_second: f64,
    pub error: Option<String>,
    #[serde(skip)]
    created: Instant,
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
        loop {
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
    pub fn ask(self: &Arc<Self>, question: &str, app_language: &str, history: Vec<Turn>) -> Result<String, String> {
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
        tokio::spawn(async move {
            let _turn = me.turn.lock().await;
            if let Err(e) = me.run(&id2, &question, language, &history).await {
                warn!("assistant: {e}");
                me.update(&id2, |a| {
                    a.status = AnswerStatus::Failed;
                    a.error = Some(e);
                });
            }
            me.last_used.store(me.epoch.elapsed().as_secs(), Ordering::Relaxed);
        });
        Ok(id)
    }

    async fn run(&self, id: &str, question: &str, language: &'static str, history: &[Turn]) -> Result<(), String> {
        // 1. Make sure the engine runs (it also picks the search words).
        self.update(id, |a| a.status = AnswerStatus::Starting);
        let port = self.ensure_running().await?;

        // 2. Find passages in the library, if there is one.
        self.update(id, |a| a.status = AnswerStatus::Searching);
        let (sources, passages) = if self.library.books().is_empty() {
            (Vec::new(), Vec::new())
        } else {
            let mut terms = self.keywords(port, question, language).await;
            if terms.is_empty() {
                // The model gave nothing usable: fall back to the question's own words.
                terms = search_words(question).iter().map(|w| stem(w)).collect();
            }
            let shown = terms.clone();
            self.update(id, |a| a.searched = shown);
            self.find_sources(&terms).await
        };
        let grounded = !sources.is_empty();
        self.update(id, |a| {
            a.sources = sources.clone();
            a.grounded = grounded;
            a.status = AnswerStatus::Thinking;
        });

        // 3. Ask, streaming the text as it comes.
        let messages = build_messages(question, language, &passages, history);
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
        let mut buf = String::new();
        let mut tokens = 0u64;
        let mut first: Option<Instant> = None;
        let stall = Duration::from_secs(120);
        loop {
            let next = tokio::time::timeout(stall, stream.next()).await.map_err(|_| "the AI engine stopped answering".to_string())?;
            let Some(chunk) = next else { break };
            let chunk = chunk.map_err(|e| format!("AI engine: {e}"))?;
            buf.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(nl) = buf.find('\n') {
                let line: String = buf.drain(..=nl).collect();
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
            a.status = AnswerStatus::Done;
        });
        Ok(())
    }

    /// Encyclopedia search terms for a question, written by the model in
    /// their basic form ("konzerva, pasulj, rok trajanja"). A library search
    /// works on terms, not sentences, and the model knows which words matter.
    async fn keywords(&self, port: u16, question: &str, language: &str) -> Vec<String> {
        // A few examples work better with small models than a long explanation.
        let (prompt, examples): (&str, [(&str, &str); 3]) = if language == "sr" {
            (
                "Za pitanje napiši 2 do 4 pojma za pretragu srpske enciklopedije: imenice u osnovnom obliku, na srpskom, latinicom, odvojene zarezom. Samo pojmove.",
                [
                    ("Koliko dugo traje hleb?", "hleb, rok trajanja"),
                    ("Kako da izlečim prehladu kod deteta?", "prehlada, lečenje, dete"),
                    ("Kako se pravi sapun kod kuće?", "sapun, saponifikacija"),
                ],
            )
        } else {
            (
                "For the question, write 2 to 4 terms to search an encyclopedia: nouns in their basic form, separated by commas. Only the terms.",
                [
                    ("How long does bread last?", "bread, shelf life"),
                    ("How do I treat a cold in a child?", "common cold, treatment, child"),
                    ("How is soap made at home?", "soap, saponification"),
                ],
            )
        };
        let mut messages = vec![serde_json::json!({ "role": "system", "content": prompt })];
        for (q, a) in examples {
            messages.push(serde_json::json!({ "role": "user", "content": q }));
            messages.push(serde_json::json!({ "role": "assistant", "content": a }));
        }
        messages.push(serde_json::json!({ "role": "user", "content": question }));
        let body = serde_json::json!({
            "messages": messages,
            "max_tokens": 40,
            "temperature": 0.1,
            "chat_template_kwargs": { "enable_thinking": false },
        });
        let reply = self
            .http
            .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
            .timeout(Duration::from_secs(60))
            .json(&body)
            .send()
            .await;
        let text = match reply {
            Ok(r) => r.json::<serde_json::Value>().await.ok().and_then(|v| v["choices"][0]["message"]["content"].as_str().map(str::to_string)),
            Err(_) => None,
        }
        .unwrap_or_default();
        parse_keywords(&text)
    }

    /// The best few library passages for some search terms. Each term is
    /// looked up on its own, and articles are ranked by how well their title
    /// and text match the terms.
    async fn find_sources(&self, terms: &[String]) -> (Vec<Source>, Vec<String>) {
        if terms.is_empty() {
            return (Vec::new(), Vec::new());
        }
        let stems: Vec<String> = terms
            .iter()
            .flat_map(|t| t.split_whitespace().map(|w| zaklon_core::translit::fold(&stem(w))).collect::<Vec<_>>())
            .filter(|w| w.chars().count() >= 3)
            .collect();
        let whole: Vec<String> = terms.iter().map(|t| zaklon_core::translit::fold(t)).collect();
        let queries: Vec<String> = terms.iter().take(6).cloned().collect();

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
                // Disambiguation and list pages rarely help.
                if title.contains("вишезначн") || title.contains("списак") {
                    score -= 3;
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
            let text = relevant_text(&html, &stems, SOURCE_CHARS);
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
    let mut pos = 0;
    while out.chars().count() < max {
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
        if para.chars().count() >= 20 {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&para);
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
    fn serbian_answers_end_in_latin() {
        assert_eq!(finish_text(" Пасуљ траје дуго. ", "sr"), "Pasulj traje dugo.");
        assert_eq!(finish_text("Beans last long.", "en"), "Beans last long.");
        assert_eq!(finish_text("Boil it [1].

[1]
", "en"), "Boil it [1].");
    }
}
