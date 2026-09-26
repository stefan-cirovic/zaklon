//! Add-on downloads: one pack at a time, resumable with HTTP ranges, verified
//! with SHA-256, optionally unpacked, and importable from or exportable to a
//! folder (USB stick). State survives restarts in `<root>/catalog/state.json`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::sync::Notify;
use tracing::{info, warn};
use zaklon_core::catalog::{Catalog, Pack, PackFile, PackState, PackStatus};

// `library` is needed before `Self` exists in `new`.

/// Downloads stop below this battery level unless the charger is connected (SPEC §6).
pub const MIN_BATTERY_PERCENT: u8 = 50;
/// Keep at least this much free after a download.
const DISK_MARGIN: u64 = 512 * 1024 * 1024;
const STATE_SAVE_INTERVAL: Duration = Duration::from_secs(2);
/// A download with no data for this long is treated as a broken connection.
const STALL_TIMEOUT: Duration = Duration::from_secs(60);
/// Written into an unpack folder once unpacking finished.
const UNPACKED_MARKER: &str = ".zaklon-unpacked";

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

pub struct Downloads {
    catalog: Catalog,
    library: PathBuf,
    state_path: PathBuf,
    states: Mutex<HashMap<String, PackState>>,
    queue: Mutex<VecDeque<String>>,
    pause_requests: Mutex<HashSet<String>>,
    notify: Notify,
    client: reqwest::Client,
}

impl Downloads {
    pub fn new(catalog: Catalog, library: PathBuf, state_path: PathBuf) -> Arc<Self> {
        let library_ref = library.clone();
        let library = &library_ref;
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
        for p in &catalog.packs {
            let st = states.entry(p.id.clone()).or_insert_with(|| PackState::not_installed(p.size));
            // state.json may be missing or stale (e.g. after a power cut): trust complete files on disk.
            if st.status != PackStatus::Installed && pack_complete_on_disk(library, p) {
                st.status = PackStatus::Installed;
                st.bytes_done = p.size;
                st.bytes_total = p.size;
                st.installed_version = Some(p.version.clone());
                st.error = None;
            }
        }
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
        Arc::new(Self {
            catalog,
            library: library_ref.clone(),
            state_path,
            states: Mutex::new(states),
            queue: Mutex::new(VecDeque::new()),
            pause_requests: Mutex::new(HashSet::new()),
            notify: Notify::new(),
            client,
        })
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
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

    pub fn library_dir(&self) -> &Path {
        &self.library
    }

    pub fn enqueue(&self, id: &str) -> Result<(), String> {
        let pack = self.catalog.pack(id).ok_or("unknown pack")?.clone();
        {
            let mut states = self.states.lock().unwrap_or_else(|p| p.into_inner());
            let st = states.entry(id.to_string()).or_insert_with(|| PackState::not_installed(pack.size));
            match st.status {
                PackStatus::Installed => return Err("already installed".into()),
                PackStatus::Queued | PackStatus::Downloading | PackStatus::Verifying => {
                    // Resume right after a pause request: cancel the request.
                    self.pause_requests.lock().unwrap_or_else(|p| p.into_inner()).remove(id);
                    return Ok(());
                }
                _ => {}
            }
            st.status = PackStatus::Queued;
            st.error = None;
        }
        self.pause_requests.lock().unwrap_or_else(|p| p.into_inner()).remove(id);
        self.queue.lock().unwrap_or_else(|p| p.into_inner()).push_back(id.to_string());
        self.save();
        self.notify.notify_one();
        Ok(())
    }

    pub fn pause(&self, id: &str) -> Result<(), String> {
        let mut states = self.states.lock().unwrap_or_else(|p| p.into_inner());
        let st = states.get_mut(id).ok_or("unknown pack")?;
        match st.status {
            PackStatus::Queued => {
                self.queue.lock().unwrap_or_else(|p| p.into_inner()).retain(|q| q != id);
                st.status = PackStatus::Paused;
            }
            PackStatus::Downloading | PackStatus::Verifying => {
                self.pause_requests.lock().unwrap_or_else(|p| p.into_inner()).insert(id.to_string());
            }
            _ => return Err("nothing to pause".into()),
        }
        drop(states);
        self.save();
        Ok(())
    }

    /// Delete a pack's files (finished or partial) and forget its state.
    pub fn remove(&self, id: &str) -> Result<(), String> {
        let pack = self.catalog.pack(id).ok_or("unknown pack")?;
        if let Some(st) = self.state_of(id) {
            if matches!(st.status, PackStatus::Downloading | PackStatus::Verifying) {
                return Err("pause the download first".into());
            }
        }
        self.queue.lock().unwrap_or_else(|p| p.into_inner()).retain(|q| q != id);
        let gone = |r: std::io::Result<()>| match r {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        };
        for f in &pack.files {
            let dest = self.library.join(&f.path);
            gone(remove_file_retry(&dest)).map_err(|e| format!("could not delete {}: {e}", f.path))?;
            gone(std::fs::remove_file(part_path(&dest)))?;
            if let Some(dir) = &f.unpack_to {
                gone(std::fs::remove_dir_all(self.library.join(dir))).map_err(|e| format!("could not delete {dir}: {e}"))?;
            }
        }
        self.states
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id.to_string(), PackState::not_installed(pack.size));
        self.save();
        Ok(())
    }

