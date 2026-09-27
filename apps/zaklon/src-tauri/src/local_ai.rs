//! On-device AI for phones: copies a model from the hub, runs the llama.cpp
//! server that ships inside the app (as a separate process, on 127.0.0.1
//! only), and asks it questions. Used when the hub is out of reach, and for
//! measuring what a phone can do.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::client::ClientState;

/// The server executable, packaged as a "library" so Android extracts it.
const SERVER_FILE: &str = "libllama_server_exec.so";
const CONTEXT: &str = "2048";
/// The engine's log is cut down to about this much at each start.
const LOG_KEEP: u64 = 200 * 1024;
/// The engine holds 1-3 GB of memory: it stops after this long unused.
const IDLE_STOP: Duration = Duration::from_secs(10 * 60);
/// A copy that was left unfinished this long is deleted when the app starts.
const PART_KEEP: Duration = Duration::from_secs(30 * 24 * 3600);

#[derive(Debug, Clone, Serialize, Default)]
pub struct CopyProgress {
    pub model: String,
    /// The hub's id of the model, so the app can resume by itself.
    pub model_id: String,
    /// Reading back the part already on the phone before resuming.
    pub verifying: bool,
    pub done: u64,
    pub total: u64,
    pub error: Option<String>,
    pub finished: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalModel {
    pub file: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    /// The AI engine is packaged in this build.
    pub engine: bool,
    pub models: Vec<LocalModel>,
    /// Copies that stopped midway (`<model>.gguf.part`); copying the model
    /// again continues them.
    pub parts: Vec<LocalModel>,
    /// Model file the engine is running with, once it has loaded.
    pub running: Option<String>,
    /// Model file the engine is still loading.
    pub loading: Option<String>,
    pub starting: bool,
    pub copy: Option<CopyProgress>,
    pub cpu_cores: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Answer {
    pub text: String,
    pub tokens: u64,
    pub tokens_per_second: f64,
    pub prompt_ms: f64,
    pub total_ms: u64,
}

struct Server {
    /// Which `start` call spawned this engine.
    id: u64,
    child: std::process::Child,
    port: u16,
    model: String,
    /// The model has loaded and the engine answers.
    ready: bool,
}

pub struct LocalAi {
    models_dir: PathBuf,
    /// The engine's stderr (app data dir/llama.log).
    log_file: PathBuf,
    /// PID of the running engine, so a leftover one can be killed after the
    /// app itself was killed (app data dir/llama.pid).
    pid_file: PathBuf,
    server: Mutex<Option<Server>>,
    /// The id of the `start` call that is loading a model, if any.
    starting: Mutex<Option<u64>>,
    next_id: AtomicU64,
    copy: Arc<Mutex<Option<CopyProgress>>>,
    http: reqwest::Client,
    /// When the engine was last started or asked something.
    last_used: Mutex<Instant>,
    /// Questions being answered right now.
    asking: AtomicUsize,
}

/// Marks the engine as in use while a question is answered.
struct AskGuard<'a>(&'a LocalAi);

impl Drop for AskGuard<'_> {
    fn drop(&mut self) {
        self.0.touch();
        self.0.asking.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Drop for LocalAi {
    fn drop(&mut self) {
        if let Some(mut s) = self.server.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = s.child.kill();
            let _ = std::fs::remove_file(&self.pid_file);
        }
    }
}

/// Clears the loading flag when the `start` call that set it ends (also
/// when that call is canceled), but never a flag set by another call.
struct StartingGuard<'a> {
    starting: &'a Mutex<Option<u64>>,
    id: u64,
}

impl Drop for StartingGuard<'_> {
    fn drop(&mut self) {
        let mut g = self.starting.lock().unwrap_or_else(|p| p.into_inner());
        if *g == Some(self.id) {
            *g = None;
        }
    }
}

/// Folder with the app's native libraries (where Android extracted them).
/// Found from the path of our own library in the process memory map.
fn native_lib_dir() -> Option<PathBuf> {
    let maps = std::fs::read_to_string("/proc/self/maps").ok()?;
    maps.lines()
        .filter_map(|l| l.split_whitespace().last())
        .find(|p| p.ends_with("libzaklon_app_lib.so"))
        .and_then(|p| Path::new(p).parent().map(Path::to_path_buf))
}

