//! Add-on downloads: one pack at a time, resumable with HTTP ranges, verified
//! with SHA-256, optionally unpacked, and importable from or exportable to a
//! folder (USB stick). State survives restarts in `<root>/catalog/state.json`.
//!
//! What a pack has on disk is recorded when its files are verified
//! (`PackState::files`), and everything that uses packs goes by that record.
//! A newer catalog therefore never turns an old file into "the new version":
//! the pack shows an update, the old files keep working until the new ones
//! are verified, and only then are the old ones deleted.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::future::Future;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::sync::Notify;
use tracing::{info, warn};
use zaklon_core::catalog::{is_safe_relative, Catalog, InstalledFile, Pack, PackFile, PackState, PackStatus};

/// Downloads stop below this battery level unless the charger is connected (SPEC §6).
pub const MIN_BATTERY_PERCENT: u8 = 50;
/// Keep at least this much free after a download.
const DISK_MARGIN: u64 = 512 * 1024 * 1024;
/// A partial file with no record of how much of it reached the disk (written
/// by an older version, or state.json was lost) is trusted except for this
/// much of its end.
const RESUME_OVERLAP: u64 = 4 << 20;
const STATE_SAVE_INTERVAL: Duration = Duration::from_secs(2);
/// How often downloaded data is forced onto the disk. After a power cut a
/// download continues from the last point that was synced.
const SYNC_INTERVAL: Duration = Duration::from_secs(30);
/// A download with no data for this long is treated as a broken connection.
const STALL_TIMEOUT: Duration = Duration::from_secs(60);
/// Written into an unpack folder once unpacking finished.
const UNPACKED_MARKER: &str = ".zaklon-unpacked";

/// Makes the programs using a pack (kiwix-serve, llama-server) let go of its
/// files before they are replaced or deleted; Windows refuses both while a
/// file is open.
pub type ReleaseFn = dyn Fn(Pack) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync;

