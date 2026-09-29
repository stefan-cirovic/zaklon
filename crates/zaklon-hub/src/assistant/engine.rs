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
pub(super) const SLOTS: u32 = 5;
/// The slots when memory is short: one for the plan, in either language,
/// and one for every kind of answer (see `slot_for`). Each slot costs
/// memory (see `engine_memory`); with two, the plan's instructions are
/// still read only once.
const LEAN_SLOTS: u32 = 2;
/// Tokens of context in each slot. An answer's prompt is at most
/// `PROMPT_CHARS` (about 4,800 tokens of Serbian) and its answer 380 tokens,
/// so a slot is not made smaller to save memory: fewer slots are used instead.
const SLOT_CONTEXT: u32 = 6144;

const MIB: u64 = 1 << 20;
const GIB: u64 = 1 << 30;
/// Memory kept for everything but the AI: Windows itself, the hub, the
/// library's server and the app's window. With less than this left beside
/// the AI engine, Windows starts moving memory to the disk, and everything
/// on the computer crawls (Windows 10 itself asks for 2 GB).
const RESERVE: u64 = 2 * GIB;
/// Context checkpoints each slot keeps: copies of the model's running state
/// (a Qwen3.5 model is partly recurrent) that the engine makes where the
/// part a prompt shares with the one before ends and near its end, up to 32
/// a slot, and drops once the next prompt no longer starts with what they
/// cover. Twelve questions in a row, as the hub asks them, left 3 in each
/// slot, and the engine's memory stayed the same.
const CHECKPOINTS: u64 = 3;

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
    /// This computer has the memory for it (see `fits`).
    pub fits: bool,
    /// The memory a computer needs for it, in bytes (see `memory_needed`).
    pub needs_ram: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Overview {
    pub engine: EngineState,
    pub engine_installed: bool,
    pub selected: Option<String>,
    /// None when no model fits this computer: the assistant cannot work
    /// here, and everything else still does.
    pub recommended: Option<String>,
    pub ram_total: u64,
    pub models: Vec<ModelChoice>,
    pub books: usize,
}

pub(super) struct Running {
    child: Child,
    model: String,
}

/// Why the engine does not start: the model can never run well on this
/// computer, or it could, but not with the memory free right now.
pub(super) const TOO_BIG: &str = "this AI model needs more memory than this computer has; choose a smaller model";
pub(super) const LOW_MEMORY: &str = "not enough free memory for the AI right now; close some programs and try again";

/// What a model takes in memory besides its file, measured with the
/// engine (llama-server b11202, on Windows) and its own report of what it
/// set aside. Qwen3.5 models keep keys and values only in every fourth
/// layer, and a running state of fixed size in the others.
struct ModelMemory {
    /// Keys and values of one token of context.
    kv_per_token: u64,
    /// The running state of one slot, and of one of its checkpoints.
    state_per_slot: u64,
    /// The engine's working buffers and the program itself.
    base: u64,
}

/// Measured on the test computer (see the tests): the engine's memory with
/// the 0.8B model (its file 795 MiB) was 1,525 MiB with 5 slots and 1,237
/// MiB with 2; with the 2B (1,222 MiB) 1,983 and 1,639, with the 4B (2,614
/// MiB) 4,342 and 3,447. Not counted there are the parts of the file the
/// engine copied into another layout at the start and no longer reads (491
/// MiB of the 2B, 1,298 MiB of the 4B), which Windows drops first when
/// memory is short.
fn model_memory(model: &str) -> ModelMemory {
    match model {
        "qwen35-08b" => ModelMemory { kv_per_token: 12 << 10, state_per_slot: 19_266 * MIB / 1000, base: 160 * MIB },
        "qwen35-2b" => ModelMemory { kv_per_token: 12 << 10, state_per_slot: 19_266 * MIB / 1000, base: 190 * MIB },
        "qwen35-4b" => ModelMemory { kv_per_token: 32 << 10, state_per_slot: 50_251 * MIB / 1000, base: 215 * MIB },
        // The 9B (its file 5,417 MiB) as the 4B, with larger working buffers
        // (102 MiB, the 4B's 75 to 88); and any model not measured.
        _ => ModelMemory { kv_per_token: 32 << 10, state_per_slot: 50_251 * MIB / 1000, base: 256 * MIB },
    }
}