    /// Copy verified pack files out of `dir` (a USB stick) into the library.
    /// Looks in `dir` itself and in `dir/zaklon-packs`. Returns the pack ids imported.
    pub fn import_from_dir(&self, dir: &Path) -> Result<Vec<String>, String> {
        let candidates = [dir.to_path_buf(), dir.join("zaklon-packs")];
        let mut imported = Vec::new();
        for pack in &self.catalog.packs {
            let busy_or_done = matches!(
                self.state_of(&pack.id).map(|s| s.status),
                Some(PackStatus::Installed | PackStatus::Queued | PackStatus::Downloading | PackStatus::Verifying)
            );
            if busy_or_done {
                continue;
            }
            let mut sources = Vec::new();
            for f in &pack.files {
                let name = Path::new(&f.path).file_name().ok_or("bad path")?;
                match candidates.iter().map(|c| c.join(name)).find(|p| p.is_file()) {
                    Some(src) => sources.push((f.clone(), src)),
                    None => break,
                }
            }
            if sources.len() != pack.files.len() {
                continue;
            }
            self.set(&pack.id, |s| {
                s.status = PackStatus::Verifying;
                s.bytes_done = 0;
                s.error = None;
            });
            let mut ok = true;
            for (f, src) in &sources {
                if let Err(e) = self.import_file(&pack.id, f, src) {
                    self.set(&pack.id, |s| {
                        s.status = PackStatus::Failed;
                        s.error = Some(e);
                    });
                    ok = false;
                    break;
                }
                self.set(&pack.id, |s| s.bytes_done += f.size);
            }
            if ok {
                let version = pack.version.clone();
                self.set(&pack.id, |s| {
                    s.status = PackStatus::Installed;
                    s.bytes_done = s.bytes_total;
                    s.installed_version = Some(version.clone());
                });
                info!(pack = %pack.id, "imported from folder");
                imported.push(pack.id.clone());
            }
            self.save();
        }
        Ok(imported)
    }

    fn import_file(&self, _id: &str, f: &PackFile, src: &Path) -> Result<(), String> {
        let dest = self.library.join(&f.path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let tmp = part_path(&dest);
        let copied = copy_with_hash(src, &tmp).map_err(|e| format!("copying {}: {e}", f.path))?;
        if copied != f.sha256 {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("checksum mismatch for {}", f.path));
        }
        unpack(&self.library, f, &tmp)?;
        rename_retry(&tmp, &dest).map_err(|e| format!("moving {} into place: {e}", f.path))
    }

