//! Add-on downloads: one pack at a time, resumable with HTTP ranges, verified
//! with SHA-256 (or the BLAKE3 Protomaps and the SHA-1 CoMaps publish),
//! optionally unpacked, and importable from or exportable to a folder (USB
//! stick). State survives restarts in `<root>/catalog/state.json`.
//!
//! What a pack has on disk is recorded when its files are verified
//! (`PackState::files`), and everything that uses packs goes by that record.
//! A newer catalog therefore never turns an old file into "the new version":
//! the pack shows an update, the old files keep working until the new ones
//! are verified, and only then are the old ones deleted.
//!
//! The world map is one pack whose build changes: the hub offers the build
//! it chooses from Protomaps' list (see `set_world` and world.rs), knows the
//! other builds' files by name and size, keeps the build it has until a
//! newer one is verified next to it, and lets an unfinished download of a
//! build go on while that build can still be downloaded.

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
use zaklon_core::catalog::{is_safe_relative, Catalog, Category, InstalledFile, Localized, Offer, Pack, PackFile, PackState, PackStatus};
use zaklon_core::world_map::{self, WorldOffer, WORLD_MAP_ID};

/// Downloads stop below this battery level unless the charger is connected (SPEC §6).
pub const MIN_BATTERY_PERCENT: u8 = 50;
/// Keep at least this much free after a download.
const DISK_MARGIN: u64 = 512 * 1024 * 1024;
/// Why a download failed when every address answered that the file is not
/// there (404 or 410): Protomaps deleted a build, say. Trying again later
/// does not help; a newer offer does.
pub const GONE: &str = "the file is no longer at its download address";
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
    /// On this hub, but no longer offered by the catalog: it stays usable
    /// and can be deleted, but is not downloaded or updated any more.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub withdrawn: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemInfo {
    pub disk_free: u64,
    pub disk_total: u64,
    pub battery_percent: Option<u8>,
    pub plugged_in: bool,
}