#[derive(Debug, Clone, Serialize)]
pub struct PackView {
    #[serde(flatten)]
    pub pack: Pack,
    pub state: PackState,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemInfo {
    pub disk_free: u64,
    pub disk_total: u64,
    pub battery_percent: Option<u8>,
    pub plugged_in: bool,
}

enum Outcome {
    Done,
    Paused,
    Failed(String),
}

/// What the worker does with a pack it takes from the queue.
enum Job {
    Download,
    /// Copy the files from this folder (a USB stick).
    Import(PathBuf),
    /// Only check the files already on disk; nothing is fetched.
    Verify,
}

pub struct Downloads {
    catalog: Catalog,
    library: PathBuf,
    state_path: PathBuf,
    states: Mutex<HashMap<String, PackState>>,
    queue: Mutex<VecDeque<String>>,
    pause_requests: Mutex<HashSet<String>>,
    /// Queued packs that are only to be verified.
    verify_only: Mutex<HashSet<String>>,
    /// Packs checked again this session because the library engine could not open them.
    rechecked: Mutex<HashSet<String>>,
    notify: Notify,
    client: reqwest::Client,
    release: OnceLock<Box<ReleaseFn>>,
    /// Number of save requests so far.
    save_requested: AtomicU64,
    /// The request number state.json on disk is up to date with; also the write lock.
    save_written: Mutex<u64>,
}

impl Downloads {
    pub fn new(catalog: Catalog, library: PathBuf, state_path: PathBuf) -> Arc<Self> {
        let mut states: HashMap<String, PackState> = std::fs::read_to_string(&state_path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        // Nothing is running right after a restart: anything mid-flight waits for a resume.
        for s in states.values_mut() {
            if matches!(s.status, PackStatus::Downloading | PackStatus::Verifying | PackStatus::Queued) {
                s.status = PackStatus::Paused;
            }
            s.speed = 0;
        }
        let mut queue = VecDeque::new();
        let mut verify_only = HashSet::new();
        for p in &catalog.packs {
            let st = states.entry(p.id.clone()).or_insert_with(|| PackState::not_installed(p.size));
            if reconcile(&library, p, st) {
                queue.push_back(p.id.clone());
                verify_only.insert(p.id.clone());
            }
            // A USB copy that was cut off leaves its temporary file behind.
            for f in &p.files {
                let _ = std::fs::remove_file(import_path(&library.join(&f.path)));
            }
        }
        let with_stale: Vec<String> = states.iter().filter(|(_, s)| !s.stale.is_empty()).map(|(id, _)| id.clone()).collect();
        let client = reqwest::Client::builder()
            .user_agent(format!("Zaklon/{}", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(STALL_TIMEOUT)
            // Never follow a redirect from an encrypted to a plain address.
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                let downgrade = attempt.previous().last().is_some_and(|u| u.scheme() == "https")
                    && attempt.url().scheme() != "https";
                if downgrade || attempt.previous().len() > 10 {
                    attempt.stop()
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .expect("http client");
        let has_work = !queue.is_empty();
        let me = Arc::new(Self {
            catalog,
            library,
            state_path,
            states: Mutex::new(states),
            queue: Mutex::new(queue),
            pause_requests: Mutex::new(HashSet::new()),
            verify_only: Mutex::new(verify_only),
            rechecked: Mutex::new(HashSet::new()),
            notify: Notify::new(),
            client,
            release: OnceLock::new(),
            save_requested: AtomicU64::new(0),
            save_written: Mutex::new(0),
        });
        // Nothing has the library's files open yet: a good moment to delete old ones.
        for id in with_stale {
            me.sweep(&id);
        }
        if has_work {
            // Kept as a permit until the worker starts.
            me.notify.notify_one();
        }
        me
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// Set what makes the engines let go of a pack's files. Call once, at startup.
    pub fn set_release(&self, f: Box<ReleaseFn>) {
        let _ = self.release.set(f);
    }

    /// Stop whatever program has `pack`'s files open (see `set_release`).
    pub async fn release(&self, pack: &Pack) {
        if let Some(f) = self.release.get() {
            f(pack.clone()).await;
        }
    }

    /// Spawn the worker that processes the queue. Call from inside a Tokio runtime.
    pub fn start(self: &Arc<Self>) {
        let me = self.clone();
        tokio::spawn(async move {
            loop {
                me.notify.notified().await;
                loop {
                    // Take the next id in its own statement so the mutex guard is not held across the await.
                    let next = me.queue.lock().unwrap_or_else(|p| p.into_inner()).pop_front();
                    let Some(id) = next else { break };
                    me.run_pack(&id).await;
                }
            }
        });
    }

    pub fn snapshot(&self) -> Vec<PackView> {
        let states = self.states.lock().unwrap_or_else(|p| p.into_inner());
        self.catalog
            .packs
            .iter()
            .map(|p| PackView {
                pack: p.clone(),
                state: states.get(&p.id).cloned().unwrap_or_else(|| PackState::not_installed(p.size)),
            })
            .collect()
    }

    pub fn state_of(&self, id: &str) -> Option<PackState> {
        self.states.lock().unwrap_or_else(|p| p.into_inner()).get(id).cloned()
    }

    pub fn is_installed(&self, id: &str) -> bool {
        matches!(self.state_of(id), Some(PackState { status: PackStatus::Installed, .. }))
    }

    /// Not installed, or installed in an older version than the catalog's.
    pub fn needs_download(&self, id: &str) -> bool {
        self.state_of(id).is_none_or(|s| s.status != PackStatus::Installed || s.update_available)
    }

    /// The pack's verified files on disk (possibly an older version than the
    /// catalog's, while a newer one downloads). Empty when it has none.
    pub fn installed_files(&self, id: &str) -> Vec<InstalledFile> {
        self.state_of(id).map(|s| s.files).unwrap_or_default()
    }

    pub fn library_dir(&self) -> &Path {
        &self.library
    }

    pub fn enqueue(self: &Arc<Self>, id: &str) -> Result<(), String> {
        let pack = self.catalog.pack(id).ok_or("unknown pack")?.clone();
        {
            let mut states = self.states.lock().unwrap_or_else(|p| p.into_inner());
            let st = states.entry(id.to_string()).or_insert_with(|| PackState::not_installed(pack.size));
            match st.status {
                PackStatus::Installed if !st.update_available => return Err("already installed".into()),
                PackStatus::Queued | PackStatus::Downloading | PackStatus::Verifying => {
                    // Resume right after a pause request: cancel the request.
                    self.pause_requests.lock().unwrap_or_else(|p| p.into_inner()).remove(id);
                    return Ok(());
                }
                _ => {}
            }
            st.status = PackStatus::Queued;
            st.error = None;
            unstale(st, &pack);
        }
        // Asked for by someone: a full download, not just a check.
        self.verify_only.lock().unwrap_or_else(|p| p.into_inner()).remove(id);
        self.pause_requests.lock().unwrap_or_else(|p| p.into_inner()).remove(id);
        self.queue.lock().unwrap_or_else(|p| p.into_inner()).push_back(id.to_string());
        self.save_soon();
        self.notify.notify_one();
        Ok(())
    }

    pub fn pause(self: &Arc<Self>, id: &str) -> Result<(), String> {
        let mut states = self.states.lock().unwrap_or_else(|p| p.into_inner());
        let st = states.get_mut(id).ok_or("unknown pack")?;
        match st.status {
            PackStatus::Queued => {
                self.queue.lock().unwrap_or_else(|p| p.into_inner()).retain(|q| q != id);
                self.verify_only.lock().unwrap_or_else(|p| p.into_inner()).remove(id);
                st.status = PackStatus::Paused;
            }
            PackStatus::Downloading | PackStatus::Verifying => {
                self.pause_requests.lock().unwrap_or_else(|p| p.into_inner()).insert(id.to_string());
            }
            _ => return Err("nothing to pause".into()),
        }
        drop(states);
        self.save_soon();
        Ok(())
    }

    /// Delete a pack's files (finished or partial, of any version) and forget
    /// its state. Blocking. Stop the programs using the pack first (`release`).
    pub fn remove(&self, id: &str) -> Result<(), String> {
        let pack = self.catalog.pack(id).ok_or("unknown pack")?;
        if let Some(st) = self.state_of(id) {
            if matches!(st.status, PackStatus::Downloading | PackStatus::Verifying) {
                return Err("pause the download first".into());
            }
        }
        self.queue.lock().unwrap_or_else(|p| p.into_inner()).retain(|q| q != id);
        self.verify_only.lock().unwrap_or_else(|p| p.into_inner()).remove(id);
        // Forget the pack first: whatever happens below, it no longer counts as installed.
        let old = self
            .states
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id.to_string(), PackState::not_installed(pack.size))
            .unwrap_or_else(|| PackState::not_installed(pack.size));
        // Program folders go before their archives, so a folder that could not
        // be deleted still has what it takes to unpack it again.
        let mut targets: Vec<String> = Vec::new();
        let dirs = old.files.iter().filter_map(|f| f.unpack_to.clone()).chain(pack.files.iter().filter_map(|f| f.unpack_to.clone()));
        let files = old.files.iter().map(|f| f.path.clone()).chain(pack.files.iter().map(|f| f.path.clone()));
        let temps = pack.files.iter().flat_map(|f| [format!("{}.part", f.path), format!("{}.import", f.path)]);
        for t in dirs.chain(files).chain(old.stale.iter().cloned()).chain(temps) {
            if !targets.contains(&t) {
                targets.push(t);
            }
        }
        let mut left = Vec::new();
        let mut first_error = None;
        for rel in targets {
            if let Err(e) = delete_in_library(&self.library, &rel) {
                warn!(pack = id, path = %rel, "could not delete: {e}");
                first_error.get_or_insert_with(|| format!("could not delete {rel}: {e}"));
                left.push(rel);
            }
        }
        // What could not be deleted now is tried again later.
        self.set(id, |s| s.stale = left);
        self.save();
        first_error.map_or(Ok(()), Err)
    }

    /// Queue the packs whose files are in `dir` (a USB stick) or in
    /// `dir/zaklon-packs` to be copied into the library. The copying runs in
    /// the background with progress, and can be paused. Returns the pack ids.
    pub fn import_from_dir(self: &Arc<Self>, dir: &Path) -> Result<Vec<String>, String> {
        let mut queued = Vec::new();
        for pack in &self.catalog.packs {
            if !pack.files.iter().all(|f| find_source(dir, f).is_some()) {
                continue;
            }
            // Claim the pack in one step: a download started in the meantime
            // must not write the same files.
            let claimed = {
                let mut states = self.states.lock().unwrap_or_else(|p| p.into_inner());
                let st = states.entry(pack.id.clone()).or_insert_with(|| PackState::not_installed(pack.size));
                let busy = matches!(st.status, PackStatus::Queued | PackStatus::Downloading | PackStatus::Verifying);
                let current = st.status == PackStatus::Installed && st.matches(pack);
                if busy || current {
                    false
                } else {
                    st.status = PackStatus::Queued;
                    st.error = None;
                    st.import_from = Some(dir.to_string_lossy().into_owned());
                    unstale(st, pack);
                    true
                }
            };
            if claimed {
                self.verify_only.lock().unwrap_or_else(|p| p.into_inner()).remove(&pack.id);
                self.pause_requests.lock().unwrap_or_else(|p| p.into_inner()).remove(&pack.id);
                self.queue.lock().unwrap_or_else(|p| p.into_inner()).push_back(pack.id.clone());
                queued.push(pack.id.clone());
            }
        }
        if !queued.is_empty() {
            info!(packs = queued.len(), dir = %dir.display(), "import queued");
            self.save_soon();
            self.notify.notify_one();
        }
        Ok(queued)
    }

    /// Check an installed pack's files again, once per session: the library
    /// engine could not open one of them. A damaged file stops counting as installed.
    pub fn recheck(self: &Arc<Self>, id: &str) {
        if !self.rechecked.lock().unwrap_or_else(|p| p.into_inner()).insert(id.to_string()) {
            return;
        }
        let queued = {
            let mut states = self.states.lock().unwrap_or_else(|p| p.into_inner());
            match states.get_mut(id) {
                Some(st) if st.status == PackStatus::Installed && !st.files.is_empty() => {
                    st.status = PackStatus::Queued;
                    true
                }
                _ => false,
            }
        };
        if queued {
            self.verify_only.lock().unwrap_or_else(|p| p.into_inner()).insert(id.to_string());
            self.queue.lock().unwrap_or_else(|p| p.into_inner()).push_back(id.to_string());
            self.save_soon();
            self.notify.notify_one();
        }
    }

    // ---- internals ----------------------------------------------------------

    fn set(&self, id: &str, f: impl FnOnce(&mut PackState)) {
        let mut states = self.states.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(st) = states.get_mut(id) {
            f(st);
        }
    }

    /// Write state.json. Blocking: async code uses `save_soon` or `save_now`.
    /// The state lock is held only to take a snapshot, and a newer snapshot
    /// never loses to an older one.
    fn save(&self) {
        let wanted = self.save_requested.fetch_add(1, Ordering::SeqCst) + 1;
        let mut written = self.save_written.lock().unwrap_or_else(|p| p.into_inner());
        if *written >= wanted {
            // Someone else wrote a snapshot taken after this request.
            return;
        }
        // Taken after the write lock, so it includes every change requested so far.
        let (upto, json) = {
            let states = self.states.lock().unwrap_or_else(|p| p.into_inner());
            let upto = self.save_requested.load(Ordering::SeqCst);
            // Only what differs from "not installed": over a thousand map pieces stay out.
            let kept: BTreeMap<&String, &PackState> = states.iter().filter(|(_, s)| !s.is_blank()).collect();
            (upto, serde_json::to_string_pretty(&kept))
        };
        let Ok(json) = json else { return };
        if let Some(parent) = self.state_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match zaklon_core::config::write_atomic(&self.state_path, json.as_bytes()) {
            Ok(()) => *written = upto,
            Err(e) => warn!("saving download state: {e}"),
        }
    }

    /// Save in the background, off the async threads.
    fn save_soon(self: &Arc<Self>) {
        let me = self.clone();
        match tokio::runtime::Handle::try_current() {
            Ok(h) => {
                h.spawn_blocking(move || me.save());
            }
            Err(_) => me.save(),
        }
    }

    /// Save and wait until it is on disk.
    async fn save_now(self: &Arc<Self>) {
        let me = self.clone();
        let _ = tokio::task::spawn_blocking(move || me.save()).await;
    }

    fn pause_requested(&self, id: &str) -> bool {
        self.pause_requests.lock().unwrap_or_else(|p| p.into_inner()).contains(id)
    }

    async fn run_pack(self: &Arc<Self>, id: &str) {
        let Some(pack) = self.catalog.pack(id).cloned() else { return };
        let verify = self.verify_only.lock().unwrap_or_else(|p| p.into_inner()).remove(id);
        let recorded = self.installed_files(id);
        // What is already in place, for progress and the disk space check.
        let (mut done_bytes, mut part_bytes) = (0, 0);
        for f in &pack.files {
            let dest = self.library.join(&f.path);
            if recorded.iter().any(|x| x.is(f)) && dest.is_file() {
                done_bytes += f.size;
            } else {
                part_bytes += std::fs::metadata(part_path(&dest)).map(|m| m.len().min(f.size)).unwrap_or(0);
            }
        }
        // Only a pack still waiting in the queue runs: a pause or a removal in the meantime wins.
        let (job, already) = {
            let mut states = self.states.lock().unwrap_or_else(|p| p.into_inner());
            let Some(st) = states.get_mut(id) else { return };
            if st.status != PackStatus::Queued {
                return;
            }
            let (job, already) = match (&st.import_from, verify) {
                (_, true) => (Job::Verify, 0),
                (Some(dir), false) => (Job::Import(PathBuf::from(dir)), done_bytes),
                (None, false) => (Job::Download, done_bytes + part_bytes),
            };
            st.status = if verify { PackStatus::Verifying } else { PackStatus::Downloading };
            st.bytes_done = already;
            st.bytes_total = pack.size;
            st.error = None;
            st.speed = 0;
            (job, already)
        };
        self.save_soon();
        let outcome = match job {
            Job::Verify => self.verify_pack(id, &pack).await,
            job => self.install_pack(id, &pack, &job, already).await,
        };
        self.finish(id, outcome);
    }

    async fn install_pack(self: &Arc<Self>, id: &str, pack: &Pack, job: &Job, already: u64) -> Outcome {
        if battery_too_low() {
            // Paused, not failed: it continues with one tap once the charger is in.
            return Outcome::Paused;
        }
        // Old files stay until the new ones are verified, so the whole new size must fit.
        if system_info(&self.library).disk_free < pack.size.saturating_sub(already) + DISK_MARGIN {
            return Outcome::Failed("not enough free disk space".into());
        }
        for f in &pack.files {
            match self.install_file(id, pack, f, job).await {
                Outcome::Done => {}
                other => return other,
            }
        }
        // Every file of this version is verified and in place: older ones go.
        let catalog_paths: HashSet<&str> = pack.files.iter().map(|f| f.path.as_str()).collect();
        let mut stale = false;
        self.set(id, |s| {
            let (keep, old): (Vec<InstalledFile>, Vec<InstalledFile>) =
                s.files.drain(..).partition(|x| catalog_paths.contains(x.path.as_str()));
            s.files = keep;
            for o in old {
                s.stale.push(o.path);
            }
            stale = !s.stale.is_empty();
            s.installed_version = Some(pack.version.clone());
            s.update_available = false;
            s.import_from = None;
            s.bytes_done = s.bytes_total;
        });
        self.save_now().await;
        if stale {
            self.release(pack).await;
            let (me, id2) = (self.clone(), id.to_string());
            let _ = tokio::task::spawn_blocking(move || me.sweep(&id2)).await;
        }
        Outcome::Done
    }

    fn finish(self: &Arc<Self>, id: &str, outcome: Outcome) {
        self.pause_requests.lock().unwrap_or_else(|p| p.into_inner()).remove(id);
        self.set(id, |s| {
            s.speed = 0;
            match outcome {
                Outcome::Done => {
                    s.status = PackStatus::Installed;
                    s.error = None;
                    info!(pack = id, "installed");
                }
                Outcome::Paused => {
                    s.status = PackStatus::Paused;
                    info!(pack = id, "paused");
                }
                Outcome::Failed(msg) => {
                    s.status = PackStatus::Failed;
                    warn!(pack = id, "failed: {msg}");
                    s.error = Some(msg);
                    // "Retry" then downloads; importing again is one tap away.
                    s.import_from = None;
                }
            }
        });
        self.save_soon();
    }

    /// Put one file of `pack` in place: keep it if it is already there and
    /// verified, else download or copy it, verify it, and move it into place.
    async fn install_file(self: &Arc<Self>, id: &str, pack: &Pack, f: &PackFile, job: &Job) -> Outcome {
        let dest = self.library.join(&f.path);
        if dest.is_file() {
            let known = self.installed_files(id).into_iter().find(|x| x.path == f.path);
            let good = match known {
                Some(x) if x.is(f) => true,
                // An older version under the same name: replaced below.
                Some(_) => false,
                // Nobody recorded it (state.json was lost, or the power went out
                // right after it was moved into place): check it before trusting it.
                None => {
                    self.set(id, |s| s.status = PackStatus::Verifying);
                    match self.hash_checked(id, &dest).await {
                        Ok(Some(d)) => d.matches(f),
                        Ok(None) => return Outcome::Paused,
                        Err(e) => return Outcome::Failed(e),
                    }
                }
            };
            if good {
                if let Err(e) = self.ensure_unpacked(pack, f.unpack_to.as_deref(), &dest).await {
                    return Outcome::Failed(e);
                }
                self.record_file(id, f);
                self.set(id, |s| s.status = PackStatus::Downloading);
                return Outcome::Done;
            }
        }
        self.set(id, |s| s.status = PackStatus::Downloading);
        let fetched = match job {
            Job::Import(dir) => self.copy_in(id, f, dir).await,
            _ => self.fetch(id, f).await,
        };
        let tmp = match fetched {
            Ok(t) => t,
            Err(o) => return o,
        };
        // Replacing what a program may have open (an older version, or its own folder): stop it first.
        if dest.exists() || f.unpack_to.as_ref().is_some_and(|d| self.library.join(d).exists()) {
            self.release(pack).await;
        }
        // Unpack from the verified temporary file first, then rename: a
        // finished file therefore always means "verified and unpacked".
        let (lib, unpack_to, t2, d2) = (self.library.clone(), f.unpack_to.clone(), tmp.clone(), dest.clone());
        let placed = tokio::task::spawn_blocking(move || {
            unpack(&lib, unpack_to.as_deref(), &t2)?;
            rename_retry(&t2, &d2).map_err(|e| format!("moving file into place: {e}"))
        })
        .await
        .unwrap_or_else(|e| Err(format!("install task: {e}")));
        if let Err(e) = placed {
            if matches!(job, Job::Import(_)) {
                let _ = std::fs::remove_file(&tmp);
            }
            return Outcome::Failed(e);
        }
        // An imported file replaces any partial download of it.
        let _ = std::fs::remove_file(part_path(&dest));
        self.record_file(id, f);
        self.save_soon();
        self.set(id, |s| s.status = PackStatus::Downloading);
        Outcome::Done
    }

    /// Note that `f` is on disk and verified.
    fn record_file(&self, id: &str, f: &PackFile) {
        self.set(id, |s| {
            s.files.retain(|x| x.path != f.path);
            s.files.push(InstalledFile::from(f));
            s.stale.retain(|p| *p != f.path && Some(p) != f.unpack_to.as_ref());
            s.synced.remove(&f.path);
        });
    }

    /// Check the files a pack has on disk: the recorded ones, or else the
    /// catalog's (found without a record). Nothing is downloaded.
    async fn verify_pack(self: &Arc<Self>, id: &str, pack: &Pack) -> Outcome {
        let recorded = self.installed_files(id);
        let adopting = recorded.is_empty();
        let files: Vec<InstalledFile> = if adopting { pack.files.iter().map(InstalledFile::from).collect() } else { recorded };
        let total: u64 = files.iter().map(|f| f.size).sum();
        self.set(id, |s| {
            s.bytes_done = 0;
            s.bytes_total = total;
        });
        for f in &files {
            let path = self.library.join(&f.path);
            let good = std::fs::metadata(&path).is_ok_and(|m| m.len() == f.size)
                && match self.hash_checked(id, &path).await {
                    Ok(Some(d)) => d.matches_hash(&f.sha256, f.sha1_base64.as_deref()),
                    Ok(None) => return Outcome::Paused,
                    Err(_) => false,
                };
            if !good {
                warn!(pack = id, path = %f.path, "the file on disk does not match");
                // It is not used any more. Files of an older version go; one
                // under the catalog's name is replaced by the next download.
                self.set(id, |s| {
                    for x in s.files.drain(..) {
                        if !pack.files.iter().any(|c| c.path == x.path) {
                            s.stale.push(x.path);
                        }
                    }
                    s.update_available = false;
                });
                return Outcome::Failed("the file on disk is damaged or another version; download it again".into());
            }
            self.set(id, |s| s.bytes_done += f.size);
        }
        for f in &files {
            if let Err(e) = self.ensure_unpacked(pack, f.unpack_to.as_deref(), &self.library.join(&f.path)).await {
                return Outcome::Failed(e);
            }
        }
        self.set(id, |s| {
            if adopting {
                s.installed_version = Some(pack.version.clone());
            }
            s.stale.retain(|p| !files.iter().any(|f| f.path == *p || f.unpack_to.as_ref() == Some(p)));
            s.files = files;
            s.update_available = !s.matches(pack);
            s.import_from = None;
            s.bytes_done = s.bytes_total;
        });
        Outcome::Done
    }

    /// Unpack a verified archive again if its folder is incomplete.
    async fn ensure_unpacked(&self, pack: &Pack, unpack_to: Option<&str>, archive: &Path) -> Result<(), String> {
        let Some(dir) = unpack_to else { return Ok(()) };
        if unpacked_ok(&self.library, Some(dir)) {
            return Ok(());
        }
        // The program may still be running from a half-updated folder.
        self.release(pack).await;
        let (lib, dir, archive) = (self.library.clone(), dir.to_string(), archive.to_path_buf());
        tokio::task::spawn_blocking(move || unpack(&lib, Some(&dir), &archive))
            .await
            .unwrap_or_else(|e| Err(format!("unpack task: {e}")))
    }

    /// SHA-256 and SHA-1 of a file, off the async threads. `Ok(None)` when a pause was asked for.
    async fn hash_checked(self: &Arc<Self>, id: &str, path: &Path) -> Result<Option<FileDigest>, String> {
        let (me, id2, p) = (self.clone(), id.to_string(), path.to_path_buf());
        tokio::task::spawn_blocking(move || hash_file(&p, || me.pause_requested(&id2)))
            .await
            .map_err(|e| format!("verify task: {e}"))?
            .map_err(|e| format!("reading file: {e}"))
    }

    /// Download `f` into its `.part` file and verify it. Returns the `.part`.
    async fn fetch(self: &Arc<Self>, id: &str, f: &PackFile) -> Result<PathBuf, Outcome> {
        let dest = self.library.join(&f.path);
        if let Some(parent) = dest.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return Err(Outcome::Failed(format!("creating folder: {e}")));
            }
        }
        let part = part_path(&dest);
        let len = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        let synced = self.state_of(id).and_then(|s| s.synced.get(&f.path).copied());
        let keep = resume_point(len, synced, f.size);
        if keep < len {
            // After a power cut the end of a partial file may never have reached
            // the disk (it reads back as zeros): continue from what surely did.
            let cut = std::fs::OpenOptions::new().write(true).open(&part).and_then(|file| file.set_len(keep));
            if cut.is_err() {
                let _ = std::fs::remove_file(&part);
                self.set(id, |s| {
                    s.synced.remove(&f.path);
                });
            }
            self.set(id, |s| s.bytes_done = s.bytes_done.saturating_sub(len - keep));
        }
        let mut have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        if have < f.size {
            let mut last_err = String::from("no download locations");
            let mut done = false;
            for url in &f.urls {
                match self.fetch_range(id, f, url, &part, &mut have).await {
                    Ok(true) => {
                        done = true;
                        break;
                    }
                    Ok(false) => return Err(Outcome::Paused),
                    Err(e) => {
                        warn!(pack = id, url, "download error: {e}");
                        last_err = e;
                    }
                }
            }
            if !done {
                return Err(Outcome::Failed(format!("download failed: {last_err}")));
            }
        }

        self.set(id, |s| {
            s.status = PackStatus::Verifying;
            s.speed = 0;
        });
        self.save_soon();
        match self.hash_checked(id, &part).await {
            Ok(Some(d)) if d.matches(f) => Ok(part),
            Ok(Some(_)) => {
                let _ = std::fs::remove_file(&part);
                self.set(id, |s| {
                    s.synced.remove(&f.path);
                    s.bytes_done = s.bytes_done.saturating_sub(f.size);
                });
                Err(Outcome::Failed("checksum mismatch, the file was discarded; try again".into()))
            }
            Ok(None) => Err(Outcome::Paused),
            Err(e) => Err(Outcome::Failed(e)),
        }
    }

    /// Download `url` into `part` starting at offset `*have`. Ok(true) when the
    /// file is complete, Ok(false) when paused, Err on a network problem.
    async fn fetch_range(self: &Arc<Self>, id: &str, f: &PackFile, url: &str, part: &Path, have: &mut u64) -> Result<bool, String> {
        // What is really on disk decides where to continue: a write that failed
        // halfway (disk full) may have left more bytes than were counted, and a
        // second mirror appending after them would corrupt the file.
        *have = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);
        let mut req = self.client.get(url);
        if *have > 0 {
            req = req.header("range", format!("bytes={}-", *have));
        }
        let res = req.send().await.map_err(|e| e.to_string())?;
        let status = res.status();
        if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE && *have > 0 {
            // The server says there is nothing past what we have: let the checksum decide.
            return Ok(true);
        }
        // Trust the server's idea of the total over the catalog; the SHA-256 check is the real authority.
        let header = |name: &str| res.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
        let partial = status == reqwest::StatusCode::PARTIAL_CONTENT;
        let server_total: Option<u64> = if partial {
            header("content-range").and_then(|s| s.rsplit('/').next().and_then(|t| t.trim().parse().ok()))
        } else {
            header("content-length").and_then(|t| t.trim().parse().ok())
        };
        if header("content-type").is_some_and(|t| t.to_ascii_lowercase().starts_with("text/html")) {
            return Err("this address returns a web page, not the pack file".into());
        }
        // The reply must continue exactly where the file ends; any other part would shift the file.
        if partial && header("content-range").as_deref().and_then(range_start) != Some(*have) {
            return Err("this mirror cannot resume downloads (it sent another part of the file)".into());
        }
        let expected = server_total.unwrap_or(f.size);
        if expected != f.size {
            warn!(pack = id, path = %f.path, catalog = f.size, server = expected, "size differs from catalog");
        }
        // Hard limit: never write more than a little over the expected size.
        let limit = expected.max(f.size) + 1024 * 1024;
        let mut file = if *have > 0 && partial {
            std::fs::OpenOptions::new().append(true).open(part).map_err(|e| e.to_string())?
        } else if status.is_success() {
            if *have > 0 {
                // The server ignored the range. Starting over throws away progress, so
                // only do it when this is clearly the whole file; otherwise try another mirror.
                if server_total != Some(f.size) {
                    return Err("this mirror cannot resume downloads".into());
                }
                self.set(id, |s| s.bytes_done = s.bytes_done.saturating_sub(*have));
                *have = 0;
            }
            // Starting from nothing: none of what follows is on the disk yet,
            // and that must be on record before any of it is written.
            self.set(id, |s| {
                s.synced.insert(f.path.clone(), 0);
            });
            self.save_now().await;
            std::fs::File::create(part).map_err(|e| e.to_string())?
        } else {
            return Err(format!("server replied {status}"));
        };

        let mut too_much = false;
        let result: Result<bool, String> = async {
            let mut stream = res.bytes_stream();
            let mut last_save = Instant::now();
            let mut last_sync = Instant::now();
            let mut tick = Instant::now();
            let mut tick_bytes: u64 = 0;
            let mut idle = Duration::ZERO;
            loop {
                // Wake up every second so a pause takes effect even when no data arrives.
                let next = match tokio::time::timeout(Duration::from_secs(1), stream.next()).await {
                    Ok(n) => n,
                    Err(_) => {
                        if self.pause_requested(id) {
                            return Ok(false);
                        }
                        idle += Duration::from_secs(1);
                        if idle >= STALL_TIMEOUT {
                            return Err("no data for 60 seconds".into());
                        }
                        continue;
                    }
                };
                let Some(chunk) = next else { break };
                idle = Duration::ZERO;
                let chunk = chunk.map_err(|e| e.to_string())?;
                if *have + chunk.len() as u64 > limit {
                    too_much = true;
                    return Err("the server sent more data than expected".into());
                }
                file.write_all(&chunk).map_err(|e| e.to_string())?;
                let n = chunk.len() as u64;
                *have += n;
                tick_bytes += n;
                self.set(id, |s| s.bytes_done += n);
                if tick.elapsed() >= Duration::from_secs(1) {
                    let speed = (tick_bytes as f64 / tick.elapsed().as_secs_f64()) as u64;
                    self.set(id, |s| s.speed = speed);
                    tick = Instant::now();
                    tick_bytes = 0;
                }
                if last_save.elapsed() >= STATE_SAVE_INTERVAL {
                    self.save_soon();
                    last_save = Instant::now();
                    // Put what was written so far really on the disk, now and then.
                    if last_sync.elapsed() >= SYNC_INTERVAL {
                        self.sync_part(id, &f.path, &file, *have).await;
                        last_sync = Instant::now();
                    }
                    // The battery rule holds for the whole download, not just its start:
                    // a long download on a laptop that was unplugged pauses in time.
                    if battery_too_low() {
                        info!(pack = id, "battery low while downloading; pausing");
                        return Ok(false);
                    }
                }
                if self.pause_requested(id) {
                    return Ok(false);
                }
            }
            if *have < expected {
                return Err(format!("connection ended early at {} of {} bytes", *have, expected));
            }
            Ok::<bool, String>(true)
        }
        .await;
        if too_much {
            drop(file);
            let _ = std::fs::remove_file(part);
            self.set(id, |s| {
                s.bytes_done = s.bytes_done.saturating_sub(*have);
                s.synced.remove(&f.path);
            });
            *have = 0;
        } else {
            // Whatever happened, what was written goes onto the disk and is
            // where a later resume starts. A finished file is thus durable
            // before it is verified and moved into place.
            self.sync_part(id, &f.path, &file, *have).await;
        }
        result
    }

    /// Force `file`'s data onto the disk, then record `len` as its durable length.
    async fn sync_part(self: &Arc<Self>, id: &str, path: &str, file: &std::fs::File, len: u64) {
        let Ok(handle) = file.try_clone() else { return };
        let (me, id, path) = (self.clone(), id.to_string(), path.to_string());
        let _ = tokio::task::spawn_blocking(move || {
            if handle.sync_data().is_ok() {
                me.set(&id, |s| {
                    s.synced.insert(path, len);
                });
                me.save();
            }
        })
        .await;
    }

    /// Copy `f` from the folder `dir` (a USB stick) into its `.import` file,
    /// hashing on the way. Returns the verified `.import`.
    async fn copy_in(self: &Arc<Self>, id: &str, f: &PackFile, dir: &Path) -> Result<PathBuf, Outcome> {
        let Some(src) = find_source(dir, f) else {
            return Err(Outcome::Failed(format!("the drive or folder to import {} from is not there any more; connect it and import again", f.path)));
        };
        let dest = self.library.join(&f.path);
        if let Some(parent) = dest.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return Err(Outcome::Failed(format!("creating folder: {e}")));
            }
        }
        // Its own temporary file: a paused download's ".part" (with its
        // progress) is left alone, and a failed copy leaves nothing behind.
        let tmp = import_path(&dest);
        let (me, id2, t2) = (self.clone(), id.to_string(), tmp.clone());
        let copied = tokio::task::spawn_blocking(move || me.copy_with_hash(&id2, &src, &t2))
            .await
            .unwrap_or_else(|e| Err(std::io::Error::other(e.to_string())));
        let result = match copied {
            Ok(Some(d)) if d.matches(f) => return Ok(tmp),
            Ok(Some(_)) => Outcome::Failed(format!("checksum mismatch for {}", f.path)),
            Ok(None) => Outcome::Paused,
            Err(e) => Outcome::Failed(format!("copying {}: {e}", f.path)),
        };
        let _ = std::fs::remove_file(&tmp);
        Err(result)
    }