fn server_exe() -> Option<PathBuf> {
    native_lib_dir().map(|d| d.join(SERVER_FILE)).filter(|p| p.is_file())
}

/// Model files (`.gguf`) or unfinished copies (`.gguf.part`) in `dir`.
fn list_files(dir: &Path, suffix: &str) -> Vec<LocalModel> {
    let mut out: Vec<LocalModel> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.file_name().to_string_lossy().ends_with(suffix))
                .map(|e| LocalModel {
                    file: e.file_name().to_string_lossy().to_string(),
                    size: e.metadata().map(|m| m.len()).unwrap_or(0),
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| a.file.cmp(&b.file));
    out
}

/// Delete unfinished copies nobody continued for `PART_KEEP`: each can be
/// gigabytes, and nothing else would ever remove them.
fn prune_parts(dir: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        if !e.file_name().to_string_lossy().ends_with(".gguf.part") {
            continue;
        }
        let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > PART_KEEP);
        if old {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

fn free_port() -> std::io::Result<u16> {
    Ok(std::net::TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port())
}

/// Keep only the last `LOG_KEEP` bytes of the log (from a line start).
fn trim_log(path: &Path) {
    let Ok(data) = std::fs::read(path) else { return };
    if (data.len() as u64) <= LOG_KEEP {
        return;
    }
    let mut from = data.len() - LOG_KEEP as usize;
    if let Some(nl) = data[from..].iter().position(|&b| b == b'\n') {
        from += nl + 1;
    }
    let _ = std::fs::write(path, &data[from..]);
}

/// The last few non-empty lines written to the log after `offset`.
fn log_tail(path: &Path, offset: u64) -> String {
    let Ok(data) = std::fs::read(path) else { return String::new() };
    let from = (offset as usize).min(data.len()).max(data.len().saturating_sub(16 * 1024));
    let text = String::from_utf8_lossy(&data[from..]);
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let tail = lines[lines.len().saturating_sub(3)..].join(" | ");
    let count = tail.chars().count();
    if count > 600 {
        format!("…{}", tail.chars().skip(count - 600).collect::<String>())
    } else {
        tail
    }
}

/// Kill an engine left running by an earlier run of the app (for example
/// when Android killed the app), if the PID file points at one.
fn kill_leftover(pid_file: &Path) {
    let Ok(text) = std::fs::read_to_string(pid_file) else { return };
    let _ = std::fs::remove_file(pid_file);
    let Ok(pid) = text.trim().parse::<u32>() else { return };
    let is_ours = |pid: u32| {
        std::fs::read(format!("/proc/{pid}/cmdline"))
            .map(|c| String::from_utf8_lossy(&c).contains(SERVER_FILE))
            .unwrap_or(false)
    };
    if pid == std::process::id() || !is_ours(pid) {
        return;
    }
    let _ = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    // Give the system a moment to free its memory before loading again.
    let deadline = Instant::now() + Duration::from_secs(3);
    while is_ours(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
}

impl LocalAi {
    pub fn new(app_data: &Path) -> Self {
        let models_dir = app_data.join("models");
        let _ = std::fs::create_dir_all(&models_dir);
        prune_parts(&models_dir);
        Self {
            models_dir,
            log_file: app_data.join("llama.log"),
            pid_file: app_data.join("llama.pid"),
            server: Mutex::new(None),
            starting: Mutex::new(None),
            next_id: AtomicU64::new(1),
            copy: Arc::new(Mutex::new(None)),
            http: reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(600)).build().expect("http client"),
            last_used: Mutex::new(Instant::now()),
            asking: AtomicUsize::new(0),
        }
    }

    fn touch(&self) {
        *self.last_used.lock().unwrap_or_else(|p| p.into_inner()) = Instant::now();
    }

    pub fn status(&self) -> Status {
        let (running, loading) = {
            let mut guard = self.server.lock().unwrap_or_else(|p| p.into_inner());
            let alive = guard.as_mut().is_some_and(|s| matches!(s.child.try_wait(), Ok(None)));
            if !alive {
                *guard = None;
            }
            match guard.as_ref() {
                Some(s) if s.ready => (Some(s.model.clone()), None),
                Some(s) => (None, Some(s.model.clone())),
                None => (None, None),
            }
        };
        Status {
            engine: server_exe().is_some(),
            models: list_files(&self.models_dir, ".gguf"),
            parts: list_files(&self.models_dir, ".gguf.part"),
            running,
            loading,
            starting: self.starting.lock().unwrap_or_else(|p| p.into_inner()).is_some(),
            copy: self.copy.lock().unwrap_or_else(|p| p.into_inner()).clone(),
            cpu_cores: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4),
        }
    }

    /// Copy a model from the hub in the background; progress via `status()`.
    pub fn start_copy(&self, client: Arc<ClientState>, model_id: String, file: String) -> Result<(), String> {
        // Check the name before claiming the copy slot, so a bad name cannot block later copies.
        if !file.ends_with(".gguf") || file.contains('/') || file.contains('\\') || file.contains("..") {
            return Err("bad model file name".into());
        }
        {
            let mut c = self.copy.lock().unwrap_or_else(|p| p.into_inner());
            if c.as_ref().is_some_and(|c| !c.finished) {
                return Err("a copy is already running".into());
            }
            *c = Some(CopyProgress { model: file.clone(), model_id: model_id.clone(), ..Default::default() });
        }
        let dest = self.models_dir.join(&file);
        let progress = self.copy.clone();
        tauri::async_runtime::spawn(async move {
            let p2 = progress.clone();
            let result = client
                .fetch_to_file(&format!("/api/models/{model_id}/file"), &dest, move |done, total, verifying| {
                    if let Some(c) = p2.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
                        c.done = done;
                        c.total = total;
                        c.verifying = verifying;
                    }
                })
                .await;
            if let Some(c) = progress.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
                c.finished = true;
                c.error = result.err();
            }
        });
        Ok(())
    }

    /// Delete a model, or an unfinished copy of one (`<model>.gguf.part`).
    pub fn delete_model(&self, file: &str) -> Result<(), String> {
        let named_ok = file.ends_with(".gguf") || file.ends_with(".gguf.part");
        if !named_ok || file.contains('/') || file.contains('\\') || file.contains("..") {
            return Err("bad model file name".into());
        }
        let copying = self
            .copy
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .is_some_and(|c| !c.finished && (c.model == file || format!("{}.part", c.model) == file));
        if copying {
            return Err("that model is still being copied".into());
        }
        // Stop the engine only when it uses this file.
        let taken = {
            let mut g = self.server.lock().unwrap_or_else(|p| p.into_inner());
            if g.as_ref().is_some_and(|s| s.model == file) { g.take() } else { None }
        };
        if let Some(s) = taken {
            self.kill(s);
        }
        std::fs::remove_file(self.models_dir.join(file)).map_err(|e| e.to_string())
    }

    fn kill(&self, mut s: Server) {
        let _ = s.child.kill();
        let _ = s.child.wait();
        let _ = std::fs::remove_file(&self.pid_file);
    }

    pub fn stop(&self) {
        let taken = self.server.lock().unwrap_or_else(|p| p.into_inner()).take();
        if let Some(s) = taken {
            self.kill(s);
        }
    }

    /// Stop the engine only if it is the one spawned by `start` call `id`.
    fn stop_if(&self, id: u64) {
        let taken = {
            let mut g = self.server.lock().unwrap_or_else(|p| p.into_inner());
            if g.as_ref().is_some_and(|s| s.id == id) { g.take() } else { None }
        };
        if let Some(s) = taken {
            self.kill(s);
        }
    }

    /// Start the engine with a model and wait until it has loaded.
    pub async fn start(self: &Arc<Self>, file: &str) -> Result<(), String> {
        let exe = server_exe().ok_or("the AI engine is not part of this app build")?;
        let model = self.models_dir.join(file);
        if !model.is_file() {
            return Err("that model is not on this phone".into());
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        {
            let mut g = self.starting.lock().unwrap_or_else(|p| p.into_inner());
            if g.is_some() {
                return Err("the AI is already starting".into());
            }
            *g = Some(id);
        }
        let _starting = StartingGuard { starting: &self.starting, id };
        kill_leftover(&self.pid_file);
        self.stop();
        let port = free_port().map_err(|e| e.to_string())?;
        let lib_dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
        // Big cores only: phones have 4 fast cores and 4 efficient ones.
        let threads = std::thread::available_parallelism().map(|n| (n.get() / 2).max(2)).unwrap_or(4);
        trim_log(&self.log_file);
        let (stderr, log_start) = match std::fs::OpenOptions::new().create(true).append(true).open(&self.log_file) {
            Ok(mut f) => {
                use std::io::Write;
                let _ = writeln!(f, "--- starting {file} ---");
                let start = f.metadata().map(|m| m.len()).unwrap_or(0);
                (std::process::Stdio::from(f), start)
            }
            Err(_) => (std::process::Stdio::null(), 0),
        };
        let child = std::process::Command::new(&exe)
            .arg("-m")
            .arg(&model)
            .args(["--host", "127.0.0.1", "--port", &port.to_string(), "-c", CONTEXT, "-t", &threads.to_string()])
            .args(["-ngl", "0"])
            .env("LD_LIBRARY_PATH", &lib_dir)
            .current_dir(&lib_dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(stderr)
            .spawn()
            .map_err(|e| format!("could not start the AI engine: {e}"))?;
        let _ = std::fs::write(&self.pid_file, child.id().to_string());
        *self.server.lock().unwrap_or_else(|p| p.into_inner()) = Some(Server { id, child, port, model: file.to_string(), ready: false });

        // Loading a model takes a few seconds to a minute.
        let deadline = Instant::now() + Duration::from_secs(180);
        let result = loop {
            if Instant::now() > deadline {
                break Err("the AI engine did not become ready in 3 minutes".to_string());
            }
            // None: stopped from elsewhere (e.g. the model was deleted); Some(exited).
            let state = {
                let mut g = self.server.lock().unwrap_or_else(|p| p.into_inner());
                g.as_mut().filter(|s| s.id == id).map(|s| matches!(s.child.try_wait(), Ok(Some(_)) | Err(_)))
            };
            match state {
                None => break Err("the AI engine was stopped while loading".to_string()),
                Some(true) => {
                    let tail = log_tail(&self.log_file, log_start);
                    break Err(if tail.is_empty() {
                        "the AI engine stopped while loading (not enough memory?)".to_string()
                    } else {
                        format!("the AI engine stopped while loading: {tail}")
                    });
                }
                Some(false) => {}
            }
            if let Ok(r) = self.http.get(format!("http://127.0.0.1:{port}/health")).timeout(Duration::from_secs(2)).send().await {
                if r.status().is_success() {
                    break Ok(());
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        };
        match &result {
            Ok(()) => {
                if let Some(s) = self.server.lock().unwrap_or_else(|p| p.into_inner()).as_mut().filter(|s| s.id == id) {
                    s.ready = true;
                }
                self.touch();
                self.stop_when_idle(id);
            }
            Err(_) => self.stop_if(id),
        }
        result
    }

    /// Stop the engine started by `start` call `id` once nobody has asked it
    /// anything for `IDLE_STOP`, so its memory goes back to the phone.
    fn stop_when_idle(self: &Arc<Self>, id: u64) {
        let me = Arc::downgrade(self);
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                let Some(ai) = me.upgrade() else { return };
                let current = ai.server.lock().unwrap_or_else(|p| p.into_inner()).as_ref().map(|s| s.id);
                if current != Some(id) {
                    return;
                }
                let idle = ai.last_used.lock().unwrap_or_else(|p| p.into_inner()).elapsed();
                if ai.asking.load(Ordering::SeqCst) == 0 && idle >= IDLE_STOP {
                    tracing::info!("stopping the AI engine: unused for {} minutes", idle.as_secs() / 60);
                    ai.stop_if(id);
                    return;
                }
            }
        });
    }

    pub async fn ask(&self, prompt: &str, language: &str) -> Result<Answer, String> {
        let port = match self.server.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
            Some(s) if s.ready => s.port,
            Some(_) => return Err("the AI is still loading; try again in a moment".into()),
            None => return Err("start the AI engine first".into()),
        };
        self.asking.fetch_add(1, Ordering::SeqCst);
        self.touch();
        let _in_use = AskGuard(self);
        // Answer in the language of the question; the app language only breaks ties.
        let language = question_language(prompt).unwrap_or(language);
        let system = if language == "sr" {
            "Ti si Zaklon, pomoćnik za domaćinstvo. Odgovaraj kratko i jasno, na srpskom jeziku, latinicom. Ako nisi siguran, reci da nisi siguran."
        } else {
            "You are Zaklon, a household assistant. Answer briefly and clearly. If you are not sure, say so."
        };
        let body = serde_json::json!({
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": prompt }
            ],
            "max_tokens": 300,
            // Measured on Qwen3.5 2B/4B: a low temperature and a mild repetition
            // penalty stop invented facts and answers that loop.
            "temperature": 0.3,
            "repeat_penalty": 1.1,
            "chat_template_kwargs": { "enable_thinking": false }
        });
        let started = Instant::now();
        let res = self
            .http
            .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = res.status();
        let v: serde_json::Value = res.json().await.unwrap_or_default();
        if !status.is_success() {
            // llama-server says {"error": {"code": 503, "message": "Loading model"}}.
            if status == reqwest::StatusCode::SERVICE_UNAVAILABLE {
                return Err("the AI is still loading; try again in a moment".into());
            }
            let msg = v["error"]["message"].as_str().unwrap_or_default();
            return Err(format!("AI engine replied {status}: {msg}").trim_end_matches([':', ' ']).to_string());
        }
        let text = strip_thinking(v["choices"][0]["message"]["content"].as_str().unwrap_or_default());
        if text.is_empty() {
            return Err("the AI engine returned an empty answer".into());
        }
        Ok(Answer {
            text,
            tokens: v["usage"]["completion_tokens"].as_u64().unwrap_or(0),
            tokens_per_second: v["timings"]["predicted_per_second"].as_f64().unwrap_or(0.0),
            prompt_ms: v["timings"]["prompt_ms"].as_f64().unwrap_or(0.0),
            total_ms: started.elapsed().as_millis() as u64,
        })
    }
}