/// Memory the engine takes with `model`, a file of `size` bytes, and
/// `slots` slots of `SLOT_CONTEXT` tokens: the file (the engine maps it into
/// memory and reads all of it for every word), the keys and values of the
/// context, each slot's running state with its checkpoints, and the
/// engine's own buffers.
pub fn engine_memory(model: &str, size: u64, slots: u32) -> u64 {
    let m = model_memory(model);
    let slots = u64::from(slots);
    size + slots * u64::from(SLOT_CONTEXT) * m.kv_per_token + slots * (1 + CHECKPOINTS) * m.state_per_slot + m.base
}

/// The memory a computer needs in all to run `model` (a file of `size`
/// bytes) well: the engine's with the fewest slots, and `RESERVE` for the rest.
pub fn memory_needed(model: &str, size: u64) -> u64 {
    engine_memory(model, size, LEAN_SLOTS) + RESERVE
}

/// A computer with `ram_total` bytes of memory can run `model` well.
pub fn fits(model: &str, size: u64, ram_total: u64) -> bool {
    ram_total >= memory_needed(model, size)
}

/// The slots to start the engine with, or why it cannot start: all of them
/// when the computer has the memory for them and it is free now, else as
/// few as will do, as long as that much is free. Starting with less would
/// make Windows move memory to the disk, and the whole computer would crawl.
pub(super) fn start_slots(model: &str, size: u64, ram_total: u64, ram_available: u64) -> Result<u32, &'static str> {
    if !fits(model, size, ram_total) {
        return Err(TOO_BIG);
    }
    let full = engine_memory(model, size, SLOTS);
    if ram_total >= full + RESERVE && ram_available >= full {
        return Ok(SLOTS);
    }
    if ram_available >= engine_memory(model, size, LEAN_SLOTS) {
        return Ok(LEAN_SLOTS);
    }
    Err(LOW_MEMORY)
}

/// The engine's slot for a kind of prompt (`SLOT_PLAN`...) when it runs
/// with `slots` of them: its own, or with `LEAN_SLOTS` the first for the
/// plan and the second for every answer.
pub(super) fn slot_for(kind: u32, slots: u32) -> u32 {
    if slots >= SLOTS {
        kind
    } else if kind == SLOT_PLAN || kind == SLOT_PLAN_EN || slots < 2 {
        0
    } else {
        1
    }
}