/// The world map as Add-ons explains it: which build is offered, which one
/// the hub has, and whether a newer one fits next to it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorldView {
    /// The build offered for download (or the one downloading), "20260811".
    pub offered: String,
    pub offered_size: u64,
    /// The build on the hub (verified, or found in place and being checked).
    pub installed: Option<String>,
    pub installed_size: u64,
    /// A newer build than the one on the hub is offered.
    pub update: bool,
    /// Free space the offered build still needs, with what is kept free.
    pub needed: u64,
    pub disk_free: u64,
    /// It fits next to the build on the hub.
    pub room_for_both: bool,
    /// Offered from Protomaps' list (else the build pinned in the app).
    pub listed: bool,
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
    /// What can be downloaded. Only the world map's entry changes (its build,
    /// see `set_world`); readers take the whole catalog as it is at that moment.
    catalog: Mutex<Arc<Catalog>>,
    /// The world map's offer and the builds the hub knows (None: this catalog
    /// has no world map to manage, as in most tests).
    world: Mutex<Option<WorldOffer>>,
    /// Tests: the size of a pretend disk the library is on (see `disk_free`).
    #[cfg(test)]
    fake_disk: Mutex<Option<u64>>,
    /// Packs this hub has that the catalog no longer offers (see `retired`).
    retired: Vec<Pack>,
    library: PathBuf,
    state_path: PathBuf,
    states: Mutex<HashMap<String, PackState>>,
    queue: Mutex<VecDeque<String>>,
    pause_requests: Mutex<HashSet<String>>,
    /// Queued packs that are only to be verified.
    verify_only: Mutex<HashSet<String>>,
    /// Packs checked again this session because the library engine could not open them.
    rechecked: Mutex<HashSet<String>>,
    /// Map packs whose files were found in place (put there by hand, say)
    /// and are being checked. The map shows them meanwhile; see `map_archives`.
    found_in_place: Mutex<HashSet<String>>,
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
        Self::with_world(catalog, library, state_path, None)
    }

    /// `new`, with the world map's offer and the builds the hub knows (see
    /// `set_world`): the catalog's world map is the offered build from the start.
    pub fn with_world(mut catalog: Catalog, library: PathBuf, state_path: PathBuf, world: Option<WorldOffer>) -> Arc<Self> {
        if let Some(w) = &world {
            if let Some(slot) = catalog.packs.iter_mut().find(|p| p.id == WORLD_MAP_ID) {
                *slot = w.offer.clone();
            }
        }
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
        let mut found_in_place = HashSet::new();
        for p in &catalog.packs {
            let st = states.entry(p.id.clone()).or_insert_with(|| PackState::not_installed(p.size));
            let builds = candidates(p, world.as_ref());
            if reconcile(&library, p, st, &builds) {
                queue.push_back(p.id.clone());
                verify_only.insert(p.id.clone());
                if is_map_archive_pack(p) && placed_build(&library, &builds).is_some() {
                    found_in_place.insert(p.id.clone());
                }
            }
            // A USB copy that was cut off leaves its temporary file behind.
            for f in &p.files {
                let _ = std::fs::remove_file(import_path(&library.join(&f.path)));
            }
        }
        let retired = retired(&catalog, &library, &mut states);
        for p in &retired {
            if states.get(&p.id).is_some_and(|s| s.status == PackStatus::Queued) {
                // Its files were found without a record: checked, as for any pack.
                queue.push_back(p.id.clone());
                verify_only.insert(p.id.clone());
            }
        }
        let client = reqwest::Client::builder()
            .user_agent(user_agent())
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
            catalog: Mutex::new(Arc::new(catalog)),
            world: Mutex::new(world),
            #[cfg(test)]
            fake_disk: Mutex::new(None),
            retired,
            library,
            state_path,
            states: Mutex::new(states),
            queue: Mutex::new(queue),
            pause_requests: Mutex::new(HashSet::new()),
            verify_only: Mutex::new(verify_only),
            rechecked: Mutex::new(HashSet::new()),
            found_in_place: Mutex::new(found_in_place),
            notify: Notify::new(),
            client,
            release: OnceLock::new(),
            save_requested: AtomicU64::new(0),
            save_written: Mutex::new(0),
        });
        // The world map's build, and which of its unfinished downloads cannot go on.
        me.apply_world();
        // Nothing has the library's files open yet: a good moment to delete old ones.
        let with_stale: Vec<String> = me.lock_states().iter().filter(|(_, s)| !s.stale.is_empty()).map(|(id, _)| id.clone()).collect();
        for id in with_stale {
            me.sweep(&id);
        }
        if has_work {
            // Kept as a permit until the worker starts.
            me.notify.notify_one();
        }
        me
    }

    /// The catalog as it is now (the world map's entry can change later).
    pub fn catalog(&self) -> Arc<Catalog> {
        self.catalog.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// A pack by id: one the catalog offers, or one this hub has that the
    /// catalog no longer offers.
    pub fn pack(&self, id: &str) -> Option<Pack> {
        self.catalog().pack(id).cloned().or_else(|| self.retired.iter().find(|p| p.id == id).cloned())
    }

    fn lock_states(&self) -> std::sync::MutexGuard<'_, HashMap<String, PackState>> {
        self.states.lock().unwrap_or_else(|p| p.into_inner())
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

    /// Every pack the catalog offers, then the ones this hub has that it no
    /// longer offers (until they are deleted).
    pub fn snapshot(&self) -> Vec<PackView> {
        let catalog = self.catalog();
        let states = self.states.lock().unwrap_or_else(|p| p.into_inner());
        let offered = catalog.packs.iter().map(|p| (p, false));
        offered
            .chain(self.retired.iter().map(|p| (p, true)))
            .filter_map(|(p, withdrawn)| {
                let state = states.get(&p.id).cloned().unwrap_or_else(|| PackState::not_installed(p.size));
                (!withdrawn || state.status != PackStatus::NotInstalled).then(|| PackView { pack: p.clone(), state, withdrawn })
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
        let Some(pack) = self.catalog().pack(id).cloned() else {
            let why = if self.pack(id).is_some() { "this pack is no longer offered" } else { "unknown pack" };
            return Err(why.into());
        };
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
        self.found_in_place.lock().unwrap_or_else(|p| p.into_inner()).remove(id);
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
    /// Also for a pack the catalog no longer offers: only a person deletes it.
    pub fn remove(&self, id: &str) -> Result<(), String> {
        let pack = self.pack(id).ok_or("unknown pack")?;
        if let Some(st) = self.state_of(id) {
            if matches!(st.status, PackStatus::Downloading | PackStatus::Verifying) {
                return Err("pause the download first".into());
            }
        }
        self.queue.lock().unwrap_or_else(|p| p.into_inner()).retain(|q| q != id);
        self.verify_only.lock().unwrap_or_else(|p| p.into_inner()).remove(id);
        self.found_in_place.lock().unwrap_or_else(|p| p.into_inner()).remove(id);
        // Forget the pack first: whatever happens below, it no longer counts as installed.
        let old = self
            .states
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id.to_string(), PackState::not_installed(pack.size))
            .unwrap_or_else(|| PackState::not_installed(pack.size));
        // Program folders go before their archives, so a folder that could not
        // be deleted still has what it takes to unpack it again. For the
        // world map, the files of every build it knows (one put in place by
        // hand and still being checked, say) go too.
        let own: Vec<PackFile> = self.candidates_of(&pack).into_iter().flat_map(|b| b.files).collect();
        let mut targets: Vec<String> = Vec::new();
        let dirs = old.files.iter().filter_map(|f| f.unpack_to.clone()).chain(own.iter().filter_map(|f| f.unpack_to.clone()));
        let files = old.files.iter().map(|f| f.path.clone()).chain(own.iter().map(|f| f.path.clone()));
        let temps = own
            .iter()
            .flat_map(|f| [format!("{}.part", f.path), format!("{}.import", f.path)])
            // Partial downloads it has (for a pack no longer in the catalog, the only record of them).
            .chain(old.synced.keys().map(|p| format!("{p}.part")));
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
        // The world map is offered as its current build again (not one whose download was under way).
        if id == WORLD_MAP_ID {
            self.apply_world();
        }
        self.save();
        first_error.map_or(Ok(()), Err)
    }

    /// Queue the packs whose files are in `dir` (a USB stick) or in
    /// `dir/zaklon-packs` to be copied into the library. The copying runs in
    /// the background with progress, and can be paused. Returns the pack ids.
    pub fn import_from_dir(self: &Arc<Self>, dir: &Path) -> Result<Vec<String>, String> {
        let mut queued = Vec::new();
        for pack in &self.catalog().packs {
            if !pack.files.iter().all(|f| find_source(dir, f).is_some()) {
                continue;
            }
            // Claim the pack in one step: a download started in the meantime
            // must not write the same files.
            let claimed = {
                let mut states = self.states.lock().unwrap_or_else(|p| p.into_inner());
                let st = states.entry(pack.id.clone()).or_insert_with(|| PackState::not_installed(pack.size));
                let busy = matches!(st.status, PackStatus::Queued | PackStatus::Downloading | PackStatus::Verifying);
                let current = st.status == PackStatus::Installed && !has_update(pack, st);
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

    /// Map archives (the `.pmtiles` files of map packs, such as the world
    /// map) the map can use now: the verified files of each pack (also while
    /// a newer version downloads), and the files of a pack that were found in
    /// place with the right size and are still being checked, so a map put
    /// there by hand shows at once (for the world map, of any build the hub
    /// knows). Blocking only for a few file lookups.
    pub fn map_archives(&self) -> Vec<(String, PathBuf)> {
        let catalog = self.catalog();
        let states = self.states.lock().unwrap_or_else(|p| p.into_inner());
        let found = self.found_in_place.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let mut out = Vec::new();
        for p in catalog.packs.iter().filter(|p| is_map_archive_pack(p)) {
            let Some(st) = states.get(&p.id) else { continue };
            let files: Vec<String> = if !st.files.is_empty() {
                st.files.iter().map(|f| f.path.clone()).collect()
            } else if found.contains(&p.id) && matches!(st.status, PackStatus::Queued | PackStatus::Verifying | PackStatus::Paused) {
                match self.placed(p) {
                    Some(b) => b.files.into_iter().map(|f| f.path).collect(),
                    None => continue,
                }
            } else {
                continue;
            };
            out.extend(files.into_iter().filter(|f| f.ends_with(".pmtiles")).map(|f| (p.id.clone(), self.library.join(f))));
        }
        out
    }

    /// Map packs whose files appeared in place while the hub runs (copied or
    /// linked there by hand) with the right size: they are checked in the
    /// background and shown meanwhile, as if found at startup.
    pub fn notice_placed_maps(self: &Arc<Self>) {
        let mut noticed = Vec::new();
        {
            let catalog = self.catalog();
            let mut states = self.states.lock().unwrap_or_else(|p| p.into_inner());
            for p in catalog.packs.iter().filter(|p| is_map_archive_pack(p)) {
                let st = states.entry(p.id.clone()).or_insert_with(|| PackState::not_installed(p.size));
                if st.status == PackStatus::NotInstalled && st.files.is_empty() && self.placed(p).is_some() {
                    st.status = PackStatus::Queued;
                    st.error = None;
                    noticed.push(p.id.clone());
                }
            }
        }
        if noticed.is_empty() {
            return;
        }
        for id in noticed {
            info!(pack = %id, "map found in place; checking it");
            self.verify_only.lock().unwrap_or_else(|p| p.into_inner()).insert(id.clone());
            self.found_in_place.lock().unwrap_or_else(|p| p.into_inner()).insert(id.clone());
            self.queue.lock().unwrap_or_else(|p| p.into_inner()).push_back(id);
        }
        self.save_soon();
        self.notify.notify_one();
    }

    // ---- the world map --------------------------------------------------------

    /// A new offer for the world map (Protomaps' list was read again, or a
    /// build grew old enough): the pack becomes the offered build, unless an
    /// unfinished download of another build can go on; the pack shows an
    /// update when the offered build is newer than the one on disk. Applied
    /// now, or when a running download or check of the world map ends.
    pub fn set_world(self: &Arc<Self>, offer: WorldOffer) {
        {
            let mut w = self.world.lock().unwrap_or_else(|p| p.into_inner());
            if w.as_ref().is_some_and(|old| old.same(&offer)) {
                return;
            }
            *w = Some(offer);
        }
        if self.apply_world() {
            self.sweep_soon(WORLD_MAP_ID);
        }
        self.save_soon();
    }

    /// The world map as Add-ons explains it (None when the catalog has none).
    pub fn world_view(&self) -> Option<WorldView> {
        let pack = self.catalog().pack(WORLD_MAP_ID).cloned()?;
        let st = self.state_of(WORLD_MAP_ID).unwrap_or_else(|| PackState::not_installed(pack.size));
        let found = self.found_in_place.lock().unwrap_or_else(|p| p.into_inner()).contains(WORLD_MAP_ID);
        let (installed, installed_size) = if !st.files.is_empty() {
            (installed_build(&st), st.files.iter().map(|f| f.size).sum())
        } else if found {
            self.placed(&pack).map_or((None, 0), |b| (Some(b.version.clone()), b.size))
        } else {
            (None, 0)
        };
        let (needed, disk_free) = (self.space_needed(&pack), self.disk_free());
        let listed = self.world.lock().unwrap_or_else(|p| p.into_inner()).as_ref().is_some_and(|w| w.listed);
        Some(WorldView {
            offered: pack.version.clone(),
            offered_size: pack.size,
            installed,
            installed_size,
            update: st.update_available,
            needed,
            disk_free,
            room_for_both: disk_free >= needed,
            listed,
        })
    }

    /// Update the world map to the offered build (the laptop asks). The new
    /// build downloads next to the old one, which the map keeps using until
    /// the new one is verified; then the old one is deleted. Without room
    /// for both it is refused, unless `remove_old`: then the old one is
    /// deleted first (the map shows only the overview meanwhile), if that
    /// makes room.
    pub async fn update_world(self: &Arc<Self>, remove_old: bool) -> Result<(), String> {
        let pack = self.catalog().pack(WORLD_MAP_ID).cloned().ok_or("unknown pack")?;
        let st = self.state_of(WORLD_MAP_ID).unwrap_or_else(|| PackState::not_installed(pack.size));
        if matches!(st.status, PackStatus::Queued | PackStatus::Downloading | PackStatus::Verifying) {
            // Under way already.
            return Ok(());
        }
        if st.files.is_empty() || !st.update_available {
            return Err("no newer world map is offered".into());
        }
        let (free, needed) = (self.disk_free(), self.space_needed(&pack));
        if free < needed {
            if !remove_old {
                return Err("not enough free disk space for both world maps; remove the old one first".into());
            }
            let old: u64 = st.files.iter().map(|f| f.size).sum();
            if free.saturating_add(old) < needed {
                return Err("not enough free disk space".into());
            }
            info!(old = ?installed_build(&st), new = %pack.version, "removing the old world map first, to make room for the new one");
            self.release(&pack).await;
            let me = self.clone();
            tokio::task::spawn_blocking(move || me.drop_installed(WORLD_MAP_ID)).await.map_err(|e| format!("delete task: {e}"))??;
        }
        self.enqueue(WORLD_MAP_ID)
    }

    /// Bring the world map in line with its offer (see `set_world`). Not
    /// while it downloads or is checked: `finish` does it afterwards.
    /// Returns true when unfinished downloads became stale.
    fn apply_world(&self) -> bool {
        let Some(w) = self.world.lock().unwrap_or_else(|p| p.into_inner()).clone() else { return false };
        let Some(current) = self.catalog().pack(WORLD_MAP_ID).cloned() else { return false };
        let mut dropped = false;
        let next = {
            let mut states = self.lock_states();
            let st = states.entry(WORLD_MAP_ID.to_string()).or_insert_with(|| PackState::not_installed(current.size));
            if matches!(st.status, PackStatus::Queued | PackStatus::Downloading | PackStatus::Verifying) {
                return false;
            }
            let builds: Vec<&Pack> = std::iter::once(&current).chain(w.known.iter()).collect();
            let unfinished = |b: &Pack| b.files.iter().any(|f| st.synced.contains_key(&f.path) || part_path(&self.library.join(&f.path)).is_file());
            // A download of a build that can still be downloaded goes on with that build.
            let next = builds.iter().copied().find(|b| unfinished(b) && w.downloadable.contains(&b.version)).map_or_else(|| w.offer.clone(), Pack::clone);
            // Unfinished downloads of other builds cannot go on: their pieces go.
            for b in &builds {
                for f in b.files.iter().filter(|f| !next.files.iter().any(|n| n.path == f.path)) {
                    let part = format!("{}.part", f.path);
                    let had = st.synced.remove(&f.path).is_some() | self.library.join(&part).is_file();
                    if had {
                        info!(part = %part, "an unfinished download of a world map build that is no longer offered is deleted");
                        if !st.stale.contains(&part) {
                            st.stale.push(part);
                        }
                        dropped = true;
                    }
                }
            }
            if dropped {
                // What it had downloaded is gone; a failure keeps its reason.
                if st.status == PackStatus::Paused {
                    st.status = if st.files.is_empty() { PackStatus::NotInstalled } else { PackStatus::Installed };
                }
                st.bytes_done = if st.status == PackStatus::Installed { st.files.iter().map(|f| f.size).sum() } else { 0 };
            }
            if st.status == PackStatus::NotInstalled {
                st.bytes_total = next.size;
            }
            st.update_available = has_update(&next, st);
            next
        };
        let file_of = |p: &Pack| p.files.iter().map(|f| (f.path.clone(), f.size, f.sha256.clone(), f.blake3.clone())).collect::<Vec<_>>();
        if next.version != current.version || file_of(&next) != file_of(&current) {
            info!(build = %next.version, "world map offered");
            let mut c = self.catalog.lock().unwrap_or_else(|p| p.into_inner());
            let mut updated = (**c).clone();
            if let Some(slot) = updated.packs.iter_mut().find(|p| p.id == WORLD_MAP_ID) {
                *slot = next;
            }
            *c = Arc::new(updated);
        }
        dropped
    }

    /// Delete the files a pack has installed (not an unfinished download)
    /// and forget them. Blocking; stop the programs using them first.
    fn drop_installed(&self, id: &str) -> Result<(), String> {
        for f in self.installed_files(id) {
            for rel in f.unpack_to.iter().chain(std::iter::once(&f.path)) {
                delete_in_library(&self.library, rel).map_err(|e| format!("could not delete {rel}: {e}"))?;
            }
            info!(pack = id, path = %f.path, "deleted to make room");
            self.set(id, |s| s.files.retain(|x| x.path != f.path));
        }
        self.set(id, |s| {
            s.installed_version = None;
            s.update_available = false;
            if s.status == PackStatus::Installed {
                s.status = PackStatus::NotInstalled;
                s.bytes_done = 0;
            }
        });
        self.save();
        Ok(())
    }

    /// The builds `p`'s files on disk may be (see `candidates`).
    fn candidates_of(&self, p: &Pack) -> Vec<Pack> {
        candidates(p, self.world.lock().unwrap_or_else(|p| p.into_inner()).as_ref())
    }

    /// The build of `p` whose files are all in place with the right size (not checked yet).
    fn placed(&self, p: &Pack) -> Option<Pack> {
        placed_build(&self.library, &self.candidates_of(p)).cloned()
    }

    /// Free space on the library's drive.
    fn disk_free(&self) -> u64 {
        #[cfg(test)]
        if let Some(size) = *self.fake_disk.lock().unwrap_or_else(|p| p.into_inner()) {
            return size.saturating_sub(dir_size(&self.library));
        }
        system_info(&self.library).disk_free
    }

    /// The free space a download of `pack` still needs, with what is kept free.
    fn space_needed(&self, pack: &Pack) -> u64 {
        let recorded = self.installed_files(&pack.id);
        let missing: u64 = pack
            .files
            .iter()
            .map(|f| {
                let dest = self.library.join(&f.path);
                if recorded.iter().any(|x| x.is(f)) && dest.is_file() {
                    0
                } else {
                    f.size.saturating_sub(std::fs::metadata(part_path(&dest)).map(|m| m.len().min(f.size)).unwrap_or(0))
                }
            })
            .sum();
        missing + DISK_MARGIN
    }

    /// Delete a pack's stale files in the background.
    fn sweep_soon(self: &Arc<Self>, id: &str) {
        let (me, id) = (self.clone(), id.to_string());
        match tokio::runtime::Handle::try_current() {
            Ok(h) => {
                h.spawn_blocking(move || me.sweep(&id));
            }
            Err(_) => me.sweep(&id),
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
        let Some(pack) = self.pack(id) else { return };
        let verify = self.verify_only.lock().unwrap_or_else(|p| p.into_inner()).remove(id);
        if !verify && self.catalog().pack(id).is_none() {
            // No longer offered: its files on disk may be checked, nothing more.
            self.set(id, |s| {
                if s.status == PackStatus::Queued {
                    s.status = PackStatus::Paused;
                }
            });
            self.save_soon();
            return;
        }
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
        if self.disk_free() < pack.size.saturating_sub(already) + DISK_MARGIN {
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
        // A paused check goes on later; a finished one decided.
        if !matches!(outcome, Outcome::Paused) {
            self.found_in_place.lock().unwrap_or_else(|p| p.into_inner()).remove(id);
        }
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
        // An offer that came while the world map downloaded or was checked.
        if id == WORLD_MAP_ID && self.apply_world() {
            self.sweep_soon(id);
        }
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
                    match self.hash_checked(id, &dest, Checksum::of_file(f), false).await {
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
    /// ones found without a record (the catalog's, or for the world map
    /// those of any build the hub knows). Nothing is downloaded.
    async fn verify_pack(self: &Arc<Self>, id: &str, pack: &Pack) -> Outcome {
        let recorded = self.installed_files(id);
        let adopting = recorded.is_empty();
        let adopted = if adopting { self.placed(pack).unwrap_or_else(|| pack.clone()) } else { pack.clone() };
        let files: Vec<InstalledFile> = if adopting { adopted.files.iter().map(InstalledFile::from).collect() } else { recorded };
        let total: u64 = files.iter().map(|f| f.size).sum();
        self.set(id, |s| {
            s.bytes_done = 0;
            s.bytes_total = total;
        });
        for (i, f) in files.iter().enumerate() {
            let path = self.library.join(&f.path);
            // Progress by bytes read: a check of the world map takes minutes.
            let checked_before: u64 = files[..i].iter().map(|x| x.size).sum();
            self.set(id, |s| s.bytes_done = checked_before);
            let good = std::fs::metadata(&path).is_ok_and(|m| m.len() == f.size)
                && match self.hash_checked(id, &path, Checksum::of(&f.sha256, f.blake3.as_deref()), true).await {
                    Ok(Some(d)) => d.matches_hash(&f.sha256, f.blake3.as_deref(), f.sha1_base64.as_deref()),
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
            self.set(id, |s| s.bytes_done = checked_before + f.size);
        }
        for f in &files {
            if let Err(e) = self.ensure_unpacked(pack, f.unpack_to.as_deref(), &self.library.join(&f.path)).await {
                return Outcome::Failed(e);
            }
        }
        self.set(id, |s| {
            if adopting {
                s.installed_version = Some(adopted.version.clone());
            }
            s.stale.retain(|p| !files.iter().any(|f| f.path == *p || f.unpack_to.as_ref() == Some(p)));
            s.files = files;
            s.update_available = has_update(pack, s);
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

    /// The checksum of a file (see `Checksum`), off the async threads.
    /// `Ok(None)` when a pause was asked for. With `progress`, what has been
    /// read counts towards the pack's progress (a check of a very large file
    /// takes a while).
    async fn hash_checked(self: &Arc<Self>, id: &str, path: &Path, kind: Checksum, progress: bool) -> Result<Option<FileDigest>, String> {
        let (me, id2, p) = (self.clone(), id.to_string(), path.to_path_buf());
        tokio::task::spawn_blocking(move || {
            hash_file(&p, kind, || me.pause_requested(&id2), |n| {
                if progress {
                    me.set(&id2, |s| s.bytes_done += n);
                }
            })
        })
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
            let mut gone = 0;
            for url in &f.urls {
                match self.fetch_range(id, f, url, &part, &mut have).await {
                    Ok(true) => {
                        done = true;
                        break;
                    }
                    Ok(false) => return Err(Outcome::Paused),
                    Err(e) => {
                        warn!(pack = id, url, "download error: {e}");
                        gone += usize::from(e == GONE);
                        last_err = e;
                    }
                }
            }
            if !done {
                // Every address says the file is not there: waiting will not bring it back.
                let why = if gone > 0 && gone == f.urls.len() { GONE.to_string() } else { format!("download failed: {last_err}") };
                return Err(Outcome::Failed(why));
            }
        }

        self.set(id, |s| {
            s.status = PackStatus::Verifying;
            s.speed = 0;
        });
        self.save_soon();
        match self.hash_checked(id, &part, Checksum::of_file(f), false).await {
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
        if matches!(status, reqwest::StatusCode::NOT_FOUND | reqwest::StatusCode::GONE) {
            return Err(GONE.into());
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
        let (me, id2, t2, kind) = (self.clone(), id.to_string(), tmp.clone(), Checksum::of_file(f));
        let copied = tokio::task::spawn_blocking(move || me.copy_with_hash(&id2, &src, &t2, kind))
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
    fn copy_with_hash(&self, id: &str, src: &Path, dest: &Path, kind: Checksum) -> std::io::Result<Option<FileDigest>> {
        let mut input = std::fs::File::open(src)?;
        let mut output = std::fs::File::create(dest)?;
        let mut h = Hashers::new(kind);
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
/// `builds` are what the pack's files on disk may be (see `candidates`).
/// Returns true when files of the pack were found on disk without a record:
/// they are verified before they count as installed.
fn reconcile(library: &Path, p: &Pack, st: &mut PackState, builds: &[Pack]) -> bool {
    // state.json is ours, but only ever act on paths inside the library.
    st.files.retain(|f| is_safe_relative(&f.path) && f.unpack_to.as_deref().is_none_or(is_safe_relative));
    st.stale.retain(|s| is_safe_relative(s));
    // A state file from before files were recorded: an installed pack of the
    // catalog's version had exactly the catalog's files.
    if st.files.is_empty() && st.status == PackStatus::Installed {
        let version = st.installed_version.clone();
        let before = std::iter::once(p).chain(builds).find(|b| version.as_deref() == Some(b.version.as_str()) && pack_complete_on_disk(library, b));
        if let Some(b) = before {
            st.files = b.files.iter().map(InstalledFile::from).collect();
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
    st.update_available = has_update(p, st);
    if st.status == PackStatus::Installed {
        st.bytes_total = p.size;
        st.bytes_done = p.size;
    }
    // (Not after a failure: a file that failed its check is not checked again at every start.)
    let unknown_files = st.files.is_empty()
        && matches!(st.status, PackStatus::NotInstalled | PackStatus::Paused)
        && (p.files.iter().all(|f| library.join(&f.path).is_file()) || placed_build(library, builds).is_some());
    if unknown_files {
        st.status = PackStatus::Queued;
        st.error = None;
    }
    unknown_files
}

/// The packs this hub has that the catalog no longer offers, their states
/// brought in line with the disk. Nothing of them is deleted: they stay
/// installed and readable until a person deletes them. One the catalog lists
/// as withdrawn is known as it was listed; any other (a newer catalog left it
/// out) by what was recorded when its files were verified.
fn retired(catalog: &Catalog, library: &Path, states: &mut HashMap<String, PackState>) -> Vec<Pack> {
    let known: HashSet<String> = catalog.packs.iter().chain(&catalog.withdrawn).map(|p| p.id.clone()).collect();
    let mut unknown: Vec<Pack> = states.iter().filter(|(id, st)| !known.contains(*id) && !st.files.is_empty()).map(|(id, st)| from_record(id, st)).collect();
    unknown.sort_by(|a, b| a.id.cmp(&b.id));
    let listed = catalog.withdrawn.iter().filter(|p| catalog.pack(&p.id).is_none()).cloned();
    let mut out = Vec::new();
    for p in listed.chain(unknown) {
        let st = states.entry(p.id.clone()).or_insert_with(|| PackState::not_installed(p.size));
        reconcile(library, &p, st, &[]);
        // Nothing newer will come, and an update that was under way cannot
        // continue: the verified files it has are what it is.
        st.update_available = false;
        if !st.files.is_empty() && matches!(st.status, PackStatus::Paused | PackStatus::Failed) {
            st.status = PackStatus::Installed;
            st.error = None;
            st.bytes_total = st.files.iter().map(|f| f.size).sum();
            st.bytes_done = st.bytes_total;
        }
        if st.status != PackStatus::NotInstalled {
            out.push(p);
        }
    }
    out
}

/// A pack known only by what was recorded when its files were verified,
/// named after its first file.
fn from_record(id: &str, st: &PackState) -> Pack {
    let files: Vec<PackFile> = st
        .files
        .iter()
        .map(|f| PackFile {
            path: f.path.clone(),
            urls: Vec::new(),
            sha256: f.sha256.clone(),
            sha1_base64: f.sha1_base64.clone(),
            blake3: f.blake3.clone(),
            size: f.size,
            unpack: f.unpack_to.as_ref().map(|_| "zip".to_string()),
            unpack_to: f.unpack_to.clone(),
        })
        .collect();
    let first = files.first().map(|f| f.path.clone()).unwrap_or_default();
    let name = Path::new(&first).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| id.to_string());
    // Only a ZIM goes to the library engine.
    let category = if id.starts_with(zaklon_core::maps::MAP_ID_PREFIX) {
        Category::Maps
    } else if first.ends_with(".zim") {
        Category::Knowledge
    } else if first.starts_with("models/") {
        Category::Model
    } else {
        Category::App
    };
    Pack {
        id: id.to_string(),
        title: Localized { en: name.clone(), sr: name },
        description: Localized::default(),
        category,
        topics: Vec::new(),
        version: st.installed_version.clone().unwrap_or_default(),
        size: files.iter().map(|f| f.size).sum(),
        files,
        license: String::new(),
        attribution: String::new(),
        source: String::new(),
        offer: Offer::Auto,
        offer_reason: String::new(),
        languages: Vec::new(),
        recommended_for: Vec::new(),
    }
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

/// A map pack the Zaklon map draws from: its files are map archives
/// (PMTiles), unlike the pieces of CoMaps maps.
pub fn is_map_archive_pack(p: &Pack) -> bool {
    p.category == zaklon_core::catalog::Category::Maps
        && !p.id.starts_with(zaklon_core::maps::MAP_ID_PREFIX)
        && p.files.iter().any(|f| f.path.ends_with(".pmtiles"))
}

/// Every file of a pack is at its place with exactly the catalog's size
/// (not yet checked).
fn complete_in_place(library: &Path, p: &Pack) -> bool {
    p.files.iter().all(|f| std::fs::metadata(library.join(&f.path)).is_ok_and(|m| m.is_file() && m.len() == f.size))
}

/// The builds a pack's files on disk may be: the pack itself, and for the
/// world map every build the hub knows (the pinned one first, which for its
/// date wins over the offer, then the listed ones).
fn candidates(p: &Pack, world: Option<&WorldOffer>) -> Vec<Pack> {
    match world {
        Some(w) if p.id == WORLD_MAP_ID => {
            let mut all = w.known.clone();
            if !all.iter().any(|k| k.version == p.version) {
                all.insert(0, p.clone());
            }
            all
        }
        _ => vec![p.clone()],
    }
}

/// The first of `builds` whose files are all in place with exactly their size.
fn placed_build<'a>(library: &Path, builds: &'a [Pack]) -> Option<&'a Pack> {
    builds.iter().find(|b| complete_in_place(library, b))
}

/// The build of the world map on disk ("20260928"), by its file's name.
fn installed_build(st: &PackState) -> Option<String> {
    if st.files.is_empty() {
        return None;
    }
    st.files.iter().find_map(|f| world_map::date_of_path(&f.path)).map(str::to_string).or_else(|| st.installed_version.clone())
}

/// The catalog offers something newer than what is on disk: other files
/// for the pack, or for the world map a later build (an earlier build that
/// is offered never replaces a later one on disk).
fn has_update(p: &Pack, st: &PackState) -> bool {
    if st.files.is_empty() {
        return false;
    }
    if p.id == WORLD_MAP_ID {
        if let Some(have) = installed_build(st) {
            return have.as_str() < p.version.as_str();
        }
    }
    !st.matches(p)
}

/// How Zaklon names itself to the servers it downloads from.
pub fn user_agent() -> String {
    format!("Zaklon/{} (+https://github.com/stefan-cirovic/zaklon)", env!("CARGO_PKG_VERSION"))
}

/// Bytes the files in a folder take, for the pretend disk of tests.
#[cfg(test)]
fn dir_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|e| if e.path().is_dir() { dir_size(&e.path()) } else { e.metadata().map(|m| m.len()).unwrap_or(0) }).sum())
        .unwrap_or(0)
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

/// The checksum a file is checked with: the one its catalog entry gives.
/// Our catalog uses SHA-256, Protomaps publishes BLAKE3 (hex) for its world
/// map builds and CoMaps SHA-1 (base64) for its map files; with more than
/// one, SHA-256 wins. Only that one is computed: a second one would add
/// minutes to a check of a file of a hundred gigabytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Checksum {
    Sha256,
    Blake3,
    Sha1,
}

impl Checksum {
    fn of(sha256: &str, blake3: Option<&str>) -> Self {
        if !sha256.is_empty() {
            Self::Sha256
        } else if blake3.is_some() {
            Self::Blake3
        } else {
            Self::Sha1
        }
    }

    fn of_file(f: &PackFile) -> Self {
        Self::of(&f.sha256, f.blake3.as_deref())
    }
}

/// A file's checksum, of the kind asked for (see `Checksum`).
struct FileDigest {
    sha256: Option<String>,
    blake3: Option<String>,
    sha1_base64: Option<String>,
}

impl FileDigest {
    fn matches(&self, f: &PackFile) -> bool {
        self.matches_hash(&f.sha256, f.blake3.as_deref(), f.sha1_base64.as_deref())
    }

    /// The file is the one these checksums describe, by the checksum that
    /// counts (see `Checksum`). A file without any checksum never matches.
    fn matches_hash(&self, sha256: &str, blake3: Option<&str>, sha1_base64: Option<&str>) -> bool {
        let same = |have: &Option<String>, want: &str| have.as_deref().is_some_and(|h| h.eq_ignore_ascii_case(want));
        if !sha256.is_empty() {
            return same(&self.sha256, sha256);
        }
        if let Some(want) = blake3 {
            return same(&self.blake3, want);
        }
        sha1_base64.is_some_and(|h| Some(h) == self.sha1_base64.as_deref())
    }
}

enum Hashers {
    Sha256(Sha256),
    Blake3(Box<blake3::Hasher>),
    Sha1(sha1::Sha1),
}

impl Hashers {
    fn new(kind: Checksum) -> Self {
        match kind {
            Checksum::Sha256 => Self::Sha256(Sha256::new()),
            Checksum::Blake3 => Self::Blake3(Box::new(blake3::Hasher::new())),
            Checksum::Sha1 => Self::Sha1(sha1::Sha1::new()),
        }
    }
    fn update(&mut self, b: &[u8]) {
        match self {
            Self::Sha256(h) => h.update(b),
            Self::Blake3(h) => {
                h.update(b);
            }
            Self::Sha1(h) => sha1::Digest::update(h, b),
        }
    }
    fn finish(self) -> FileDigest {
        use base64::Engine;
        let mut d = FileDigest { sha256: None, blake3: None, sha1_base64: None };
        match self {
            Self::Sha256(h) => d.sha256 = Some(h.finalize().iter().map(|b| format!("{b:02x}")).collect()),
            Self::Blake3(h) => d.blake3 = Some(h.finalize().to_hex().to_string()),
            Self::Sha1(h) => d.sha1_base64 = Some(base64::engine::general_purpose::STANDARD.encode(sha1::Digest::finalize(h))),
        }
        d
    }
}

/// Hash a file; `Ok(None)` when `stop` says to give up (a pause). `read`
/// hears how many bytes were read, now and then. Blocking.
fn hash_file(path: &Path, kind: Checksum, stop: impl Fn() -> bool, mut read: impl FnMut(u64)) -> std::io::Result<Option<FileDigest>> {
    const REPORT_EVERY: u64 = 64 << 20;
    let mut file = std::fs::File::open(path)?;
    let mut h = Hashers::new(kind);
    let mut buf = vec![0u8; 1 << 20];
    let mut unreported = 0u64;
    loop {
        if stop() {
            return Ok(None);
        }
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        unreported += n as u64;
        if unreported >= REPORT_EVERY {
            read(unreported);
            unreported = 0;
        }
    }
    if unreported > 0 {
        read(unreported);
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
            blake3: None,
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
            topics: Vec::new(),
            version: version.into(),
            size: f.size,
            files: vec![f],
            license: String::new(),
            attribution: String::new(),
            source: String::new(),
            offer: Offer::Auto,
            offer_reason: String::new(),
            languages: vec![],
            recommended_for: vec![],
        }
    }

    fn catalog(packs: Vec<Pack>) -> Catalog {
        Catalog { version: 1, generated: "2999-01-01".into(), starter_sets: Vec::new(), packs, withdrawn: Vec::new() }
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
        const SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        const BLAKE3: &str = "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85";
        const SHA1: &str = "qZk+NkcGgWq6PiVxeFDCbJzQ2J0=";
        let mut read = 0;
        let d = hash_file(&p, Checksum::Sha256, || false, |n| read += n).unwrap().unwrap();
        assert_eq!(d.sha256.as_deref(), Some(SHA256));
        assert_eq!(read, 3, "what was read is reported");
        assert!(d.matches_hash(&SHA256.to_uppercase(), None, None));
        assert!(!d.matches_hash("", None, Some(SHA1)), "no SHA-1 was computed to compare");
        assert!(!d.matches_hash("", None, None), "nothing to compare with: never a match");
        let d = hash_file(&p, Checksum::Blake3, || false, |_| {}).unwrap().unwrap();
        assert_eq!(d.blake3.as_deref(), Some(BLAKE3), "BLAKE3 of abc");
        assert!(d.sha256.is_none() && d.sha1_base64.is_none(), "only the checksum asked for");
        assert!(d.matches_hash("", Some(BLAKE3), None));
        assert!(!d.matches_hash("", Some(&"0".repeat(64)), None));
        assert!(!d.matches_hash(SHA256, Some(BLAKE3), None), "with a SHA-256 given, the SHA-256 counts");
        let d = hash_file(&p, Checksum::Sha1, || false, |_| {}).unwrap().unwrap();
        assert_eq!(d.sha1_base64.as_deref(), Some(SHA1), "SHA-1 of abc");
        assert!(d.matches_hash("", None, Some(SHA1)));
        // Which one a catalog entry is checked with.
        assert_eq!(Checksum::of(SHA256, Some(BLAKE3)), Checksum::Sha256);
        assert_eq!(Checksum::of("", Some(BLAKE3)), Checksum::Blake3);
        assert_eq!(Checksum::of("", None), Checksum::Sha1);
        assert!(hash_file(&p, Checksum::Blake3, || true, |_| {}).unwrap().is_none(), "a pause stops the check");
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
        assert!(!reconcile(&library, &p1, &mut st, &[]));
        assert_eq!(st.status, PackStatus::Installed);
        assert_eq!(st.files, vec![InstalledFile::from(&p1.files[0])]);
        assert!(!st.update_available);

        // A newer catalog: still installed and in use, with an update offered.
        assert!(!reconcile(&library, &p2, &mut st, &[]));
        assert_eq!(st.status, PackStatus::Installed);
        assert_eq!(st.files[0].path, "zim/x_2026-01.zim", "the old file keeps working");
        assert!(st.update_available);

        // Recorded files that are gone: not installed any more.
        std::fs::remove_file(library.join("zim/x_2026-01.zim")).unwrap();
        assert!(!reconcile(&library, &p2, &mut st, &[]));
        assert_eq!(st.status, PackStatus::NotInstalled);
        assert!(st.files.is_empty() && !st.update_available);

        // No record (state.json lost) but a file at the catalog's path: it is checked, never trusted.
        put(&library, "zim/x_2026-02.zim", &new);
        let mut lost = PackState::not_installed(p2.size);
        assert!(reconcile(&library, &p2, &mut lost, &[]), "to be verified");
        assert_eq!(lost.status, PackStatus::Queued);
        assert!(lost.files.is_empty() && lost.installed_version.is_none());

        // An old state naming another version is not trusted either.
        let mut stale: PackState = serde_json::from_value(serde_json::json!({ "status": "installed", "bytes_done": 1, "bytes_total": 1, "installed_version": "2026-01" })).unwrap();
        assert!(reconcile(&library, &p2, &mut stale, &[]));
        assert!(stale.files.is_empty() && stale.installed_version.is_none());

        // Paths in state.json never leave the library.
        let mut evil: PackState = serde_json::from_value(serde_json::json!({ "status": "not_installed", "bytes_done": 0, "bytes_total": 0, "stale": ["../../Windows", "zim/old.zim"] })).unwrap();
        reconcile(&library, &p2, &mut evil, &[]);
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

    /// A pack the catalog stops offering stays on the hub: installed, in the
    /// library and listed (as no longer offered) until a person deletes it.
    /// Nothing downloads it again and nothing deletes it by itself.
    #[test]
    fn a_pack_the_catalog_no_longer_offers_stays_installed() {
        let (a, b, c) = (payload(11, 1000), payload(12, 1000), payload(13, 1000));
        let fa = file("zim/a_2024-08.zim", &a, "http://127.0.0.1:9/");
        let fb = file("zim/b_2025-01.zim", &b, "http://127.0.0.1:9/");
        let fc = file("zim/c.zim", &c, "http://127.0.0.1:9/");
        let state = serde_json::json!({
            // Listed by the catalog as withdrawn.
            "a": { "status": "installed", "bytes_done": 1000, "bytes_total": 1000, "installed_version": "2024-08", "files": [InstalledFile::from(&fa)] },
            // Left out of a newer catalog without a word, while an update of it was paused.
            "b": { "status": "paused", "bytes_done": 10, "bytes_total": 2000, "installed_version": "2025-01", "files": [InstalledFile::from(&fb)], "synced": { "zim/b_2026-01.zim": 10 } },
            // Only a piece of a download of a pack no catalog knows: nothing usable.
            "p": { "status": "paused", "bytes_done": 10, "bytes_total": 1000, "synced": { "zim/p.zim": 10 } },
        });
        let (library, state_path) = hub_dir(Some(state));
        put(&library, "zim/a_2024-08.zim", &a);
        put(&library, "zim/b_2025-01.zim", &b);
        put(&library, "zim/b_2026-01.zim.part", &b[..10]);
        put(&library, "zim/p.zim.part", &b[..10]);
        let mut gone = pack("a", "2024-08", PackFile { urls: Vec::new(), ..fa.clone() });
        gone.title = Localized { en: "Guides A".into(), sr: "Vodiči A".into() };
        gone.topics = vec!["health".into()];
        let mut cat = catalog(vec![pack("c", "1", fc)]);
        cat.withdrawn = vec![gone];
        let d = Downloads::new(cat, library.clone(), state_path.clone());

        let views = d.snapshot();
        let view = |id: &str| views.iter().find(|v| v.pack.id == id);
        let va = view("a").expect("still listed");
        assert!(va.withdrawn);
        assert_eq!(va.state.status, PackStatus::Installed);
        assert_eq!((va.pack.title.en.as_str(), va.pack.topics.clone()), ("Guides A", vec!["health".to_string()]), "as the catalog listed it");
        let vb = view("b").expect("listed though the catalog does not know it");
        assert!(vb.withdrawn);
        assert_eq!(vb.state.status, PackStatus::Installed, "the update cannot continue; the verified file is what it is");
        assert!(!vb.state.update_available);
        assert_eq!(vb.pack.title.en, "b_2025-01", "named after its file");
        assert_eq!(vb.pack.category, Category::Knowledge);
        assert!(!view("c").unwrap().withdrawn);
        assert!(view("p").is_none(), "nothing of it is usable");
        // The library takes installed knowledge packs with their files from this list (kiwix.rs, `books`).
        for v in [va, vb] {
            assert!(v.pack.category == Category::Knowledge && !v.state.files.is_empty(), "{} is readable", v.pack.id);
        }
        assert!(d.is_installed("a") && d.is_installed("b"));
        assert_eq!(d.installed_files("b"), vec![InstalledFile::from(&fb)]);
        // What the app is told.
        assert_eq!(serde_json::to_value(va).unwrap()["withdrawn"], true);
        assert!(serde_json::to_value(view("c").unwrap()).unwrap().get("withdrawn").is_none());
        // Nothing downloads it again.
        assert!(d.enqueue("a").unwrap_err().contains("no longer offered"));
        assert!(d.enqueue("b").unwrap_err().contains("no longer offered"));
        assert!(d.enqueue("p").unwrap_err().contains("unknown pack"));
        // Nothing was deleted, and all of it is remembered after a restart.
        for f in ["zim/a_2024-08.zim", "zim/b_2025-01.zim", "zim/b_2026-01.zim.part", "zim/p.zim.part"] {
            assert!(library.join(f).is_file(), "{f} is still there");
        }
        d.save();
        let again = Downloads::new((*d.catalog()).clone(), library.clone(), state_path.clone());
        assert!(again.is_installed("a") && again.is_installed("b"));
        assert_eq!(again.snapshot().iter().filter(|v| v.withdrawn).count(), 2);
        // A person deletes it on the laptop: gone from the disk and from the list.
        again.remove("b").unwrap();
        assert!(!library.join("zim/b_2025-01.zim").exists());
        assert!(!library.join("zim/b_2026-01.zim.part").exists(), "its unfinished update too");
        again.remove("a").unwrap();
        assert!(!library.join("zim/a_2024-08.zim").exists());
        assert!(again.snapshot().iter().all(|v| !v.withdrawn));
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

    fn map_pack(id: &str, f: PackFile) -> Pack {
        Pack { category: Category::Maps, ..pack(id, "20260928", f) }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_map_found_in_place_is_shown_while_it_is_checked() {
        no_battery_rule();
        let world = payload(11, 400_000);
        let (library, state_path) = hub_dir(None);
        put(&library, "maps/world.pmtiles", &world);
        // Right name, wrong size: never shown, and the check fails at once.
        put(&library, "maps/other.pmtiles", &world[..1000]);
        let unreachable = "http://127.0.0.1:9/nothing";
        let d = Downloads::new(
            catalog(vec![
                map_pack("world", file("maps/world.pmtiles", &world, unreachable)),
                map_pack("other", file("maps/other.pmtiles", &world, unreachable)),
                map_pack("later", file("maps/later.pmtiles", &world, unreachable)),
                // A piece of a CoMaps map is not a map archive.
                map_pack("map:Serbia", file("maps/1/Serbia.mwm", &world, unreachable)),
            ]),
            library.clone(),
            state_path,
        );
        put(&library, "maps/1/Serbia.mwm", &world);
        assert_eq!(d.state_of("world").unwrap().status, PackStatus::Queued, "checked before it counts as installed");
        let shown = |d: &Downloads| d.map_archives().into_iter().map(|(id, _)| id).collect::<Vec<_>>();
        assert_eq!(shown(&d), ["world"], "shown while it is checked");
        assert_eq!(d.map_archives()[0].1, library.join("maps/world.pmtiles"));
        d.start();
        let st = wait(&d, "world", &[PackStatus::Installed, PackStatus::Failed]).await;
        assert_eq!(st.status, PackStatus::Installed, "{:?}", st.error);
        assert_eq!(st.bytes_done, world.len() as u64, "the check counts its progress");
        assert_eq!(wait(&d, "other", &[PackStatus::Installed, PackStatus::Failed]).await.status, PackStatus::Failed);
        assert_eq!(shown(&d), ["world"], "verified, and the one of the wrong size is not shown");

        // A map put in place while the hub runs is noticed and checked too.
        put(&library, "maps/later.pmtiles", &world);
        d.notice_placed_maps();
        assert!(shown(&d).contains(&"later".to_string()));
        let st = wait(&d, "later", &[PackStatus::Installed, PackStatus::Failed]).await;
        assert_eq!(st.status, PackStatus::Installed, "{:?}", st.error);

        // Removed: not shown any more.
        d.remove("world").unwrap();
        assert_eq!(shown(&d), ["later"]);
        assert!(!library.join("maps/world.pmtiles").exists());
    }

    // ---- the world map --------------------------------------------------------

    fn blake3_hex(b: &[u8]) -> String {
        blake3::hash(b).to_hex().to_string()
    }

    /// A small stand-in for a world map build: checked with BLAKE3 like a
    /// listed build, or with SHA-256 like the pinned one.
    fn world_build(date: &str, bytes: &[u8], url: &str, with_sha256: bool) -> Pack {
        let f = PackFile {
            path: world_map::path_of(date),
            urls: vec![url.into()],
            sha256: if with_sha256 { sha256_hex(bytes) } else { String::new() },
            sha1_base64: None,
            blake3: (!with_sha256).then(|| blake3_hex(bytes)),
            size: bytes.len() as u64,
            unpack: None,
            unpack_to: None,
        };
        Pack { category: Category::Maps, topics: vec!["maps".into()], ..pack(WORLD_MAP_ID, date, f) }
    }

    /// An offer of `offer`, knowing `known`, of which `downloadable` can still be downloaded.
    fn offer_of(offer: &Pack, known: &[&Pack], downloadable: &[&str]) -> WorldOffer {
        WorldOffer {
            offer: offer.clone(),
            known: known.iter().map(|p| (*p).clone()).collect(),
            downloadable: downloadable.iter().map(|s| s.to_string()).collect(),
            listed: true,
        }
    }

    fn installed_state(p: &Pack) -> serde_json::Value {
        serde_json::json!({
            WORLD_MAP_ID: { "status": "installed", "bytes_done": p.size, "bytes_total": p.size, "installed_version": p.version, "files": [InstalledFile::from(&p.files[0])] },
        })
    }

    fn shown(d: &Downloads) -> Vec<PathBuf> {
        d.map_archives().into_iter().map(|(_, p)| p).collect()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_download_is_checked_with_its_blake3() {
        no_battery_rule();
        let good = payload(21, 300_000);
        let files = temp("files");
        std::fs::write(files.join("w.pmtiles"), &good).unwrap();
        let server = serve(&files).await;
        let with_blake3 = |path: &str, hash: String, url: String| PackFile {
            sha256: String::new(),
            blake3: Some(hash),
            ..file(path, &good, &url)
        };
        let (library, state_path) = hub_dir(None);
        let d = Downloads::new(
            catalog(vec![
                pack("good", "1", with_blake3("maps/good.pmtiles", blake3_hex(&good).to_uppercase(), format!("{server}/w.pmtiles"))),
                pack("bad", "1", with_blake3("maps/bad.pmtiles", blake3_hex(&good[1..]), format!("{server}/w.pmtiles"))),
                // Every address answers that the file is not there.
                pack("gone", "1", with_blake3("maps/gone.pmtiles", blake3_hex(&good), format!("{server}/deleted.pmtiles"))),
                // One is not there, another does not answer: only a failed download.
                pack("mixed", "1", PackFile { urls: vec![format!("{server}/deleted.pmtiles"), "http://127.0.0.1:9/w.pmtiles".into()], ..with_blake3("maps/mixed.pmtiles", blake3_hex(&good), String::new()) }),
            ]),
            library.clone(),
            state_path,
        );
        assert!(d.catalog().packs.iter().all(|p| p.is_safe()), "a BLAKE3 alone is a checksum");
        d.start();
        for id in ["good", "bad", "gone", "mixed"] {
            d.enqueue(id).unwrap();
        }
        let st = wait(&d, "good", &[PackStatus::Installed, PackStatus::Failed]).await;
        assert_eq!(st.status, PackStatus::Installed, "{:?}", st.error);
        assert_eq!(std::fs::read(library.join("maps/good.pmtiles")).unwrap(), good);
        assert_eq!(st.files[0].blake3.as_deref(), Some(blake3_hex(&good).to_uppercase().as_str()));
        assert!(st.files[0].sha256.is_empty());
        let st = wait(&d, "bad", &[PackStatus::Installed, PackStatus::Failed]).await;
        assert_eq!(st.status, PackStatus::Failed);
        assert!(st.error.unwrap().contains("checksum mismatch"));
        assert!(!library.join("maps/bad.pmtiles").exists() && !library.join("maps/bad.pmtiles.part").exists(), "discarded");
        let st = wait(&d, "gone", &[PackStatus::Installed, PackStatus::Failed]).await;
        assert_eq!(st.error.as_deref(), Some(GONE), "a clear reason, at once");
        let st = wait(&d, "mixed", &[PackStatus::Installed, PackStatus::Failed]).await;
        assert!(st.error.unwrap().starts_with("download failed"));

        // A file found in place is checked with its BLAKE3 too (a restart without the state).
        let _ = std::fs::remove_file(library.parent().unwrap().join("state.json"));
        let again = Downloads::new((*d.catalog()).clone(), library.clone(), library.parent().unwrap().join("state2.json"));
        std::fs::write(library.join("maps/bad.pmtiles"), &good).unwrap();
        assert_eq!(again.state_of("good").unwrap().status, PackStatus::Queued, "checked before it counts");
        again.start();
        assert_eq!(wait(&again, "good", &[PackStatus::Installed, PackStatus::Failed]).await.status, PackStatus::Installed);
    }

    /// The build machine's library: the pinned build recorded as installed
    /// (its file hard-linked there). With the real offer of today, it stays
    /// installed, in use, and nothing newer is offered.
    #[test]
    fn the_pinned_build_on_disk_stays_installed_with_todays_offer() {
        let list = world_map::parse_builds(include_str!("../../zaklon-core/testdata/protomaps-builds-20260929.json")).unwrap();
        let now = time::OffsetDateTime::parse("2026-09-29T09:00:00Z", &time::format_description::well_known::Rfc3339).unwrap();
        let w = world_map::offer(Some(&list), now);
        assert_eq!(w.offer.version, "20260811");
        let pinned = world_map::pinned();
        let (library, state_path) = hub_dir(Some(installed_state(&pinned)));
        // Only its name counts at startup (it is not read again then).
        put(&library, "maps/protomaps-world-20260928.pmtiles", b"a stand-in for 138 GB");
        let mut cat = catalog(vec![]);
        cat.packs.push(pinned.clone());
        let d = Downloads::with_world(cat, library.clone(), state_path, Some(w));
        let st = d.state_of(WORLD_MAP_ID).unwrap();
        assert_eq!(st.status, PackStatus::Installed);
        assert!(!st.update_available, "an older build is no update");
        assert_eq!(st.files[0].sha256, world_map::PINNED_SHA256);
        assert_eq!(shown(&d), [library.join("maps/protomaps-world-20260928.pmtiles")]);
        assert_eq!(d.catalog().pack(WORLD_MAP_ID).unwrap().version, "20260811", "what is offered to others");
        let view = d.world_view().unwrap();
        assert_eq!((view.offered.as_str(), view.installed.as_deref(), view.update, view.listed), ("20260811", Some("20260928"), false, true));
        assert!(library.join("maps/protomaps-world-20260928.pmtiles").is_file(), "nothing is deleted");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_pinned_build_put_in_place_is_checked_with_its_sha256() {
        no_battery_rule();
        let unreachable = "http://127.0.0.1:9/nothing";
        let (pinned_bytes, offer_bytes, newer_bytes) = (payload(22, 200_000), payload(23, 210_000), payload(24, 220_000));
        let pinned = world_build("20260928", &pinned_bytes, unreachable, true);
        let offered = world_build("20260811", &offer_bytes, unreachable, false);
        let (library, state_path) = hub_dir(None);
        put(&library, &pinned.files[0].path, &pinned_bytes);
        let d = Downloads::with_world(catalog(vec![pinned.clone()]), library.clone(), state_path, Some(offer_of(&offered, &[&pinned, &offered], &["20260811"])));
        assert_eq!(d.catalog().pack(WORLD_MAP_ID).unwrap().version, "20260811");
        assert_eq!(d.state_of(WORLD_MAP_ID).unwrap().status, PackStatus::Queued, "found in place: checked first");
        assert_eq!(shown(&d), [library.join("maps/protomaps-world-20260928.pmtiles")], "shown while it is checked");
        assert_eq!(d.world_view().unwrap().installed.as_deref(), Some("20260928"));
        d.start();
        let st = wait(&d, WORLD_MAP_ID, &[PackStatus::Installed, PackStatus::Failed]).await;
        assert_eq!(st.status, PackStatus::Installed, "{:?}", st.error);
        assert_eq!(st.files, vec![InstalledFile::from(&pinned.files[0])], "checked with the pinned SHA-256");
        assert_eq!(st.installed_version.as_deref(), Some("20260928"));
        assert!(!st.update_available, "the offer is older");
        // A newer build appears in the list: an update, and the old build keeps working.
        let newer = world_build("20261019", &newer_bytes, unreachable, false);
        d.set_world(offer_of(&newer, &[&pinned, &offered, &newer], &["20260811", "20261019"]));
        let st = d.state_of(WORLD_MAP_ID).unwrap();
        assert!(st.update_available && st.status == PackStatus::Installed);
        let view = d.world_view().unwrap();
        assert_eq!((view.offered.as_str(), view.offered_size, view.update), ("20261019", 220_000, true));
        assert_eq!(shown(&d), [library.join("maps/protomaps-world-20260928.pmtiles")]);

        // Put in place with the wrong content: never taken.
        let (library2, state_path2) = hub_dir(None);
        put(&library2, &pinned.files[0].path, &payload(25, 200_000));
        let d2 = Downloads::with_world(catalog(vec![pinned.clone()]), library2, state_path2, Some(offer_of(&offered, &[&pinned, &offered], &["20260811"])));
        d2.start();
        let st = wait(&d2, WORLD_MAP_ID, &[PackStatus::Installed, PackStatus::Failed]).await;
        assert_eq!(st.status, PackStatus::Failed);
        assert!(st.files.is_empty() && shown(&d2).is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_update_replaces_the_old_build_only_once_the_new_one_is_verified() {
        no_battery_rule();
        let (old_bytes, new_bytes) = (payload(26, 250_000), payload(27, 260_000));
        let files = temp("files");
        std::fs::write(files.join("20261019.pmtiles"), &new_bytes).unwrap();
        std::fs::write(files.join("20261102.pmtiles"), &new_bytes).unwrap();
        let server = serve(&files).await;
        let old = world_build("20260811", &old_bytes, "http://127.0.0.1:9/", false);
        // Its BLAKE3 does not match what the server sends.
        let damaged = Pack { files: vec![PackFile { blake3: Some(blake3_hex(&old_bytes)), ..world_build("20261019", &new_bytes, &format!("{server}/20261019.pmtiles"), false).files[0].clone() }], ..world_build("20261019", &new_bytes, "", false) };
        let good = world_build("20261102", &new_bytes, &format!("{server}/20261102.pmtiles"), false);
        let (library, state_path) = hub_dir(Some(installed_state(&old)));
        put(&library, &old.files[0].path, &old_bytes);
        let d = Downloads::with_world(catalog(vec![old.clone()]), library.clone(), state_path, Some(offer_of(&damaged, &[&old, &damaged], &["20260811", "20261019"])));
        *d.fake_disk.lock().unwrap() = Some(1 << 40);
        assert!(d.state_of(WORLD_MAP_ID).unwrap().update_available);
        d.start();
        d.update_world(false).await.unwrap();
        let st = wait(&d, WORLD_MAP_ID, &[PackStatus::Failed, PackStatus::Installed]).await;
        assert_eq!(st.status, PackStatus::Failed);
        assert!(st.error.as_deref().unwrap_or_default().contains("checksum mismatch"));
        // The old build is still there, recorded, and on the map.
        assert_eq!(st.files, vec![InstalledFile::from(&old.files[0])]);
        assert!(st.update_available);
        assert_eq!(shown(&d), [library.join(&old.files[0].path)]);
        assert_eq!(std::fs::read(library.join(&old.files[0].path)).unwrap(), old_bytes);

        // A good newer build: verified, in use, and only then the old one goes.
        d.set_world(offer_of(&good, &[&old, &damaged, &good], &["20260811", "20261019", "20261102"]));
        assert_eq!(d.catalog().pack(WORLD_MAP_ID).unwrap().version, "20261102");
        d.update_world(false).await.unwrap();
        let st = wait(&d, WORLD_MAP_ID, &[PackStatus::Installed]).await;
        assert_eq!(st.files, vec![InstalledFile::from(&good.files[0])]);
        assert_eq!(st.installed_version.as_deref(), Some("20261102"));
        assert!(!st.update_available && st.stale.is_empty(), "{st:?}");
        assert!(!library.join(&old.files[0].path).exists(), "the old build is deleted");
        assert_eq!(std::fs::read(library.join(&good.files[0].path)).unwrap(), new_bytes);
        assert_eq!(shown(&d), [library.join(&good.files[0].path)]);
        assert!(d.update_world(false).await.unwrap_err().contains("no newer world map"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn without_room_for_both_the_old_build_goes_first_only_when_asked() {
        no_battery_rule();
        let (old_bytes, new_bytes) = (payload(28, 300_000), payload(29, 400_000));
        let files = temp("files");
        std::fs::write(files.join("20261019.pmtiles"), &new_bytes).unwrap();
        let server = serve(&files).await;
        let old = world_build("20260811", &old_bytes, "http://127.0.0.1:9/", false);
        let new = world_build("20261019", &new_bytes, &format!("{server}/20261019.pmtiles"), false);
        let (library, state_path) = hub_dir(Some(installed_state(&old)));
        put(&library, &old.files[0].path, &old_bytes);
        let d = Downloads::with_world(catalog(vec![old.clone()]), library.clone(), state_path, Some(offer_of(&new, &[&old, &new], &["20260811", "20261019"])));
        let (old_size, new_size) = (old_bytes.len() as u64, new_bytes.len() as u64);
        // A disk one byte too small for both (with what is kept free).
        *d.fake_disk.lock().unwrap() = Some(old_size + new_size + DISK_MARGIN - 1);
        let view = d.world_view().unwrap();
        assert!(!view.room_for_both);
        assert_eq!((view.needed, view.disk_free), (new_size + DISK_MARGIN, new_size + DISK_MARGIN - 1));
        d.start();
        let refused = d.update_world(false).await.unwrap_err();
        assert!(refused.contains("for both world maps"), "{refused}");
        assert_eq!(api_code(&refused), "world_no_room");
        assert!(library.join(&old.files[0].path).is_file(), "nothing was deleted");
        assert_eq!(d.state_of(WORLD_MAP_ID).unwrap().status, PackStatus::Installed);

        // Even without the old build it would not fit: nothing is deleted.
        *d.fake_disk.lock().unwrap() = Some(new_size + DISK_MARGIN - 1);
        assert_eq!(d.update_world(true).await.unwrap_err(), "not enough free disk space");
        assert!(library.join(&old.files[0].path).is_file());

        // Confirmed, and removing the old build makes room: it goes first.
        *d.fake_disk.lock().unwrap() = Some(old_size + new_size + DISK_MARGIN - 1);
        d.update_world(true).await.unwrap();
        assert!(!library.join(&old.files[0].path).exists(), "the old build went first");
        let st = wait(&d, WORLD_MAP_ID, &[PackStatus::Installed, PackStatus::Failed]).await;
        assert_eq!(st.status, PackStatus::Installed, "{:?}", st.error);
        assert_eq!(st.files, vec![InstalledFile::from(&new.files[0])]);
        assert_eq!(shown(&d), [library.join(&new.files[0].path)]);
    }

    /// The hub's code for a message (see api/error.rs).
    fn api_code(msg: &str) -> &'static str {
        crate::api::error_code(axum::http::StatusCode::BAD_REQUEST, msg)
    }

    #[test]
    fn an_unfinished_download_goes_on_while_its_build_can_be_downloaded() {
        let (a_bytes, b_bytes) = (payload(30, 100_000), payload(31, 110_000));
        let a = world_build("20260811", &a_bytes, "http://127.0.0.1:9/a", false);
        let b = world_build("20261019", &b_bytes, "http://127.0.0.1:9/b", false);
        let part = format!("{}.part", a.files[0].path);
        let state = serde_json::json!({
            WORLD_MAP_ID: { "status": "paused", "bytes_done": 5000, "bytes_total": a.size, "synced": { &a.files[0].path: 5000 } },
        });
        let (library, state_path) = hub_dir(Some(state));
        put(&library, &part, &a_bytes[..5000]);
        let d = Downloads::with_world(catalog(vec![a.clone()]), library.clone(), state_path, Some(offer_of(&a, &[&a], &["20260811"])));
        // A newer build is offered, and the one under way is still there to download: it goes on.
        d.set_world(offer_of(&b, &[&a, &b], &["20260811", "20261019"]));
        assert_eq!(d.catalog().pack(WORLD_MAP_ID).unwrap().version, "20260811");
        let st = d.state_of(WORLD_MAP_ID).unwrap();
        assert_eq!((st.status.clone(), st.bytes_done), (PackStatus::Paused, 5000));
        assert!(library.join(&part).is_file());
        assert_eq!(d.world_view().unwrap().offered, "20260811", "Add-ons names the build under way");
        // Its build left the list (Protomaps deleted it): it cannot go on, and its piece goes.
        d.set_world(offer_of(&b, &[&b], &["20261019"]));
        assert_eq!(d.catalog().pack(WORLD_MAP_ID).unwrap().version, "20261019");
        let st = d.state_of(WORLD_MAP_ID).unwrap();
        assert_eq!((st.status.clone(), st.bytes_done, st.bytes_total), (PackStatus::NotInstalled, 0, b.size));
        assert!(st.synced.is_empty());
        assert!(!library.join(&part).exists(), "{st:?}");
    }
}