    /// Blocking copy with progress and pause. `Ok(None)` when paused. The copy
    /// is on the disk (synced) before its hash is trusted.
    fn copy_with_hash(&self, id: &str, src: &Path, dest: &Path) -> std::io::Result<Option<FileDigest>> {
        let mut input = std::fs::File::open(src)?;
        let mut output = std::fs::File::create(dest)?;
        let mut h = Hashers::new();
        let mut buf = vec![0u8; 1 << 20];
        let mut copied: u64 = 0;
        let (mut tick, mut tick_bytes) = (Instant::now(), 0u64);
        let outcome = loop {
            if self.pause_requested(id) {
                break Ok(None);
            }
            let n = match input.read(&mut buf) {
                Ok(0) => break Ok(Some(())),
                Ok(n) => n,
                Err(e) => break Err(e),
            };
            h.update(&buf[..n]);
            if let Err(e) = output.write_all(&buf[..n]) {
                break Err(e);
            }
            copied += n as u64;
            tick_bytes += n as u64;
            self.set(id, |s| s.bytes_done += n as u64);
            if tick.elapsed() >= Duration::from_secs(1) {
                let speed = (tick_bytes as f64 / tick.elapsed().as_secs_f64()) as u64;
                self.set(id, |s| s.speed = speed);
                (tick, tick_bytes) = (Instant::now(), 0);
            }
        };
        let finished = outcome.and_then(|o| match o {
            Some(()) => output.sync_all().map(|_| true),
            None => Ok(false),
        });
        if !matches!(finished, Ok(true)) {
            // The copy starts over next time; progress shows only what is kept.
            self.set(id, |s| s.bytes_done = s.bytes_done.saturating_sub(copied));
        }
        Ok(finished?.then(|| h.finish()))
    }