/// The model to recommend for a computer with `ram_total` bytes of memory,
/// given each model's file size (`size_of`, None for a model the catalog
/// does not have): the one that answers best at a speed such a computer
/// manages (a computer with more memory usually has a faster processor),
/// or the biggest smaller one that fits. None when no model fits: the
/// assistant is not for this computer.
pub fn recommended_model(ram_total: u64, size_of: impl Fn(&str) -> Option<u64>) -> Option<&'static str> {
    let wanted = if ram_total >= 15 * GIB {
        // A 16 GB machine reports about 15.x GiB.
        "qwen35-9b"
    } else if ram_total >= 11 * GIB {
        "qwen35-4b"
    } else if ram_total >= 5 * GIB {
        "qwen35-2b"
    } else {
        "qwen35-08b"
    };
    let rank = MODEL_ORDER.iter().position(|m| *m == wanted).unwrap_or(0);
    MODEL_ORDER[..=rank].iter().rev().find(|id| size_of(id).is_some_and(|size| fits(id, size, ram_total))).copied()
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

    /// This computer's memory in bytes, all of it and what is available now.
    fn ram(&self) -> (u64, u64) {
        #[cfg(test)]
        if let Some(ram) = *self.test_ram.lock().unwrap_or_else(|p| p.into_inner()) {
            return ram;
        }
        crate::machine::ram()
    }

    /// The size of a model's file, from the catalog.
    fn model_size(&self, id: &str) -> Option<u64> {
        self.downloads.catalog().pack(id).map(|p| p.size)
    }

    /// This computer has the memory for the model (see `fits`).
    fn model_fits(&self, id: &str, ram_total: u64) -> bool {
        self.model_size(id).is_some_and(|size| fits(id, size, ram_total))
    }

    fn recommended(&self, ram_total: u64) -> Option<&'static str> {
        recommended_model(ram_total, |id| self.model_size(id))
    }

    /// Of the installed models that fit this computer: the chosen one, else
    /// the recommended one, else the biggest below the recommendation, else
    /// the smallest. When none fits, the chosen or the smallest installed
    /// one, so that asking says it needs more memory than this computer has.
    pub fn selected(&self) -> Option<String> {
        let installed = self.installed_models();
        let ram_total = self.ram().0;
        let chosen = self.chosen.lock().unwrap_or_else(|p| p.into_inner()).clone().filter(|id| installed.contains(id));
        let fitting: Vec<&String> = installed.iter().filter(|m| self.model_fits(m, ram_total)).collect();
        if let Some(id) = chosen.as_ref().filter(|id| fitting.contains(id)) {
            return Some(id.clone());
        }
        let rank = |m: &str| MODEL_ORDER.iter().position(|x| *x == m).unwrap_or(0);
        let rec_rank = self.recommended(ram_total).map(rank).unwrap_or(0);
        fitting
            .iter()
            .rfind(|m| rank(m) <= rec_rank)
            .or(fitting.first())
            .map(|m| m.to_string())
            .or(chosen)
            .or_else(|| installed.first().cloned())
    }

    /// Choose the model; one that needs more memory than this computer has is refused.
    pub fn select(&self, id: &str) -> Result<(), String> {
        if !MODEL_ORDER.contains(&id) {
            return Err("unknown model".into());
        }
        if self.model_size(id).is_some() && !self.model_fits(id, self.ram().0) {
            return Err(TOO_BIG.into());
        }
        *self.chosen.lock().unwrap_or_else(|p| p.into_inner()) = Some(id.to_string());
        Ok(())
    }

    /// The engine's slot for a kind of prompt (`SLOT_PLAN`...), in the
    /// slots it was started with.
    pub(super) fn slot(&self, kind: u32) -> u32 {
        slot_for(kind, self.slots.load(Ordering::Relaxed))
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
        let ram_total = self.ram().0;
        let rec = self.recommended(ram_total);
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
                recommended: rec == Some(p.id.as_str()),
                fits: fits(&p.id, p.size, ram_total),
                needs_ram: memory_needed(&p.id, p.size),
            })
            .collect();
        Overview {
            engine: self.engine_state(),
            engine_installed: self.exe().is_file(),
            selected: self.selected(),
            recommended: rec.map(str::to_string),
            ram_total,
            models,
            books: self.library.books().len(),
        }
    }

    fn model_path(&self, id: &str) -> Option<PathBuf> {
        let pack = self.downloads.catalog().pack(id).cloned()?;
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
        // Only with the memory for it (the engine that ran before has given
        // its memory back by now): a model that does not fit makes the whole
        // computer crawl, the library and everything else with it.
        let size = self.model_size(&model).or_else(|| std::fs::metadata(&path).ok().map(|m| m.len())).unwrap_or(0);
        let (ram_total, ram_available) = self.ram();
        let slots = start_slots(&model, size, ram_total, ram_available).map_err(|e| {
            warn!(model = %model, needs = engine_memory(&model, size, LEAN_SLOTS), ram_total, ram_available, "AI engine not started: {e}");
            e.to_string()
        })?;
        self.slots.store(slots, Ordering::Relaxed);
        let port = {
            let l = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
            l.local_addr().map_err(|e| e.to_string())?.port()
        };
        let mut cmd = Command::new(self.exe());
        cmd.arg("-m")
            .arg(&path)
            // A slot for each kind of prompt, each keeping what it read last
            // (see `SLOT_PLAN`; fewer when memory is short, see `start_slots`),
            // with `SLOT_CONTEXT` tokens of context each.
            .args(["--host", "127.0.0.1", "--port", &port.to_string(), "--jinja"])
            .args(["-c", &(slots * SLOT_CONTEXT).to_string(), "-np", &slots.to_string()])
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
        info!(model = %model, port, slots, "AI engine starting");
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

    use super::super::test_util::assistant_on;

    /// The models' file sizes as the bundled catalog has them.
    fn catalog_size(id: &str) -> Option<u64> {
        zaklon_core::catalog::Catalog::bundled().pack(id).map(|p| p.size)
    }

    fn size(id: &str) -> u64 {
        catalog_size(id).unwrap_or_else(|| panic!("the catalog has {id}"))
    }

    /// Memory as computers report it: a 4 GB laptop about 3.8 GiB (less when
    /// the graphics take a part), an 8 GB one 7.6 to 7.9, a 16 GB one 15.9.
    fn gib(tenths: u64) -> u64 {
        tenths * GIB / 10
    }

    #[test]
    fn what_each_model_needs() {
        let mib = |id: &str, slots| engine_memory(id, size(id), slots) / MIB;
        // The engine's memory (MiB) measured with llama-server b11202 on the
        // test computer after a prompt of 6,000 tokens and one in every other
        // slot (the 0.8B with 2 slots: after 12 questions), less the parts of
        // the file copied into another layout. The formula is never below,
        // and not much above (it counts more checkpoints).
        let measured = [
            ("qwen35-08b", SLOTS, 1525),
            ("qwen35-08b", LEAN_SLOTS, 1237),
            ("qwen35-2b", SLOTS, 1983),
            ("qwen35-2b", LEAN_SLOTS, 1639),
            ("qwen35-4b", SLOTS, 4342),
            ("qwen35-4b", LEAN_SLOTS, 3447),
            // The engine gave back part of the 9B's copied file; more was needed.
            ("qwen35-9b", LEAN_SLOTS, 5730),
        ];
        for (id, slots, used) in measured {
            let formula = mib(id, slots);
            assert!(formula >= used && formula < used * 115 / 100, "{id} with {slots} slots: {formula} MiB, measured {used}");
        }
        // Fewer slots, less memory, and a bigger model always needs more.
        for id in MODEL_ORDER {
            assert!(mib(id, LEAN_SLOTS) < mib(id, SLOTS), "{id}");
        }
        for pair in MODEL_ORDER.windows(2) {
            assert!(memory_needed(pair[0], size(pair[0])) < memory_needed(pair[1], size(pair[1])), "{pair:?}");
        }
        // With what Windows and the rest need, the 0.8B model needs a computer of about 3.2 GiB.
        assert!((gib(31)..gib(34)).contains(&memory_needed("qwen35-08b", size("qwen35-08b"))));
    }

    #[test]
    fn models_that_fit_and_the_one_recommended_at_4_8_and_16_gb() {
        let fitting = |ram| MODEL_ORDER.iter().filter(|id| fits(id, size(id), ram)).copied().collect::<Vec<_>>();
        // 4 GB: the 0.8B model is recommended; the 2B fits too, the others not.
        assert_eq!(recommended_model(gib(38), catalog_size), Some("qwen35-08b"));
        assert_eq!(fitting(gib(38)), ["qwen35-08b", "qwen35-2b"]);
        // 4 GB of which the graphics take half a gigabyte: the 0.8B only.
        assert_eq!(recommended_model(gib(34), catalog_size), Some("qwen35-08b"));
        assert_eq!(fitting(gib(34)), ["qwen35-08b"]);
        // 8 GB: the 2B model; the 9B does not fit.
        for ram in [gib(76), gib(79), 8 * GIB] {
            assert_eq!(recommended_model(ram, catalog_size), Some("qwen35-2b"));
            assert_eq!(fitting(ram), ["qwen35-08b", "qwen35-2b", "qwen35-4b"]);
        }
        // 12 GB: the 4B; 16 GB and more: the 9B, and everything fits.
        assert_eq!(recommended_model(gib(118), catalog_size), Some("qwen35-4b"));
        for ram in [gib(159), 16 * GIB, 32 * GIB] {
            assert_eq!(recommended_model(ram, catalog_size), Some("qwen35-9b"));
            assert_eq!(fitting(ram), MODEL_ORDER);
        }
        // Too little memory for any model: nothing is recommended.
        for ram in [gib(30), 2 * GIB, 0] {
            assert_eq!(recommended_model(ram, catalog_size), None, "{ram}");
            assert!(fitting(ram).is_empty());
        }
        // A model the catalog does not have is never recommended: the next smaller one is.
        let no_2b = |id: &str| if id == "qwen35-2b" { None } else { catalog_size(id) };
        assert_eq!(recommended_model(gib(76), no_2b), Some("qwen35-08b"));
        assert_eq!(recommended_model(gib(76), |_| None), None);
    }

    #[test]
    fn the_engine_starts_only_with_the_memory_for_it() {
        let start = |id: &str, total, available| start_slots(id, size(id), total, available);
        // 16 GB with most of it free: every slot.
        assert_eq!(start("qwen35-9b", gib(159), 12 * GIB), Ok(SLOTS));
        // Busy: fewer slots while that is enough, then not at all.
        let lean = engine_memory("qwen35-9b", size("qwen35-9b"), LEAN_SLOTS);
        let full = engine_memory("qwen35-9b", size("qwen35-9b"), SLOTS);
        assert_eq!(start("qwen35-9b", gib(159), full - 1), Ok(LEAN_SLOTS));
        assert_eq!(start("qwen35-9b", gib(159), lean), Ok(LEAN_SLOTS));
        assert_eq!(start("qwen35-9b", gib(159), lean - 1), Err(LOW_MEMORY));
        // 8 GB: the 9B never, however much is free.
        assert_eq!(start("qwen35-9b", gib(79), gib(79)), Err(TOO_BIG));
        assert_eq!(start("qwen35-4b", gib(79), 5 * GIB), Ok(SLOTS));
        assert_eq!(start("qwen35-4b", gib(79), 3 * GIB), Err(LOW_MEMORY));
        // 4 GB: the 0.8B model with every slot when that much is free, else two.
        assert_eq!(start("qwen35-08b", gib(38), 2 * GIB), Ok(SLOTS));
        assert_eq!(start("qwen35-08b", gib(38), gib(14)), Ok(LEAN_SLOTS));
        assert_eq!(start("qwen35-08b", gib(38), GIB), Err(LOW_MEMORY));
        assert_eq!(start("qwen35-4b", gib(38), gib(38)), Err(TOO_BIG));
        // The app translates the reasons by these words (see api/error.rs).
        assert!(TOO_BIG.contains("needs more memory than this computer has"));
        assert!(LOW_MEMORY.contains("not enough free memory for the AI"));
    }

    #[test]
    fn with_two_slots_the_plan_has_one_and_the_answers_the_other() {
        let kinds = [SLOT_PLAN, SLOT_PLAN_EN, SLOT_ANSWER, SLOT_HEALTH, SLOT_SUPPLIES];
        assert_eq!(kinds.map(|k| slot_for(k, SLOTS)), kinds);
        assert_eq!(kinds.map(|k| slot_for(k, LEAN_SLOTS)), [0, 0, 1, 1, 1]);
        assert_eq!(kinds.map(|k| slot_for(k, 1)), [0; 5]);
    }

    #[test]
    fn the_overview_marks_models_that_do_not_fit_and_never_recommends_one() {
        // 4 GB: the 0.8B model is recommended, the 4B and 9B are marked, and choosing one is refused.
        let ai = assistant_on((gib(38), GIB));
        let ov = ai.overview();
        assert_eq!(ov.recommended.as_deref(), Some("qwen35-08b"));
        assert_eq!(ov.ram_total, gib(38));
        let fit: Vec<(&str, bool)> = ov.models.iter().map(|m| (m.id.as_str(), m.fits)).collect();
        assert_eq!(fit, [("qwen35-08b", true), ("qwen35-2b", true), ("qwen35-4b", false), ("qwen35-9b", false)]);
        assert!(ov.models.iter().all(|m| m.needs_ram == memory_needed(&m.id, m.size)));
        assert!(ov.models.iter().filter(|m| m.recommended).map(|m| m.id.as_str()).eq(["qwen35-08b"]));
        assert_eq!(ai.select("qwen35-9b"), Err(TOO_BIG.to_string()));
        assert_eq!(ai.select("qwen35-4b"), Err(TOO_BIG.to_string()));
        assert_eq!(ai.select("qwen35-08b"), Ok(()));
        assert_eq!(ai.select("nothing"), Err("unknown model".to_string()));
        // Nothing installed, nothing selected.
        assert_eq!(ov.selected, None);

        // 3 GB: no model fits, none is recommended and none can be chosen.
        let small = assistant_on((gib(30), GIB));
        let ov = small.overview();
        assert_eq!(ov.recommended, None);
        assert!(ov.models.len() == 4 && ov.models.iter().all(|m| !m.fits && !m.recommended));
        assert_eq!(small.select("qwen35-08b"), Err(TOO_BIG.to_string()));

        // 16 GB: the 9B, and every model can be chosen.
        let big = assistant_on((gib(159), 12 * GIB));
        assert_eq!(big.overview().recommended.as_deref(), Some("qwen35-9b"));
        assert!(big.overview().models.iter().all(|m| m.fits));
        assert_eq!(big.select("qwen35-9b"), Ok(()));
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
