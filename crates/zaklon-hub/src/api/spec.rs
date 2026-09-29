//! The system specification (Settings › About): Zaklon's version and build,
//! what it is built with, and this hub's database, AI, library, maps and
//! network, to send with a bug report. Read-only, for the laptop and paired
//! phones, and never anything secret: no password or its hash, no token, no
//! key; of the certificate only the start of its fingerprint, which phones
//! show anyway.
//!
//! It answers at once. What takes running a program to find out (how
//! Windows files the network, the libzim in the library engine) is read on
//! a thread of its own the first time and kept; until then it is named in
//! `pending`, and the app asks again a moment later.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use axum::{extract::State, Json};
use serde::Serialize;
use zaklon_core::catalog::{Category, PackStatus};

use super::error::ApiError;
use super::{blocking, Caller};
use crate::downloads::PackView;
use crate::HubState;

/// Written by build.rs.
const BUILT_ON: &str = env!("ZAKLON_BUILT_ON");
const BUILT_FROM: &str = env!("ZAKLON_BUILT_FROM");
const RUST_VERSION: &str = env!("ZAKLON_RUST_VERSION");
const TAURI_VERSION: &str = env!("ZAKLON_TAURI_VERSION");

/// How Windows files the network is read again after this long (it changes
/// when someone makes the network private); the last answer is given meanwhile.
#[cfg_attr(not(windows), allow(dead_code))]
const PROFILES_KEPT: Duration = Duration::from_secs(60);
/// A program asked for its version that has not answered by then is ended.
const PROGRAM_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Serialize)]
pub(super) struct Spec {
    app: App,
    built_with: BuiltWith,
    database: Database,
    assistant: AssistantSpec,
    library: LibrarySpec,
    maps: MapsSpec,
    network: NetworkSpec,
    /// Values still being read ("network_profile", "libzim"): ask again shortly.
    pending: Vec<&'static str>,
}

#[derive(Serialize)]
struct App {
    version: &'static str,
    /// The day it was built (YYYY-MM-DD); "" when not known.
    build_date: &'static str,
    /// The commit it was built from (12 hex digits); "" when not known.
    commit: &'static str,
    /// "release", or "development" for a debug build.
    mode: &'static str,
    /// The hub computer's system ("Windows 11 Pro 24H2") and its build ("26100.4061").
    os: String,
    os_build: Option<String>,
    /// The WebView2 runtime the desktop window draws with.
    webview2: Option<String>,
}

/// A program Zaklon runs as a process of its own: the version on the hub,
/// or the one Add-ons offers while it is not installed.
#[derive(Serialize)]
struct Program {
    version: String,
    installed: bool,
}

#[derive(Serialize)]
struct BuiltWith {
    rust: &'static str,
    tauri: &'static str,
    sqlite: &'static str,
    /// The AI engine (llama.cpp's build, "b11202").
    llama_cpp: Option<Program>,
    /// The library engine (kiwix-tools' version) and the libzim in it.
    kiwix_serve: Option<Program>,
    libzim: Option<String>,
    /// The CoMaps app the hub hands out to phones for navigation.
    comaps: &'static str,
}

#[derive(Serialize)]
struct Database {
    sqlite: &'static str,
    /// Bytes on disk, with the write-ahead log.
    size: u64,
    /// The one-time migrations done; the schema itself has no version
    /// number (see `Db::migrations_done`).
    migrations: Vec<String>,
    /// When the newest backup was written (RFC 3339), if there is one.
    last_backup: Option<String>,
}

#[derive(Serialize)]
struct Model {
    id: String,
    title_en: String,
    title_sr: String,
    size: u64,
}

impl From<&crate::assistant::ModelChoice> for Model {
    fn from(m: &crate::assistant::ModelChoice) -> Self {
        Self { id: m.id.clone(), title_en: m.title_en.clone(), title_sr: m.title_sr.clone(), size: m.size }
    }
}

#[derive(Serialize)]
struct AssistantSpec {
    engine: crate::assistant::EngineState,
    /// The AI models on the hub, smallest first, and the one the assistant uses.
    models: Vec<Model>,
    in_use: Option<String>,
    /// The hub computer's memory, and the model it calls for (None: none fits).
    ram_total: u64,
    recommended: Option<Model>,
    cpu: String,
    /// Physical cores (read on Windows), and the threads they run.
    cores: Option<usize>,
    threads: usize,
}