    /// Delete a pack's stale files and folders. What is still in use stays
    /// listed and is tried again later. Blocking.
    fn sweep(&self, id: &str) {
        let Some(st) = self.state_of(id) else { return };
        if st.stale.is_empty() {
            return;
        }
        // Never what the current files are, or unpack into.
        let in_use = |p: &String| st.files.iter().any(|f| f.path == *p || f.unpack_to.as_ref() == Some(p));
        let mut done = Vec::new();
        for rel in &st.stale {
            if in_use(rel) {
                done.push(rel.clone());
                continue;
            }
            match delete_in_library(&self.library, rel) {
                Ok(()) => {
                    info!(pack = id, path = %rel, "old file deleted");
                    done.push(rel.clone());
                }
                Err(e) => warn!(pack = id, path = %rel, "old file still in use: {e}"),
            }
        }
        self.set(id, |s| s.stale.retain(|p| !done.contains(p)));
        self.save();
    }
}

/// Bring a saved pack state in line with the catalog and the disk at startup.
/// Returns true when the catalog's files were found on disk without a
/// record: they are verified before they count as installed.
fn reconcile(library: &Path, p: &Pack, st: &mut PackState) -> bool {
    // state.json is ours, but only ever act on paths inside the library.
    st.files.retain(|f| is_safe_relative(&f.path) && f.unpack_to.as_deref().is_none_or(is_safe_relative));
    st.stale.retain(|s| is_safe_relative(s));
    // A state file from before files were recorded: an installed pack of the
    // catalog's version had exactly the catalog's files.
    if st.files.is_empty() && st.status == PackStatus::Installed {
        if st.installed_version.as_deref() == Some(p.version.as_str()) && pack_complete_on_disk(library, p) {
            st.files = p.files.iter().map(InstalledFile::from).collect();
        } else {
            let stale = std::mem::take(&mut st.stale);
            *st = PackState::not_installed(p.size);
            st.stale = stale;
        }
    }
    // Recorded files that are gone (deleted by hand, an engine folder emptied)
    // leave nothing usable.
    if !st.files.is_empty() && !st.files.iter().all(|f| library.join(&f.path).is_file() && unpacked_ok(library, f.unpack_to.as_deref())) {
        warn!(pack = %p.id, "files of an installed pack are missing");
        let stale = std::mem::take(&mut st.stale);
        *st = PackState::not_installed(p.size);
        st.stale = stale;
    }
    st.update_available = !st.files.is_empty() && !st.matches(p);
    if st.status == PackStatus::Installed {
        st.bytes_total = p.size;
        st.bytes_done = p.size;
    }
    // (Not after a failure: a file that failed its check is not checked again at every start.)
    let unknown_files = st.files.is_empty()
        && matches!(st.status, PackStatus::NotInstalled | PackStatus::Paused)
        && p.files.iter().all(|f| library.join(&f.path).is_file());
    if unknown_files {
        st.status = PackStatus::Queued;
        st.error = None;
    }
    unknown_files
}

