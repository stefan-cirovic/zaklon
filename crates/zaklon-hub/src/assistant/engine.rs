//! The AI engine (llama-server) and its model: which model fits this
//! computer, starting the engine with it and stopping it, and warming it up
//! before the first question.

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::process::{Child, Command};
use tracing::{info, warn};
use zaklon_core::catalog::{Category, PackStatus};

use super::Assistant;

/// Stop the engine after this long without questions, to give the memory back.
const IDLE_STOP: Duration = Duration::from_secs(20 * 60);
/// How long the engine may take to answer at all after it was started.
const START_TIMEOUT: Duration = Duration::from_secs(180);
/// While the engine answers "still loading the model", it is making progress:
/// a large model read from a hard disk on a busy computer can take many
/// minutes, so wait up to this long in that case.
const LOADING_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// The engine's slots, one for each kind of prompt. A slot keeps what it
/// read last up to the start of the last message, and reads only what
/// follows: the instructions and examples of the plan (in Serbian and in
/// English), the instructions for answers, those for health answers, and
/// those for the supplies with the supplies list are each read once, not
/// again whenever the kind of question changes (200 to 1,000 tokens, 10 to
/// 50 seconds on a slow computer).
pub(super) const SLOT_PLAN: u32 = 0;
pub(super) const SLOT_PLAN_EN: u32 = 1;
pub(super) const SLOT_ANSWER: u32 = 2;
pub(super) const SLOT_HEALTH: u32 = 3;
pub(super) const SLOT_SUPPLIES: u32 = 4;
const SLOTS: u32 = 5;
/// Tokens of context in each slot.
const SLOT_CONTEXT: u32 = 6144;

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

pub(super) struct Running {
    child: Child,
    model: String,
}

/// Which model fits this computer's memory. The model, its context and the
/// rest of the system all have to fit.
pub fn recommended_model(ram_total: u64) -> &'static str {
    const GIB: u64 = 1 << 30;
    if ram_total >= 15 * GIB {
        // A 16 GB machine reports about 15.x GiB.
        "qwen35-9b"
    } else if ram_total >= 11 * GIB {
        "qwen35-4b"
    } else if ram_total >= 5 * GIB {
        "qwen35-2b"
    } else {
        "qwen35-08b"
    }
}

/// Model size order, smallest first, for picking a fallback.
const MODEL_ORDER: [&str; 4] = ["qwen35-08b", "qwen35-2b", "qwen35-4b", "qwen35-9b"];

impl Assistant {
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
    pub(super) async fn ensure_running(&self) -> Result<u16, String> {
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
            // A slot for each kind of prompt, each keeping what it read last
            // (see `SLOT_PLAN`), with `SLOT_CONTEXT` tokens of context each.
            .args(["--host", "127.0.0.1", "--port", &port.to_string(), "--jinja"])
            .args(["-c", &(SLOTS * SLOT_CONTEXT).to_string(), "-np", &SLOTS.to_string()])
            .args(engine_tuning(crate::machine::cores()))
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
        // A new engine has read nothing yet.
        self.plan_warm.store(false, Ordering::Relaxed);

        // Loading a model takes from seconds to a minute or two, longer for a
        // large model on a hard disk: the engine answers 503 while it loads.
        let started = Instant::now();
        let mut loading_seen = false;
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
            let health = self
                .http
                .get(format!("http://127.0.0.1:{port}/health"))
                .timeout(Duration::from_secs(3))
                .send()
                .await
                .map(|r| r.status());
            match health {
                Ok(s) if s.is_success() => {
                    self.set_state(EngineState::Ready);
                    info!(seconds = started.elapsed().as_secs(), "AI engine ready");
                    return Ok(port);
                }
                Ok(s) if s == reqwest::StatusCode::SERVICE_UNAVAILABLE => loading_seen = true,
                _ => {}
            }
            let limit = if loading_seen { LOADING_TIMEOUT } else { START_TIMEOUT };
            if started.elapsed() > limit {
                if let Some(mut r) = running.take() {
                    let _ = r.child.kill().await;
                }
                self.set_state(EngineState::Failed);
                return Err("the AI engine did not start in time".into());
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Start loading the model in the background (someone opened the
    /// Assistant), so the first answer does not wait for it. Counts as use,
    /// so the idle timer starts from now.
    pub fn warm_up(self: &Arc<Self>, language: &str) {
        if !matches!(self.engine_state(), EngineState::Stopped | EngineState::Failed) {
            return;
        }
        self.last_used.store(self.epoch.elapsed().as_secs(), Ordering::Relaxed);
        let me = self.clone();
        let language = if language == "sr" { "sr".to_string() } else { "en".to_string() };
        tokio::spawn(async move {
            // Not while a question is being answered; that one starts it anyway.
            let Ok(_turn) = me.turn.try_lock() else { return };
            match me.ensure_running().await {
                // Also read the long routing instructions once, so the first
                // question does not wait for them (the slot keeps them cached).
                Ok(port) => {
                    let _ = me.plan(port, "?", &language).await;
                }
                Err(e) => warn!("warming up the AI engine: {e}"),
            }
        });
    }
}

/// Engine options for this computer. The engine's store of earlier prompts
/// in memory is off: each kind of prompt has its own slot (see `SLOT_PLAN`),
/// and the store kept a copy of the model's state for each prompt (about
/// 50 MiB each with the 9B model, several a prompt) up to 8 GiB. It grew by
/// half a gigabyte a question until a 16 GB computer ran short of memory and
/// Windows began to push the model itself out to disk. Prompts are read with
/// every thread where that is faster (see `Cores::prompt_threads`); writing
/// keeps the engine's own choice, one thread a core, as the speed of memory
/// limits it anyway.
fn engine_tuning(cores: Option<crate::machine::Cores>) -> Vec<String> {
    let mut args = vec!["--cache-ram".to_string(), "0".to_string()];
    if let Some(n) = cores.and_then(|c| c.prompt_threads()) {
        args.extend(["-tb".to_string(), n.to_string()]);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_by_memory() {
        const GIB: u64 = 1 << 30;
        assert_eq!(recommended_model(32 * GIB), "qwen35-9b");
        assert_eq!(recommended_model(16 * GIB), "qwen35-9b");
        assert_eq!(recommended_model(15 * GIB + GIB / 2), "qwen35-9b");
        assert_eq!(recommended_model(14 * GIB), "qwen35-4b");
        assert_eq!(recommended_model(12 * GIB - GIB / 2), "qwen35-4b");
        assert_eq!(recommended_model(8 * GIB), "qwen35-2b");
        assert_eq!(recommended_model(4 * GIB), "qwen35-08b");
    }

    #[test]
    fn the_engine_reads_prompts_with_every_thread_where_that_is_faster() {
        use crate::machine::Cores;
        assert_eq!(engine_tuning(Some(Cores { physical: 6, logical: 12, uniform: true })), vec!["--cache-ram", "0", "-tb", "12"]);
        assert_eq!(engine_tuning(Some(Cores { physical: 14, logical: 20, uniform: false })), vec!["--cache-ram", "0"]);
        assert_eq!(engine_tuning(None), vec!["--cache-ram", "0"], "no store of old prompts growing in memory anywhere");
        // Without that store, each kind of prompt keeps its own slot.
        let slots = [SLOT_PLAN, SLOT_PLAN_EN, SLOT_ANSWER, SLOT_HEALTH, SLOT_SUPPLIES];
        assert!(slots.iter().all(|s| *s < SLOTS) && (1..slots.len()).all(|i| !slots[..i].contains(&slots[i])), "{slots:?}");
    }
}