/// Some models write their reasoning in <think>…</think>; show only the answer.
fn strip_thinking(s: &str) -> String {
    let mut out = s.to_string();
    while let (Some(a), Some(b)) = (out.find("<think>"), out.find("</think>")) {
        if b < a {
            break;
        }
        out.replace_range(a..b + "</think>".len(), "");
    }
    out.trim().to_string()
}

/// "sr" or "en" when the text clearly is one of them. (Same as
/// `zaklon_core::lang`; the phone app does not link the core crate.)
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
mod file_tests {
    use super::*;

    #[test]
    fn unfinished_copies_are_listed_deletable_and_pruned_when_old() {
        let dir = std::env::temp_dir().join(format!("zaklon-local-ai-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let models = dir.join("models");
        std::fs::create_dir_all(&models).unwrap();
        std::fs::write(models.join("a.gguf"), b"model").unwrap();
        std::fs::write(models.join("b.gguf.part"), b"half").unwrap();
        let stale = models.join("c.gguf.part");
        std::fs::write(&stale, b"old").unwrap();
        let long_ago = std::time::SystemTime::now() - PART_KEEP - Duration::from_secs(3600);
        std::fs::File::options().write(true).open(&stale).unwrap().set_modified(long_ago).unwrap();

        let ai = LocalAi::new(&dir);
        assert!(!stale.exists(), "an old unfinished copy is removed at start");
        let st = ai.status();
        assert_eq!(st.models.iter().map(|m| m.file.as_str()).collect::<Vec<_>>(), ["a.gguf"]);
        assert_eq!(st.parts.iter().map(|m| (m.file.as_str(), m.size)).collect::<Vec<_>>(), [("b.gguf.part", 4)]);
        assert_eq!(st.running, None);
        assert_eq!(st.loading, None);
        // A part being copied right now stays.
        *ai.copy.lock().unwrap() = Some(CopyProgress { model: "b.gguf".into(), ..Default::default() });
        assert!(ai.delete_model("b.gguf.part").is_err());
        ai.copy.lock().unwrap().as_mut().unwrap().finished = true;
        ai.delete_model("b.gguf.part").unwrap();
        assert!(ai.status().parts.is_empty());
        assert!(ai.delete_model("../llama.pid").is_err());
        assert!(ai.delete_model("llama.log").is_err());
        drop(ai);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod lang_tests {
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