/// A new download or import of `pack` writes its own paths again: a failed
/// removal's leftovers there must not be swept away later.
fn unstale(st: &mut PackState, pack: &Pack) {
    st.stale.retain(|p| {
        !pack.files.iter().any(|f| {
            *p == f.path || *p == format!("{}.part", f.path) || *p == format!("{}.import", f.path) || f.unpack_to.as_ref() == Some(p)
        })
    });
}

/// Where a partial download continues: only what is known to be on the disk.
fn resume_point(len: u64, synced: Option<u64>, size: u64) -> u64 {
    if len > size {
        return 0;
    }
    match synced {
        Some(s) => len.min(s),
        // No record: trust all but the end.
        None if len > RESUME_OVERLAP && len < size => len - RESUME_OVERLAP,
        None => len,
    }
}

/// The first byte of a `Content-Range: bytes START-END/TOTAL` reply.
fn range_start(value: &str) -> Option<u64> {
    value.trim().strip_prefix("bytes")?.trim().split('-').next()?.trim().parse().ok()
}

/// A pack file on a USB stick: in `dir` or `dir/zaklon-packs`, under its own
/// name and with the right size (a stick made for another map version has
/// files of the same name that would only fail the checksum after copying).
fn find_source(dir: &Path, f: &PackFile) -> Option<PathBuf> {
    let name = Path::new(&f.path).file_name()?;
    [dir.to_path_buf(), dir.join(crate::export::FOLDER)]
        .into_iter()
        .map(|c| c.join(name))
        .find(|p| std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.len() == f.size))
}