    /// Copy an installed pack's files to `dir/zaklon-packs` (USB stick).
    pub fn export_to_dir(&self, id: &str, dir: &Path) -> Result<PathBuf, String> {
        let pack = self.catalog.pack(id).ok_or("unknown pack")?;
        if !self.is_installed(id) {
            return Err("pack is not installed".into());
        }
        let target = dir.join("zaklon-packs");
        std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
        for f in &pack.files {
            let src = self.library.join(&f.path);
            let name = Path::new(&f.path).file_name().ok_or("bad path")?;
            std::fs::copy(&src, target.join(name)).map_err(|e| format!("copying {}: {e}", f.path))?;
        }
        Ok(target)
    }

    // ---- internals ----------------------------------------------------------

    fn set(&self, id: &str, f: impl FnOnce(&mut PackState)) {
        let mut states = self.states.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(st) = states.get_mut(id) {
            f(st);
        }
    }

    fn save(&self) {
        let states = self.states.lock().unwrap_or_else(|p| p.into_inner());
        if let Ok(json) = serde_json::to_string_pretty(&*states) {
            if let Some(parent) = self.state_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = zaklon_core::config::write_atomic(&self.state_path, json.as_bytes()) {
                warn!("saving download state: {e}");
            }
        }
    }

    fn pause_requested(&self, id: &str) -> bool {
        self.pause_requests.lock().unwrap_or_else(|p| p.into_inner()).contains(id)
    }

    async fn run_pack(&self, id: &str) {
        let Some(pack) = self.catalog.pack(id).cloned() else { return };
        let already = pack
            .files
            .iter()
            .map(|f| {
                let dest = self.library.join(&f.path);
                if dest.is_file() {
                    f.size
                } else {
                    std::fs::metadata(part_path(&dest)).map(|m| m.len().min(f.size)).unwrap_or(0)
                }
            })
            .sum::<u64>();
        self.set(id, |s| {
            s.status = PackStatus::Downloading;
            s.bytes_done = already;
            s.bytes_total = pack.size;
            s.error = None;
        });
        self.save();

        let sys = system_info(&self.library);
        // ZAKLON_IGNORE_BATTERY=1 is for automated tests on laptops running on battery.
        let ignore_battery = std::env::var("ZAKLON_IGNORE_BATTERY").is_ok_and(|v| v == "1");
        if !ignore_battery && !sys.plugged_in && sys.battery_percent.map(|p| p < MIN_BATTERY_PERCENT).unwrap_or(false) {
            self.finish(id, Outcome::Failed(format!("battery below {MIN_BATTERY_PERCENT}%: plug in the charger and resume")));
            return;
        }
        if sys.disk_free < pack.size.saturating_sub(already) + DISK_MARGIN {
            self.finish(id, Outcome::Failed("not enough free disk space".into()));
            return;
        }

        let mut outcome = Outcome::Done;
        for f in &pack.files {
            match self.download_file(id, f).await {
                Outcome::Done => {}
                other => {
                    outcome = other;
                    break;
                }
            }
        }
        if matches!(outcome, Outcome::Done) {
            let version = pack.version.clone();
            self.set(id, |s| {
                s.installed_version = Some(version.clone());
                s.bytes_done = s.bytes_total;
            });
        }
        self.finish(id, outcome);
    }