#[derive(Serialize)]
struct LibrarySpec {
    /// Knowledge packs on the hub and their size on disk.
    packs: usize,
    size: u64,
    folder: String,
    /// The drive the library is on ("E:\"), with its free and total space.
    drive: String,
    drive_free: u64,
    drive_total: u64,
}

#[derive(Serialize)]
struct WorldMap {
    /// The Protomaps build, "20260928".
    build: String,
    size: u64,
}

#[derive(Serialize)]
struct MapsSpec {
    /// The world map on the hub; None: the Zaklon map shows only the overview.
    world: Option<WorldMap>,
    /// The world overview that comes with the app, and the Protomaps build it was cut from.
    overview: bool,
    overview_build: Option<String>,
    /// The CoMaps app the hub hands out, and whether it is on the hub yet.
    comaps_app: &'static str,
    comaps_app_on_hub: bool,
    /// Pieces of CoMaps maps on the hub and their size.
    comaps_maps: usize,
    comaps_size: u64,
}

#[derive(Serialize)]
struct NetworkSpec {
    /// Where phones reach the hub, and its port for them.
    addresses: Vec<String>,
    port: u16,
    /// How Windows files the networks this computer is on (None: not known, or not Windows).
    profiles: Option<Vec<crate::firewall::NetworkProfile>>,
    /// Paired phones.
    phones: i64,
    /// The start (16 hex digits) of the hub certificate's fingerprint.
    fingerprint: String,
}