/// Delete a file or folder inside the library; one that is already gone is fine.
fn delete_in_library(library: &Path, rel: &str) -> std::io::Result<()> {
    if !is_safe_relative(rel) {
        return Ok(());
    }
    let path = library.join(rel);
    let r = if path.is_dir() { std::fs::remove_dir_all(&path) } else { remove_file_retry(&path) };
    match r {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// All files of a pack are in place (and unpacked where needed).
fn pack_complete_on_disk(library: &Path, p: &Pack) -> bool {
    p.files.iter().all(|f| library.join(&f.path).is_file() && unpacked_ok(library, f.unpack_to.as_deref()))
}

fn unpacked_ok(library: &Path, unpack_to: Option<&str>) -> bool {
    unpack_to.is_none_or(|dir| library.join(dir).join(UNPACKED_MARKER).is_file())
}

/// On Windows a freshly written file can be briefly locked (antivirus scan,
/// search indexer). Retry for about six seconds before giving up. Blocking.
fn rename_retry(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut delay = Duration::from_millis(100);
    let mut last = None;
    for _ in 0..6 {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) => last = Some(e),
        }
        std::thread::sleep(delay);
        delay *= 2;
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("rename failed")))
}

fn remove_file_retry(path: &Path) -> std::io::Result<()> {
    let mut last = None;
    for _ in 0..6 {
        match std::fs::remove_file(path) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(e),
            Err(e) => last = Some(e),
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("delete failed")))
}

fn part_path(dest: &Path) -> PathBuf {
    let mut p = dest.as_os_str().to_owned();
    p.push(".part");
    PathBuf::from(p)
}

fn import_path(dest: &Path) -> PathBuf {
    let mut p = dest.as_os_str().to_owned();
    p.push(".import");
    PathBuf::from(p)
}

/// SHA-256 and SHA-1 of a file, computed in one pass. Our catalog uses
/// SHA-256; CoMaps publishes SHA-1 (base64) for its map files.
struct FileDigest {
    sha256: String,
    sha1_base64: String,
}

impl FileDigest {
    fn matches(&self, f: &PackFile) -> bool {
        self.matches_hash(&f.sha256, f.sha1_base64.as_deref())
    }

    fn matches_hash(&self, sha256: &str, sha1_base64: Option<&str>) -> bool {
        if !sha256.is_empty() {
            return self.sha256.eq_ignore_ascii_case(sha256);
        }
        sha1_base64.is_some_and(|h| h == self.sha1_base64)
    }
}

struct Hashers {
    sha256: Sha256,
    sha1: sha1::Sha1,
}

impl Hashers {
    fn new() -> Self {
        Self { sha256: Sha256::new(), sha1: sha1::Sha1::new() }
    }
    fn update(&mut self, b: &[u8]) {
        self.sha256.update(b);
        sha1::Digest::update(&mut self.sha1, b);
    }
    fn finish(self) -> FileDigest {
        use base64::Engine;
        FileDigest {
            sha256: self.sha256.finalize().iter().map(|b| format!("{b:02x}")).collect(),
            sha1_base64: base64::engine::general_purpose::STANDARD.encode(sha1::Digest::finalize(self.sha1)),
        }
    }
}

/// Hash a file; `Ok(None)` when `stop` says to give up (a pause). Blocking.
fn hash_file(path: &Path, stop: impl Fn() -> bool) -> std::io::Result<Option<FileDigest>> {
    let mut file = std::fs::File::open(path)?;
    let mut h = Hashers::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        if stop() {
            return Ok(None);
        }
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(Some(h.finish()))
}

/// Unpack a zip archive into `unpack_to` (inside the library), replacing
/// what was there. Nothing to do for a file that is not an archive. Blocking.
fn unpack(library: &Path, unpack_to: Option<&str>, archive: &Path) -> Result<(), String> {
    let Some(rel) = unpack_to else { return Ok(()) };
    if !is_safe_relative(rel) {
        return Err(format!("refusing to unpack outside the library: {rel}"));
    }
    let target = library.join(rel);
    let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("opening archive: {e}"))?;
    let names: Vec<PathBuf> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().and_then(|e| e.enclosed_name()))
        .collect();
    let strip = shared_top_folder(&names);
    match std::fs::remove_dir_all(&target) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(format!("could not delete {rel}: {e}")),
        _ => {}
    }
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let Some(name) = entry.enclosed_name() else { continue };
        let rel_out: PathBuf = match &strip {
            Some(top) => name.strip_prefix(top).map(Path::to_path_buf).unwrap_or(name),
            None => name,
        };
        if rel_out.as_os_str().is_empty() {
            continue;
        }
        let out = target.join(rel_out);
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut dest = std::fs::File::create(&out).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut dest).map_err(|e| e.to_string())?;
        // On the disk before the folder is marked complete.
        dest.sync_all().map_err(|e| e.to_string())?;
    }
    let marker = std::fs::File::create(target.join(UNPACKED_MARKER)).and_then(|mut m| {
        m.write_all(b"ok")?;
        m.sync_all()
    });
    marker.map_err(|e| e.to_string())?;
    info!(archive = %archive.display(), target = %target.display(), "unpacked");
    Ok(())
}

/// The one folder every entry of an archive sits in
/// (`kiwix-tools_win-x86_64-3.8.1/kiwix-serve.exe`), which unpacking leaves
/// out. Archives with anything else at the top are unpacked as they are.
fn shared_top_folder(names: &[PathBuf]) -> Option<PathBuf> {
    let first = names.iter().find_map(|n| n.components().next())?;
    let top = PathBuf::from(first.as_os_str());
    let all_inside = names.iter().all(|n| n.starts_with(&top));
    let any_below = names.iter().any(|n| n.components().count() > 1);
    (all_inside && any_below).then_some(top)
}

pub fn system_info(dir: &Path) -> SystemInfo {
    let probe = if dir.exists() { dir.to_path_buf() } else { dir.parent().map(Path::to_path_buf).unwrap_or_else(|| dir.to_path_buf()) };
    let disk_free = fs4::available_space(&probe).unwrap_or(0);
    let disk_total = fs4::total_space(&probe).unwrap_or(0);
    let (battery_percent, plugged_in) = battery();
    SystemInfo { disk_free, disk_total, battery_percent, plugged_in }
}

/// On battery and below the minimum (ZAKLON_IGNORE_BATTERY=1 turns the rule
/// off for automated tests on laptops running on battery).
fn battery_too_low() -> bool {
    if std::env::var("ZAKLON_IGNORE_BATTERY").is_ok_and(|v| v == "1") {
        return false;
    }
    let (percent, plugged) = battery();
    !plugged && percent.is_some_and(|p| p < MIN_BATTERY_PERCENT)
}

#[cfg(windows)]
fn battery() -> (Option<u8>, bool) {
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut s = SYSTEM_POWER_STATUS {
        ACLineStatus: 255,
        BatteryFlag: 255,
        BatteryLifePercent: 255,
        SystemStatusFlag: 0,
        BatteryLifeTime: 0,
        BatteryFullLifeTime: 0,
    };
    // SAFETY: GetSystemPowerStatus only writes into the struct we pass.
    if unsafe { GetSystemPowerStatus(&mut s) } == 0 {
        return (None, true);
    }
    let no_battery = s.BatteryFlag & 128 != 0;
    let percent = if s.BatteryLifePercent == 255 { None } else { Some(s.BatteryLifePercent) };
    (percent, s.ACLineStatus == 1 || no_battery)
}