    fn finish(&self, id: &str, outcome: Outcome) {
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
                }
            }
        });
        self.save();
    }

    async fn download_file(&self, id: &str, f: &PackFile) -> Outcome {
        let dest = self.library.join(&f.path);
        if dest.is_file() {
            // Already downloaded and verified earlier (finished files are only ever renamed into
            // place). If unpacking did not finish last time, do it now.
            if !unpacked_ok(&self.library, f) {
                let (f2, d2, lib) = (f.clone(), dest.clone(), self.library.clone());
                match tokio::task::spawn_blocking(move || unpack(&lib, &f2, &d2)).await {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => return Outcome::Failed(e),
                    Err(e) => return Outcome::Failed(format!("unpack task: {e}")),
                }
            }
            return Outcome::Done;
        }
        if let Some(parent) = dest.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return Outcome::Failed(format!("creating folder: {e}"));
            }
        }
        let part = part_path(&dest);
        let mut have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        if have > f.size {
            let _ = std::fs::remove_file(&part);
            have = 0;
        }

        if have < f.size {
            let mut last_err = String::from("no download locations");
            let mut done = false;
            for url in &f.urls {
                match self.fetch_range(id, f, url, &part, &mut have).await {
                    Ok(true) => {
                        done = true;
                        break;
                    }
                    Ok(false) => return Outcome::Paused,
                    Err(e) => {
                        warn!(pack = id, url, "download error: {e}");
                        last_err = e;
                    }
                }
            }
            if !done {
                return Outcome::Failed(format!("download failed: {last_err}"));
            }
        }

        self.set(id, |s| {
            s.status = PackStatus::Verifying;
            s.speed = 0;
        });
        self.save();
        let part_clone = part.clone();
        let hash = match tokio::task::spawn_blocking(move || hash_file(&part_clone)).await {
            Ok(Ok(h)) => h,
            Ok(Err(e)) => return Outcome::Failed(format!("reading file: {e}")),
            Err(e) => return Outcome::Failed(format!("verify task: {e}")),
        };
        if hash != f.sha256 {
            let _ = std::fs::remove_file(&part);
            self.set(id, |s| s.bytes_done = s.bytes_done.saturating_sub(f.size));
            return Outcome::Failed("checksum mismatch, the file was discarded; try again".into());
        }
        // Unpack from the verified .part first, then rename: a finished file
        // therefore always means "verified and unpacked".
        let (f2, p2, lib) = (f.clone(), part.clone(), self.library.clone());
        match tokio::task::spawn_blocking(move || unpack(&lib, &f2, &p2)).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Outcome::Failed(e),
            Err(e) => return Outcome::Failed(format!("unpack task: {e}")),
        }
        if let Err(e) = rename_retry(&part, &dest) {
            return Outcome::Failed(format!("moving file into place: {e}"));
        }
        self.set(id, |s| s.status = PackStatus::Downloading);
        Outcome::Done
    }

    /// Download `url` into `part` starting at offset `*have`. Ok(true) when the
    /// file is complete, Ok(false) when paused, Err on a network problem.
    async fn fetch_range(&self, id: &str, f: &PackFile, url: &str, part: &Path, have: &mut u64) -> Result<bool, String> {
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
        let server_total: Option<u64> = if status == reqwest::StatusCode::PARTIAL_CONTENT {
            header("content-range").and_then(|s| s.rsplit('/').next().and_then(|t| t.trim().parse().ok()))
        } else {
            header("content-length").and_then(|t| t.trim().parse().ok())
        };
        if header("content-type").is_some_and(|t| t.to_ascii_lowercase().starts_with("text/html")) {
            return Err("this address returns a web page, not the pack file".into());
        }
        let expected = server_total.unwrap_or(f.size);
        if expected != f.size {
            warn!(pack = id, path = %f.path, catalog = f.size, server = expected, "size differs from catalog");
        }
        // Hard limit: never write more than a little over the expected size.
        let limit = expected.max(f.size) + 1024 * 1024;
        let mut file = if *have > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT {
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
            std::fs::File::create(part).map_err(|e| e.to_string())?
        } else {
            return Err(format!("server replied {status}"));
        };

        let mut stream = res.bytes_stream();
        let mut last_save = Instant::now();
        let mut tick = Instant::now();
        let mut tick_bytes: u64 = 0;
        let mut idle = Duration::ZERO;
        loop {
            // Wake up every second so a pause takes effect even when no data arrives.
            let next = match tokio::time::timeout(Duration::from_secs(1), stream.next()).await {
                Ok(n) => n,
                Err(_) => {
                    if self.pause_requested(id) {
                        file.flush().ok();
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
                drop(file);
                let _ = std::fs::remove_file(part);
                self.set(id, |s| s.bytes_done = s.bytes_done.saturating_sub(*have));
                *have = 0;
                return Err("the server sent more data than expected".into());
            }
            file.write_all(&chunk).map_err(|e| e.to_string())?;
            *have += chunk.len() as u64;
            tick_bytes += chunk.len() as u64;
            let n = chunk.len() as u64;
            self.set(id, |s| s.bytes_done += n);
            if tick.elapsed() >= Duration::from_secs(1) {
                let speed = (tick_bytes as f64 / tick.elapsed().as_secs_f64()) as u64;
                self.set(id, |s| s.speed = speed);
                tick = Instant::now();
                tick_bytes = 0;
            }
            if last_save.elapsed() >= STATE_SAVE_INTERVAL {
                self.save();
                last_save = Instant::now();
            }
            if self.pause_requested(id) {
                file.flush().ok();
                return Ok(false);
            }
        }
        file.flush().map_err(|e| e.to_string())?;
        if *have < expected {
            return Err(format!("connection ended early at {} of {} bytes", *have, expected));
        }
        Ok(true)
    }

}

/// All files of a pack are in place (and unpacked where needed).
fn pack_complete_on_disk(library: &Path, p: &Pack) -> bool {
    p.files.iter().all(|f| library.join(&f.path).is_file() && unpacked_ok(library, f))
}

fn unpacked_ok(library: &Path, f: &PackFile) -> bool {
    match (&f.unpack, &f.unpack_to) {
        (Some(_), Some(dir)) => library.join(dir).join(UNPACKED_MARKER).is_file(),
        (Some(_), None) => library.join("bin").join(UNPACKED_MARKER).is_file(),
        _ => true,
    }
}

/// On Windows a freshly written file can be briefly locked (antivirus scan,
/// search indexer). Retry for about two seconds before giving up.
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

fn hash_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn copy_with_hash(src: &Path, dest: &Path) -> std::io::Result<String> {
    let mut input = std::fs::File::open(src)?;
    let mut output = std::fs::File::create(dest)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        output.write_all(&buf[..n])?;
    }
    output.flush()?;
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn unpack(library: &Path, f: &PackFile, archive: &Path) -> Result<(), String> {
    let Some(kind) = f.unpack.as_deref() else { return Ok(()) };
    if kind != "zip" {
        return Err(format!("unsupported archive type {kind}"));
    }
    let rel = f.unpack_to.as_deref().unwrap_or("bin");
    if !zaklon_core::catalog::is_safe_relative(rel) {
        return Err(format!("refusing to unpack outside the library: {rel}"));
    }
    let target = library.join(rel);
    let _ = std::fs::remove_dir_all(&target);
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("opening archive: {e}"))?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = entry.enclosed_name() else { continue };
        // Flatten a single top-level folder (kiwix-tools_win-i686_x.y.z/kiwix-serve.exe -> kiwix-serve.exe).
        let rel: PathBuf = rel.components().skip(if rel.components().count() > 1 { 1 } else { 0 }).collect();
        if rel.as_os_str().is_empty() {
            continue;
        }
        let out = target.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut dest = std::fs::File::create(&out).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut dest).map_err(|e| e.to_string())?;
    }
    std::fs::write(target.join(UNPACKED_MARKER), b"ok").map_err(|e| e.to_string())?;
    info!(archive = %archive.display(), target = %target.display(), "unpacked");
    Ok(())
}

pub fn system_info(dir: &Path) -> SystemInfo {
    let probe = if dir.exists() { dir.to_path_buf() } else { dir.parent().map(Path::to_path_buf).unwrap_or_else(|| dir.to_path_buf()) };
    let disk_free = fs4::available_space(&probe).unwrap_or(0);
    let disk_total = fs4::total_space(&probe).unwrap_or(0);
    let (battery_percent, plugged_in) = battery();
    SystemInfo { disk_free, disk_total, battery_percent, plugged_in }
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

    #[test]
    fn part_path_appends_suffix() {
        assert!(part_path(Path::new("a/b.zim")).to_string_lossy().ends_with("b.zim.part"));
    }

    #[test]
    fn hashing_matches_known_value() {
        let dir = std::env::temp_dir().join(format!("zaklon-hash-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("x.bin");
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(hash_file(&p).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