/// The laptop and paired phones.
pub(super) async fn system_spec(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<Spec>, ApiError> {
    Ok(Json(blocking(move || gather(&state)).await??))
}

fn gather(state: &HubState) -> anyhow::Result<Spec> {
    let mut pending = Vec::new();
    let pc = crate::machine::computer();
    let cfg = state.config();
    let packs = state.downloads.snapshot();
    let on_hub = |v: &&PackView| v.state.status == PackStatus::Installed || !v.state.files.is_empty();
    let program = |id: &str| {
        packs.iter().find(|v| v.pack.id == id).map(|v| {
            let installed = on_hub(&v);
            let version = v.state.installed_version.clone().filter(|_| installed).unwrap_or_else(|| v.pack.version.clone());
            Program { version, installed }
        })
    };
    let size_of = |v: &&PackView| v.state.files.iter().map(|f| f.size).sum::<u64>();

    // The libzim in the library engine: kiwix-serve says, once for each copy of it.
    static LIBZIM: Slow<Option<String>> = Slow::new();
    let exe = state.library.exe();
    let libzim = match std::fs::metadata(&exe) {
        Ok(meta) => {
            let key = format!("{}|{}|{:?}", exe.display(), meta.len(), meta.modified().ok());
            LIBZIM.get(&key, None, move || libzim_of(&run_program(&exe, &["--version"])?)).unwrap_or_else(|| {
                pending.push("libzim");
                None
            })
        }
        Err(_) => None,
    };

    // How Windows files the networks: PowerShell takes a second or more.
    #[cfg(windows)]
    let profiles = {
        static PROFILES: Slow<Option<Vec<crate::firewall::NetworkProfile>>> = Slow::new();
        PROFILES.get("", Some(PROFILES_KEPT), || crate::firewall::network_profiles().ok()).unwrap_or_else(|| {
            pending.push("network_profile");
            None
        })
    };
    #[cfg(not(windows))]
    let profiles = None;

    let ai = state.assistant.overview();
    let knowledge: Vec<&PackView> = packs.iter().filter(|v| v.pack.category == Category::Knowledge).filter(on_hub).collect();
    let comaps: Vec<&PackView> = packs.iter().filter(|v| v.pack.id.starts_with(zaklon_core::maps::MAP_ID_PREFIX)).filter(on_hub).collect();
    let library_dir = state.downloads.library_dir();
    let disk = crate::downloads::system_info(library_dir);
    let assets = state.tiles.assets();

    Ok(Spec {
        app: App {
            version: crate::VERSION,
            build_date: BUILT_ON,
            commit: BUILT_FROM,
            mode: if cfg!(debug_assertions) { "development" } else { "release" },
            os: pc.os.clone(),
            os_build: pc.os_build.clone(),
            webview2: pc.webview2.clone(),
        },
        built_with: BuiltWith {
            rust: RUST_VERSION,
            tauri: TAURI_VERSION,
            sqlite: zaklon_core::db::sqlite_version(),
            llama_cpp: program("llama-cpp"),
            kiwix_serve: program("kiwix-tools"),
            libzim,
            comaps: zaklon_core::maps::COMAPS_APK_VERSION,
        },
        database: Database {
            sqlite: zaklon_core::db::sqlite_version(),
            size: state.db.size_on_disk(),
            migrations: state.db.migrations_done()?,
            last_backup: crate::backup::newest(&cfg).map(rfc3339),
        },
        assistant: AssistantSpec {
            engine: ai.engine,
            models: ai.models.iter().filter(|m| m.installed).map(Model::from).collect(),
            in_use: ai.selected.clone(),
            ram_total: ai.ram_total,
            recommended: ai.recommended.as_ref().and_then(|id| ai.models.iter().find(|m| &m.id == id)).map(Model::from),
            cpu: pc.cpu.clone(),
            cores: crate::machine::cores().map(|c| c.physical),
            threads: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
        },
        library: LibrarySpec {
            packs: knowledge.len(),
            size: knowledge.iter().map(size_of).sum(),
            folder: library_dir.display().to_string(),
            drive: crate::machine::drive_root(library_dir),
            drive_free: disk.disk_free,
            drive_total: disk.disk_total,
        },
        maps: MapsSpec {
            world: state.downloads.world_view().and_then(|w| w.installed.map(|build| WorldMap { build, size: w.installed_size })),
            overview: assets.is_some_and(|a| a.join(crate::tiles::OVERVIEW_FILE).is_file()),
            overview_build: assets.and_then(overview_build),
            comaps_app: zaklon_core::maps::COMAPS_APK_VERSION,
            comaps_app_on_hub: packs.iter().filter(|v| v.pack.id == zaklon_core::maps::COMAPS_APK_ID).any(|v| on_hub(&v)),
            comaps_maps: comaps.len(),
            comaps_size: comaps.iter().map(size_of).sum(),
        },
        network: NetworkSpec {
            addresses: crate::discovery::shown_ipv4_addresses().iter().map(|a| a.to_string()).collect(),
            port: cfg.port,
            profiles,
            phones: state.db.count_devices()?,
            fingerprint: state.identity.fingerprint.chars().take(16).collect(),
        },
        pending,
    })
}

/// RFC 3339 in UTC, to the second.
fn rfc3339(t: SystemTime) -> String {
    let t = time::OffsetDateTime::from(t);
    t.replace_nanosecond(0).unwrap_or(t).format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
}

/// The Protomaps build the world overview was cut from, as the map assets'
/// ATTRIBUTION.txt names it (scripts/fetch-map-assets.sh writes it).
fn overview_build(assets: &Path) -> Option<String> {
    build_named(&std::fs::read_to_string(assets.join("ATTRIBUTION.txt")).ok()?)
}

/// "... cut from the Protomaps basemap build 20260928." -> "20260928".
fn build_named(text: &str) -> Option<String> {
    let at = text.find("basemap build ")? + "basemap build ".len();
    let build: String = text[at..].chars().take_while(char::is_ascii_digit).collect();
    (build.len() == 8).then_some(build)
}

/// The libzim in what `kiwix-serve --version` prints ("+ libzim 9.4.0").
fn libzim_of(text: &str) -> Option<String> {
    text.lines()
        .map(|l| l.trim_start_matches(['+', ' ']))
        .find_map(|l| l.strip_prefix("libzim "))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty() && v.chars().all(|c| c.is_ascii_digit() || c == '.'))
}

/// What a program printed, run without a window. None when it did not
/// start or failed, or did not finish within `PROGRAM_TIMEOUT` (it is then ended).
fn run_program(exe: &Path, args: &[&str]) -> Option<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(exe);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    // Read on another thread, so a program that prints a lot cannot block on a full pipe.
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.read_to_end(&mut out);
        out
    });
    let deadline = Instant::now() + PROGRAM_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) => return None,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    Some(String::from_utf8_lossy(&reader.join().ok()?).into_owned())
}