#[cfg(not(windows))]
fn battery() -> (Option<u8>, bool) {
    (None, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zaklon_core::catalog::{Category, Localized};

    fn temp(name: &str) -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let d = std::env::temp_dir().join(format!("zaklon-dl-{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn payload(seed: u32, len: usize) -> Vec<u8> {
        (0..len as u32).map(|i| (i.wrapping_add(seed).wrapping_mul(2654435761) >> 24) as u8).collect()
    }

    fn sha256_hex(b: &[u8]) -> String {
        Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
    }

    fn file(path: &str, bytes: &[u8], url: &str) -> PackFile {
        PackFile {
            path: path.into(),
            urls: vec![url.into()],
            sha256: sha256_hex(bytes),
            sha1_base64: None,
            size: bytes.len() as u64,
            unpack: None,
            unpack_to: None,
        }
    }

    fn pack(id: &str, version: &str, f: PackFile) -> Pack {
        Pack {
            id: id.into(),
            title: Localized { en: id.into(), sr: String::new() },
            description: Localized::default(),
            category: Category::Knowledge,
            version: version.into(),
            size: f.size,
            files: vec![f],
            license: String::new(),
            attribution: String::new(),
            source: String::new(),
            languages: vec![],
            recommended_for: vec![],
        }
    }

    fn catalog(packs: Vec<Pack>) -> Catalog {
        Catalog { version: 1, generated: "2999-01-01".into(), packs }
    }

    /// A hub data folder with `state.json` (when given) and a library.
    fn hub_dir(state: Option<serde_json::Value>) -> (PathBuf, PathBuf) {
        let root = temp("hub");
        let library = root.join("library");
        std::fs::create_dir_all(&library).unwrap();
        let state_path = root.join("state.json");
        if let Some(s) = state {
            std::fs::write(&state_path, s.to_string()).unwrap();
        }
        (library, state_path)
    }

    fn put(library: &Path, rel: &str, bytes: &[u8]) {
        let p = library.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    /// Serve `dir` over HTTP with Range support.
    async fn serve(dir: &Path) -> String {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = axum::Router::new().fallback_service(tower_http::services::ServeDir::new(dir));
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    }

    async fn wait(d: &Downloads, id: &str, until: &[PackStatus]) -> PackState {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let st = d.state_of(id).unwrap();
            if until.contains(&st.status) {
                return st;
            }
            assert!(Instant::now() < deadline, "{id} stuck in {:?}", st.status);
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    fn no_battery_rule() {
        std::env::set_var("ZAKLON_IGNORE_BATTERY", "1");
    }

    #[test]
    fn part_path_appends_suffix() {
        assert!(part_path(Path::new("a/b.zim")).to_string_lossy().ends_with("b.zim.part"));
        assert!(import_path(Path::new("a/b.zim")).to_string_lossy().ends_with("b.zim.import"));
    }

    #[test]
    fn hashing_matches_known_value() {
        let dir = temp("hash");
        let p = dir.join("x.bin");
        std::fs::write(&p, b"abc").unwrap();
        let d = hash_file(&p, || false).unwrap().unwrap();
        assert_eq!(d.sha256, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(d.sha1_base64, "qZk+NkcGgWq6PiVxeFDCbJzQ2J0=", "SHA-1 of abc");
        assert!(hash_file(&p, || true).unwrap().is_none(), "a pause stops the check");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resumes_only_from_what_reached_the_disk() {
        let mb = 1 << 20;
        // A recorded durable length wins over a longer file (its end may be zeros after a power cut).
        assert_eq!(resume_point(900 * mb, Some(300 * mb), 1000 * mb), 300 * mb);
        assert_eq!(resume_point(200 * mb, Some(300 * mb), 1000 * mb), 200 * mb);
        assert_eq!(resume_point(1000 * mb, Some(1000 * mb), 1000 * mb), 1000 * mb);
        assert_eq!(resume_point(1000 * mb, Some(990 * mb), 1000 * mb), 990 * mb, "finished but not synced");
        assert_eq!(resume_point(50 * mb, Some(0), 1000 * mb), 0, "started over, nothing synced yet");
        // Without a record, all but the end.
        assert_eq!(resume_point(900 * mb, None, 1000 * mb), 900 * mb - RESUME_OVERLAP);
        assert_eq!(resume_point(mb, None, 1000 * mb), mb);
        // Longer than the file can be: start over.
        assert_eq!(resume_point(1001 * mb, Some(10), 1000 * mb), 0);
    }

    #[test]
    fn reads_the_start_of_a_range_reply() {
        assert_eq!(range_start("bytes 1234-9999/10000"), Some(1234));
        assert_eq!(range_start(" bytes 0-9/10"), Some(0));
        assert_eq!(range_start("bytes */10000"), None);
        assert_eq!(range_start("items 1-2/3"), None);
    }

    #[test]
    fn strips_only_a_folder_all_entries_share() {
        let p = |v: &[&str]| v.iter().map(PathBuf::from).collect::<Vec<_>>();
        assert_eq!(shared_top_folder(&p(&["kt-3.8.1", "kt-3.8.1/kiwix-serve.exe", "kt-3.8.1/icu.dll"])), Some(PathBuf::from("kt-3.8.1")));
        assert_eq!(shared_top_folder(&p(&["kiwix-serve.exe", "icu.dll"])), None, "flat archive");
        assert_eq!(shared_top_folder(&p(&["a.dll", "sub/x.dll"])), None, "files at the top stay where they are");
        assert_eq!(shared_top_folder(&p(&["one/a.dll", "two/b.dll"])), None);
        assert_eq!(shared_top_folder(&[]), None);
    }

    #[test]
    fn startup_matches_saved_state_with_catalog_and_disk() {
        let (library, _) = hub_dir(None);
        let old = payload(1, 1000);
        let new = payload(2, 1200);
        put(&library, "zim/x_2026-01.zim", &old);
        let p1 = pack("x", "2026-01", file("zim/x_2026-01.zim", &old, "http://x/"));
        let p2 = pack("x", "2026-02", file("zim/x_2026-02.zim", &new, "http://x/"));

        // A state file from before files were recorded, same version: the catalog's files are adopted.
        let mut st: PackState = serde_json::from_value(serde_json::json!({ "status": "installed", "bytes_done": 1000, "bytes_total": 1000, "installed_version": "2026-01" })).unwrap();
        assert!(!reconcile(&library, &p1, &mut st));
        assert_eq!(st.status, PackStatus::Installed);
        assert_eq!(st.files, vec![InstalledFile::from(&p1.files[0])]);
        assert!(!st.update_available);

        // A newer catalog: still installed and in use, with an update offered.
        assert!(!reconcile(&library, &p2, &mut st));
        assert_eq!(st.status, PackStatus::Installed);
        assert_eq!(st.files[0].path, "zim/x_2026-01.zim", "the old file keeps working");
        assert!(st.update_available);

        // Recorded files that are gone: not installed any more.
        std::fs::remove_file(library.join("zim/x_2026-01.zim")).unwrap();
        assert!(!reconcile(&library, &p2, &mut st));
        assert_eq!(st.status, PackStatus::NotInstalled);
        assert!(st.files.is_empty() && !st.update_available);

        // No record (state.json lost) but a file at the catalog's path: it is checked, never trusted.
        put(&library, "zim/x_2026-02.zim", &new);
        let mut lost = PackState::not_installed(p2.size);
        assert!(reconcile(&library, &p2, &mut lost), "to be verified");
        assert_eq!(lost.status, PackStatus::Queued);
        assert!(lost.files.is_empty() && lost.installed_version.is_none());

        // An old state naming another version is not trusted either.
        let mut stale: PackState = serde_json::from_value(serde_json::json!({ "status": "installed", "bytes_done": 1, "bytes_total": 1, "installed_version": "2026-01" })).unwrap();
        assert!(reconcile(&library, &p2, &mut stale));
        assert!(stale.files.is_empty() && stale.installed_version.is_none());

        // Paths in state.json never leave the library.
        let mut evil: PackState = serde_json::from_value(serde_json::json!({ "status": "not_installed", "bytes_done": 0, "bytes_total": 0, "stale": ["../../Windows", "zim/old.zim"] })).unwrap();
        reconcile(&library, &p2, &mut evil);
        assert_eq!(evil.stale, vec!["zim/old.zim".to_string()]);
        let _ = std::fs::remove_dir_all(library.parent().unwrap());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn files_found_without_a_record_are_verified_first() {
        no_battery_rule();
        let good = payload(3, 300_000);
        let files = temp("files");
        std::fs::write(files.join("b.zim"), &good).unwrap();
        let server = serve(&files).await;
        let (library, state_path) = hub_dir(None);
        // What an older version left under the same name.
        put(&library, "zim/a.zim", &payload(4, 300_000));
        put(&library, "zim/b.zim", &good);
        let d = Downloads::new(
            catalog(vec![pack("a", "2", file("zim/a.zim", &good, &format!("{server}/b.zim"))), pack("b", "1", file("zim/b.zim", &good, "http://127.0.0.1:9/b.zim"))]),
            library.clone(),
            state_path,
        );
        d.start();
        let a = wait(&d, "a", &[PackStatus::Installed, PackStatus::Failed]).await;
        assert_eq!(a.status, PackStatus::Failed, "a file that does not match is never installed");
        assert!(a.files.is_empty());
        assert!(a.error.unwrap().contains("another version"));
        let b = wait(&d, "b", &[PackStatus::Installed, PackStatus::Failed]).await;
        assert_eq!(b.status, PackStatus::Installed, "a matching file is taken without downloading: {:?}", b.error);
        assert_eq!(b.files[0].sha256, sha256_hex(&good));
        // Downloading replaces the file that did not match.
        d.enqueue("a").unwrap();
        let a = wait(&d, "a", &[PackStatus::Installed, PackStatus::Failed]).await;
        assert_eq!(a.status, PackStatus::Installed, "{:?}", a.error);
        assert_eq!(std::fs::read(library.join("zim/a.zim")).unwrap(), good);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_update_keeps_the_old_file_until_the_new_one_is_in_place() {
        no_battery_rule();
        let (old, new) = (payload(5, 200_000), payload(6, 250_000));
        let files = temp("files");
        std::fs::write(files.join("x_2026-02.zim"), &new).unwrap();
        std::fs::write(files.join("m.gguf"), &new).unwrap();
        let server = serve(&files).await;
        let old_zim = file("zim/x_2026-01.zim", &old, "http://127.0.0.1:9/");
        let old_model = file("models/m.gguf", &old, "http://127.0.0.1:9/");
        let state = serde_json::json!({
            "x": { "status": "installed", "bytes_done": 1, "bytes_total": 1, "installed_version": "2026-01", "files": [InstalledFile::from(&old_zim)] },
            "m": { "status": "installed", "bytes_done": 1, "bytes_total": 1, "installed_version": "1", "files": [InstalledFile::from(&old_model)] },
        });
        let (library, state_path) = hub_dir(Some(state));
        put(&library, "zim/x_2026-01.zim", &old);
        put(&library, "models/m.gguf", &old);
        let d = Downloads::new(
            catalog(vec![
                pack("x", "2026-02", file("zim/x_2026-02.zim", &new, &format!("{server}/x_2026-02.zim"))),
                // Republished under the same name.
                pack("m", "1", file("models/m.gguf", &new, &format!("{server}/m.gguf"))),
            ]),
            library.clone(),
            state_path,
        );
        for id in ["x", "m"] {
            let st = d.state_of(id).unwrap();
            assert_eq!(st.status, PackStatus::Installed, "{id}");
            assert!(st.update_available, "{id}");
            assert!(d.needs_download(id));
            assert_eq!(d.installed_files(id)[0].sha256, sha256_hex(&old), "{id}: what is on disk is what counts");
        }
        d.start();
        d.enqueue("x").unwrap();
        d.enqueue("m").unwrap();
        for id in ["x", "m"] {
            let st = wait(&d, id, &[PackStatus::Installed, PackStatus::Failed]).await;
            assert_eq!(st.status, PackStatus::Installed, "{id}: {:?}", st.error);
            assert!(!st.update_available && st.stale.is_empty(), "{id}: {st:?}");
            assert_eq!(st.files.len(), 1);
            assert_eq!(st.files[0].sha256, sha256_hex(&new));
            assert!(!d.needs_download(id));
        }
        assert!(!library.join("zim/x_2026-01.zim").exists(), "the old version is deleted");
        assert_eq!(std::fs::read(library.join("zim/x_2026-02.zim")).unwrap(), new);
        assert_eq!(std::fs::read(library.join("models/m.gguf")).unwrap(), new);
        assert!(d.enqueue("x").is_err(), "already installed");
    }

    #[test]
    fn remove_deletes_what_is_on_disk() {
        let (old, new) = (payload(7, 1000), payload(8, 1000));
        let old_file = file("zim/x_2026-01.zim", &old, "http://127.0.0.1:9/");
        let state = serde_json::json!({
            "x": { "status": "installed", "bytes_done": 1, "bytes_total": 1, "installed_version": "2026-01", "files": [InstalledFile::from(&old_file)] },
        });
        let (library, state_path) = hub_dir(Some(state));
        put(&library, "zim/x_2026-01.zim", &old);
        put(&library, "zim/x_2026-02.zim.part", &new[..10]);
        let d = Downloads::new(catalog(vec![pack("x", "2026-02", file("zim/x_2026-02.zim", &new, "http://127.0.0.1:9/"))]), library.clone(), state_path.clone());
        d.remove("x").unwrap();
        assert!(!library.join("zim/x_2026-01.zim").exists(), "the older version on disk is deleted too");
        assert!(!library.join("zim/x_2026-02.zim.part").exists());
        assert!(d.state_of("x").unwrap().is_blank());
        // Blank states are not written out.
        let saved: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&state_path).unwrap()).unwrap();
        assert!(saved.get("x").is_none(), "{saved}");
        let _ = std::fs::remove_dir_all(library.parent().unwrap());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn resume_after_a_power_cut_starts_from_the_synced_length() {
        no_battery_rule();
        let good = payload(9, 3_000_000);
        let files = temp("files");
        std::fs::write(files.join("t.zim"), &good).unwrap();
        let server = serve(&files).await;
        // 2 MB reached the disk; the rest of the file reads back as zeros.
        let mut part = good[..2_000_000].to_vec();
        part.extend(vec![0u8; 600_000]);
        let state = serde_json::json!({
            "t": { "status": "downloading", "bytes_done": 2_600_000, "bytes_total": 3_000_000, "synced": { "zim/t.zim": 2_000_000 } },
        });
        let (library, state_path) = hub_dir(Some(state));
        put(&library, "zim/t.zim.part", &part);
        let d = Downloads::new(catalog(vec![pack("t", "1", file("zim/t.zim", &good, &format!("{server}/t.zim")))]), library.clone(), state_path);
        assert_eq!(d.state_of("t").unwrap().status, PackStatus::Paused);
        d.start();
        d.enqueue("t").unwrap();
        let st = wait(&d, "t", &[PackStatus::Installed, PackStatus::Failed]).await;
        assert_eq!(st.status, PackStatus::Installed, "{:?}", st.error);
        assert_eq!(std::fs::read(library.join("zim/t.zim")).unwrap(), good);
        assert!(st.synced.is_empty(), "nothing partial left: {st:?}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn import_runs_in_the_background_and_cleans_up() {
        no_battery_rule();
        let good = payload(10, 500_000);
        let usb = temp("usb");
        std::fs::create_dir_all(usb.join(crate::export::FOLDER)).unwrap();
        std::fs::write(usb.join(crate::export::FOLDER).join("i.zim"), &good).unwrap();
        // Same name, other size (another map version, say): not taken.
        std::fs::write(usb.join("j.zim"), &good[..10]).unwrap();
        let (library, state_path) = hub_dir(None);
        // Left by a copy that was cut off.
        put(&library, "zim/i.zim.import", b"half");
        let unreachable = "http://127.0.0.1:9/nothing";
        let d = Downloads::new(
            catalog(vec![pack("i", "1", file("zim/i.zim", &good, unreachable)), pack("j", "1", file("zim/j.zim", &good, unreachable))]),
            library.clone(),
            state_path,
        );
        assert!(!library.join("zim/i.zim.import").exists(), "cleaned up at startup");
        d.start();
        let queued = d.import_from_dir(&usb).unwrap();
        assert_eq!(queued, vec!["i".to_string()]);
        let st = wait(&d, "i", &[PackStatus::Installed, PackStatus::Failed]).await;
        assert_eq!(st.status, PackStatus::Installed, "{:?}", st.error);
        assert!(st.import_from.is_none());
        assert_eq!(std::fs::read(library.join("zim/i.zim")).unwrap(), good);
        assert!(!library.join("zim/i.zim.import").exists());
        assert!(d.import_from_dir(&usb).unwrap().is_empty(), "already installed");
        // The stick is gone when a paused import resumes: a clear error, and Retry downloads.
        d.remove("i").unwrap();
        d.set("i", |s| {
            s.status = PackStatus::Paused;
            s.import_from = Some(usb.join("gone").to_string_lossy().into_owned());
        });
        d.enqueue("i").unwrap();
        let st = wait(&d, "i", &[PackStatus::Installed, PackStatus::Failed]).await;
        assert!(st.error.unwrap().contains("import again"));
        assert!(st.import_from.is_none());
    }
}