/// A value that takes running a program to read: read on a thread of its
/// own the first time it is asked for, then kept while `key` (the program's
/// file) stays the same, for at most `max_age` if one is given. Asking never
/// waits.
struct Slow<T> {
    slot: Mutex<Slot<T>>,
}

struct Slot<T> {
    key: String,
    value: Option<T>,
    read_at: Option<Instant>,
    reading: bool,
}

impl<T: Clone + Send + 'static> Slow<T> {
    const fn new() -> Self {
        Self { slot: Mutex::new(Slot { key: String::new(), value: None, read_at: None, reading: false }) }
    }

    /// The value read for `key`, or None while it is read for the first
    /// time. One older than `max_age` is still given while it is read again.
    fn get(&'static self, key: &str, max_age: Option<Duration>, read: impl FnOnce() -> T + Send + 'static) -> Option<T> {
        let mut slot = self.slot.lock().unwrap_or_else(|p| p.into_inner());
        if slot.key != key {
            *slot = Slot { key: key.to_string(), value: None, read_at: None, reading: false };
        }
        let due = slot.read_at.is_none_or(|at| max_age.is_some_and(|m| at.elapsed() >= m));
        if due && !slot.reading {
            slot.reading = true;
            let key = key.to_string();
            std::thread::spawn(move || {
                let value = read();
                let mut slot = self.slot.lock().unwrap_or_else(|p| p.into_inner());
                // Kept only if it is still what is asked for.
                if slot.key == key {
                    *slot = Slot { key, value: Some(value), read_at: Some(Instant::now()), reading: false };
                }
            });
        }
        slot.value.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn a_slow_value_is_read_once_in_the_background() {
        static READS: AtomicUsize = AtomicUsize::new(0);
        static VALUE: Slow<usize> = Slow::new();
        let read = || {
            std::thread::sleep(Duration::from_millis(100));
            READS.fetch_add(1, Ordering::SeqCst) + 1
        };
        let settled = |key: &str, want: usize| {
            let deadline = Instant::now() + Duration::from_secs(10);
            while VALUE.get(key, None, read) != Some(want) {
                assert!(Instant::now() < deadline, "{key} was not read");
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        assert_eq!(VALUE.get("a", None, read), None, "being read, and asking does not wait");
        assert_eq!(VALUE.get("a", None, read), None);
        settled("a", 1);
        assert_eq!(READS.load(Ordering::SeqCst), 1, "read once");
        // Another copy of the program: read again.
        assert_eq!(VALUE.get("b", None, read), None);
        settled("b", 2);
        // Too old: the value read before, while it is read again.
        assert_eq!(VALUE.get("b", Some(Duration::ZERO), read), Some(2));
        settled("b", 3);
        assert_eq!(READS.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn versions_are_found_in_what_programs_print() {
        // kiwix-serve 3.8.1 --version, as it prints it.
        let kiwix = "kiwix-tools 3.8.1\n\nlibkiwix 14.1.1\n+ libzim 9.4.0\n+ libxapian 1.4.23\n+ libcurl 8.4.0\n\nlibzim 9.4.0\n+ libzstd 1.5.5\n";
        assert_eq!(libzim_of(kiwix).as_deref(), Some("9.4.0"));
        assert_eq!(libzim_of("kiwix-tools 3.8.1\n"), None);
        assert_eq!(libzim_of("+ libzim oops"), None);
        // The map assets' ATTRIBUTION.txt.
        let attribution = "overview.pmtiles\n  The world at zoom 0-5, cut from the Protomaps basemap build 20260928.\n";
        assert_eq!(build_named(attribution).as_deref(), Some("20260928"));
        assert_eq!(build_named("cut from the Protomaps basemap build 2026."), None);
        assert_eq!(build_named("no build named"), None);
    }

    #[test]
    fn the_build_is_known() {
        assert!(!RUST_VERSION.is_empty() && RUST_VERSION.split('.').count() == 3, "{RUST_VERSION}");
        assert!(TAURI_VERSION.starts_with("2."), "{TAURI_VERSION}");
        assert_eq!(BUILT_ON.len(), 10, "{BUILT_ON}");
        assert!(BUILT_FROM.is_empty() || (BUILT_FROM.len() <= 12 && BUILT_FROM.chars().all(|c| c.is_ascii_hexdigit())), "{BUILT_FROM}");
    }
}
