//! Desktop (Windows) behavior of the Zaklon app: the hub runs inside this
//! process (and is started again whenever it stops), the window can be
//! hidden to the tray while the hub keeps serving phones, the app starts
//! with Windows (on by default, see [`crate::autostart`]), and only one copy
//! runs at a time.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

use crate::autostart;

/// Passed by the Windows autostart entry: start hidden in the tray.
pub const MINIMIZED_ARG: &str = "--minimized";
const TRAY_ID: &str = "zaklon";

/// The folder of an installed copy (the installer puts `uninstall.exe` next
/// to the program).
pub fn install_dir() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    dir.join("uninstall.exe").is_file().then_some(dir)
}

/// Where the household's data lives.
/// - `ZAKLON_ROOT` if set (tests, a second hub on one machine);
/// - `<install folder>\data` for an installed copy (the folder chosen in the
///   installer), so program and data sit together;
/// - otherwise (a development build, or a copy that is not installed)
///   `%LOCALAPPDATA%\Zaklon-dev`. Never the installer's default folder
///   (`%LOCALAPPDATA%\Zaklon`), where an installed copy keeps its program
///   and its data.
pub fn data_root() -> PathBuf {
    if let Ok(p) = std::env::var("ZAKLON_ROOT") {
        return PathBuf::from(p);
    }
    if let Some(dir) = install_dir() {
        return dir.join("data");
    }
    match std::env::var_os("LOCALAPPDATA") {
        Some(p) => PathBuf::from(p).join("Zaklon-dev"),
        None => PathBuf::from("zaklon-data"),
    }
}

/// Log to `<root>/logs/app.log.<date>`, a month kept (the release build has
/// no console).
pub fn init_logging(root: &Path) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
    let (file, guard) = match zaklon_hub::log_file(root, "app.log") {
        Ok(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            (Some(fmt::layer().with_ansi(false).with_writer(writer)), Some(guard))
        }
        Err(e) => {
            eprintln!("no log file: {e:#}");
            (None, None)
        }
    };
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "zaklon_hub=info,zaklon_core=info,zaklon_app_lib=info".into());
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_ansi(false).with_writer(std::io::stdout))
        .with(file)
        .try_init()
        .ok()?;
    guard
}

// ---- the hub, kept running for as long as the app runs ---------------------------

/// While the hub has been failing for less than this, it is "starting" (its
/// ports or files may still be held by the copy that is ending during a
/// restart); after that it is "not running", and the tray and window say so.
const HUB_START_GRACE: Duration = Duration::from_secs(20);
/// Waits between attempts: from 1 s, doubling up to a minute.
const RETRY_FIRST: Duration = Duration::from_secs(1);
const RETRY_MAX: Duration = Duration::from_secs(60);
/// A run that lasted this long was healthy: the waits start over.
const HEALTHY_RUN: Duration = Duration::from_secs(60);

/// Where the hub serves the window's page, and which household it is. Known
/// once the hub has opened its data.
#[derive(Clone)]
struct HubAddr {
    port: u16,
    hub_id: String,
}

static HUB_ADDR: Mutex<Option<HubAddr>> = Mutex::new(None);
/// The hub has answered as itself on its local port since it last started.
static HUB_UP: AtomicBool = AtomicBool::new(false);
/// Since when the hub has not been running (None while it runs).
static HUB_DOWN_SINCE: Mutex<Option<Instant>> = Mutex::new(None);
static ROOT: OnceLock<PathBuf> = OnceLock::new();

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// The port of the hub's page for the window: the one the hub uses, or
/// before it has opened, the one it will use.
pub fn local_port() -> u16 {
    if let Some(addr) = lock(&HUB_ADDR).as_ref() {
        return addr.port;
    }
    std::env::var("ZAKLON_LOCAL_PORT")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(zaklon_hub::LOCAL_PORT)
}

/// Run the hub on its own thread for as long as the app runs. When it
/// stops, or cannot start (its ports held, its folder unreadable), it is
/// tried again: after 1 s, then waiting longer each time, up to a minute.
pub fn start_hub(app: AppHandle, root: PathBuf) {
    let _ = ROOT.set(root.clone());
    *lock(&HUB_DOWN_SINCE) = Some(Instant::now());
    std::thread::Builder::new()
        .name("zaklon-hub".into())
        .spawn(move || {
            let mut attempt = 0u32;
            let mut wait = RETRY_FIRST;
            loop {
                attempt += 1;
                let started = Instant::now();
                // A fresh runtime per attempt: whatever a failed attempt
                // started (downloads, discovery, ...) ends with it.
                let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime");
                let result = rt.block_on(run_hub(&app, &root));
                rt.shutdown_timeout(Duration::from_secs(2));
                HUB_UP.store(false, Ordering::SeqCst);
                let down_for = lock(&HUB_DOWN_SINCE).get_or_insert_with(Instant::now).elapsed();
                if started.elapsed() >= HEALTHY_RUN {
                    wait = RETRY_FIRST;
                }
                let why = match result {
                    Ok(()) => "it stopped".to_string(),
                    Err(e) => format!("{e:#}"),
                };
                if down_for < HUB_START_GRACE {
                    tracing::warn!("hub could not start (attempt {attempt}), trying again in {} s: {why}", wait.as_secs());
                } else {
                    tracing::error!("hub is not running (attempt {attempt}), trying again in {} s: {why}", wait.as_secs());
                }
                update_tray(&app);
                std::thread::sleep(wait);
                wait = (wait * 2).min(RETRY_MAX);
            }
        })
        .expect("spawn hub thread");
}

/// One run of the hub: open it, and serve until it stops.
async fn run_hub(app: &AppHandle, root: &Path) -> anyhow::Result<()> {
    let hub = zaklon_hub::Hub::open(root).map_err(|e| e.context(format!("opening the hub at {}", root.display())))?;
    let cfg = hub.state().config();
    let addr = HubAddr { port: cfg.local_port, hub_id: cfg.hub_id.clone() };
    *lock(&HUB_ADDR) = Some(addr.clone());
    allow_window_port(app, addr.port);

    let run = hub.run();
    tokio::pin!(run);
    let ready = async {
        loop {
            let a = addr.clone();
            if tokio::task::spawn_blocking(move || hub_answers(&a)).await.unwrap_or(false) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    };
    tokio::select! {
        r = &mut run => return r,
        () = ready => {}
    }
    HUB_UP.store(true, Ordering::SeqCst);
    *lock(&HUB_DOWN_SINCE) = None;
    tracing::info!("hub is serving");
    update_tray(app);
    run.await
}

/// The hub answers on its local port as itself (the same hub id), not some
/// other program that holds the port.
fn hub_answers(addr: &HubAddr) -> bool {
    use std::io::{Read, Write};
    let socket = std::net::SocketAddr::from(([127, 0, 0, 1], addr.port));
    let Ok(mut stream) = std::net::TcpStream::connect_timeout(&socket, Duration::from_millis(300)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let request = format!("GET /api/status HTTP/1.0\r\nHost: 127.0.0.1:{}\r\n\r\n", addr.port);
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut reply = Vec::new();
    let _ = stream.take(64 * 1024).read_to_end(&mut reply);
    let reply = String::from_utf8_lossy(&reply);
    let Some((_, body)) = reply.split_once("\r\n\r\n") else { return false };
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("hub_id").and_then(|id| id.as_str()).map(|id| id == addr.hub_id))
        .unwrap_or(false)
}

/// Not running for longer than a restart takes.
fn hub_down() -> bool {
    !HUB_UP.load(Ordering::SeqCst) && lock(&HUB_DOWN_SINCE).is_some_and(|t| t.elapsed() >= HUB_START_GRACE)
}

fn update_tray(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let _ = tray.set_tooltip(Some(tray_tooltip()));
}

fn tray_tooltip() -> &'static str {
    if hub_down() {
        labels().not_running_short
    } else {
        "Zaklon"
    }
}

/// The window's permissions belong to the hub's origin. `desktop-hub.json`
/// covers the usual port; a hub on another port (`ZAKLON_LOCAL_PORT`, a
/// second hub on one machine) gets the same permissions for its own origin.
fn allow_window_port(app: &AppHandle, port: u16) {
    static GRANTED: Mutex<Vec<u16>> = Mutex::new(Vec::new());
    if port == zaklon_hub::LOCAL_PORT {
        return;
    }
    let mut granted = lock(&GRANTED);
    if granted.contains(&port) {
        return;
    }
    let web = |url: &str| serde_json::json!({ "url": url });
    let capability = tauri::ipc::CapabilityBuilder::new(format!("desktop-hub-{port}"))
        .remote(format!("http://127.0.0.1:{port}"))
        .local(false)
        .window("main")
        .permission("core:default")
        .permission_scoped("opener:allow-open-url", vec![web("https://*"), web("http://*")], Vec::new())
        .permission("allow-app-mode")
        .permission("allow-app-restart");
    match app.add_capability(capability) {
        Ok(()) => granted.push(port),
        Err(e) => tracing::warn!("the window on port {port} gets no app permissions: {e}"),
    }
}

// ---- restarting ----------------------------------------------------------------------

/// Set for the copy started by [`restart`]: it opens its window even if the
/// previous copy was started hidden with Windows.
const RESTARTED_ENV: &str = "ZAKLON_RESTARTED";
/// The process id of the copy that is ending; the new copy waits for it.
const WAIT_PID_ENV: &str = "ZAKLON_WAIT_PID";
/// How long a new copy waits for the one that is ending.
const PREVIOUS_COPY_WAIT: Duration = Duration::from_secs(60);
/// Then how long it waits for the one-copy guard to be free.
const GUARD_WAIT: Duration = Duration::from_secs(30);
/// tauri-plugin-single-instance's guard: the identifier from
/// `tauri.conf.json`, then "-sim".
#[cfg(windows)]
const SINGLE_INSTANCE_GUARD: &str = "com.zaklon.app-sim";

/// Start a fresh copy of Zaklon and end this one (after choosing a backup to
/// restore, which is swapped in when the hub opens). The new copy waits for
/// this one to be gone, so the one-copy guard and the hub's ports are free.
pub fn restart(app: &AppHandle) {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => {
            tracing::error!("cannot restart, the program path is unknown: {e}");
            return;
        }
    };
    let args: Vec<String> = std::env::args().skip(1).filter(|a| a != MINIMIZED_ARG).collect();
    match std::process::Command::new(&exe)
        .args(&args)
        .env(RESTARTED_ENV, "1")
        .env(WAIT_PID_ENV, std::process::id().to_string())
        .spawn()
    {
        Ok(child) => {
            tracing::info!("restarting: started {} (pid {}), ending this copy", exe.display(), child.id());
            app.exit(0);
        }
        Err(e) => tracing::error!("cannot restart, starting {} failed: {e}", exe.display()),
    }
}

/// Called first thing at start: if this copy was started by [`restart`],
/// wait until the previous copy has ended and let go of the one-copy guard.
/// Otherwise a slow exit (a large window closing, an antivirus scan) would
/// make this copy hand over to the ending one and quit, leaving nothing
/// running.
pub fn wait_for_previous_copy() {
    let Some(pid) = std::env::var(WAIT_PID_ENV).ok().and_then(|v| v.trim().parse::<u32>().ok()) else {
        return;
    };
    std::env::remove_var(WAIT_PID_ENV);
    if pid == std::process::id() {
        return;
    }
    let started = Instant::now();
    let ended = wait_for_pid(pid, PREVIOUS_COPY_WAIT);
    let guard_free = wait_for_guard(GUARD_WAIT);
    // Logging is not set up yet; keep the outcome for later.
    PREVIOUS_WAIT.get_or_init(|| (pid, ended && guard_free, started.elapsed()));
}

static PREVIOUS_WAIT: OnceLock<(u32, bool, Duration)> = OnceLock::new();

/// Log what [`wait_for_previous_copy`] saw, once logging is set up.
pub fn log_restart() {
    if let Some((pid, ended, took)) = PREVIOUS_WAIT.get() {
        if *ended {
            tracing::info!("restarted; the previous copy (pid {pid}) ended after {} ms", took.as_millis());
        } else {
            tracing::warn!("restarted; the previous copy (pid {pid}) was still running after {} ms", took.as_millis());
        }
    }
}

/// True once the process has ended (or cannot be found), false on timeout.
#[cfg(windows)]
fn wait_for_pid(pid: u32, timeout: Duration) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    // SAFETY: plain Win32 calls; the handle is checked and closed here.
    unsafe {
        let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if handle.is_null() {
            return true; // already gone
        }
        let r = WaitForSingleObject(handle, timeout.as_millis() as u32);
        CloseHandle(handle);
        r != WAIT_TIMEOUT
    }
}

#[cfg(not(windows))]
fn wait_for_pid(pid: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Path::new(&format!("/proc/{pid}")).exists() {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    true
}

/// True once no copy holds the one-copy guard, false on timeout.
#[cfg(windows)]
fn wait_for_guard(timeout: Duration) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE};
    let name: Vec<u16> = SINGLE_INSTANCE_GUARD.encode_utf16().chain(Some(0)).collect();
    let deadline = Instant::now() + timeout;
    loop {
        // SAFETY: a NUL-terminated name; an opened handle is closed at once.
        let held = unsafe {
            let handle = OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, 0, name.as_ptr());
            if handle.is_null() {
                false
            } else {
                CloseHandle(handle);
                true
            }
        };
        if !held {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[cfg(not(windows))]
fn wait_for_guard(_timeout: Duration) -> bool {
    true
}

// ---- tray and window -------------------------------------------------------------------

/// Tray, menu and waiting-page texts follow the language chosen in the app.
struct Labels {
    open: &'static str,
    autostart: &'static str,
    quit: &'static str,
    starting: &'static str,
    not_running: &'static str,
    not_running_short: &'static str,
}

fn labels() -> Labels {
    let lang = ROOT
        .get()
        .and_then(|root| std::fs::read_to_string(root.join("household").join("hub.json")).ok())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("language").and_then(|l| l.as_str()).map(String::from))
        .unwrap_or_else(|| "en".into());
    if lang == "sr" {
        Labels {
            open: "Otvori Zaklon",
            autostart: "Pokreni sa Windowsom",
            quit: "Ugasi Zaklon",
            starting: "Zaklon se pokreće…",
            not_running: "Zaklon trenutno ne radi. Sam pokušava ponovo; razlog je zapisan u log fajlovima u folderu:",
            not_running_short: "Zaklon ne radi (razlog je u log fajlovima)",
        }
    } else {
        Labels {
            open: "Open Zaklon",
            autostart: "Start with Windows",
            quit: "Quit Zaklon",
            starting: "Zaklon is starting…",
            not_running: "Zaklon is not running right now. It keeps trying by itself; the reason is written in the log files in this folder:",
            not_running_short: "Zaklon is not running (the reason is in the log files)",
        }
    }
}

/// The app's own scheme for the page the window shows while the hub is not
/// answering yet (WebView2 loads it from `http://zaklon.localhost/`).
pub const WAITING_SCHEME: &str = "zaklon";
const WAITING_URL: &str = "zaklon://localhost/";
const WAITING_HOST: &str = "zaklon.localhost";

/// The waiting page: "starting", or "not running" with the log folder. It
/// reloads itself to stay current; the window switches to the hub's page as
/// soon as the hub answers.
pub fn waiting_page() -> tauri::http::Response<Vec<u8>> {
    let l = labels();
    let message = if hub_down() {
        let logs = ROOT.get().map(|r| r.join("logs").display().to_string()).unwrap_or_default();
        format!("<p>{}</p><p><code>{}</code></p>", escape_html(l.not_running), escape_html(&logs))
    } else {
        format!("<p>{}</p>", escape_html(l.starting))
    };
    let html = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta http-equiv=\"refresh\" content=\"3\"><title>Zaklon</title>\
         <style>body{{background:#0b0d10;color:#e8e6e3;font:16px/1.5 system-ui,sans-serif;display:flex;flex-direction:column;\
         justify-content:center;align-items:center;height:100vh;margin:0;padding:0 24px;box-sizing:border-box;text-align:center}}\
         code{{color:#f0b35a;word-break:break-all}}</style></head><body>{message}</body></html>"
    );
    tauri::http::Response::builder()
        .header("content-type", "text/html; charset=utf-8")
        .header("content-security-policy", "default-src 'none'; style-src 'unsafe-inline'")
        .body(html.into_bytes())
        .unwrap_or_default()
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The window only ever shows the hub's own pages and the waiting page;
/// anything else (a link that would leave, another program that took the
/// hub's port) is not loaded in it.
fn may_show(url: &tauri::Url) -> bool {
    match url.scheme() {
        "http" if url.host_str() == Some("127.0.0.1") => url.port() == Some(local_port()),
        "http" | "https" => url.host_str() == Some(WAITING_HOST),
        scheme => scheme == WAITING_SCHEME,
    }
}

/// The hub's page, when the hub answers.
fn hub_url() -> Option<tauri::Url> {
    if !HUB_UP.load(Ordering::SeqCst) {
        return None;
    }
    format!("http://127.0.0.1:{}/", local_port()).parse().ok()
}

/// Show the window, making it again if it was closed.
pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    if let Err(e) = open_window(app, true) {
        tracing::error!("could not open the window: {e}");
    }
}

/// Everything the desktop app sets up at start.
pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    autostart::ensure();
    // Started with Windows: only the tray icon; the window (and the memory a
    // web view takes) comes when someone opens it. A restart always shows it.
    let start_hidden =
        std::env::args().any(|a| a == MINIMIZED_ARG) && std::env::var_os(RESTARTED_ENV).is_none();
    build_tray(app)?;
    if !start_hidden {
        open_window(app.handle(), true)?;
    }
    Ok(())
}

/// Counts windows made, so a waiting thread knows when its window is gone.
static WINDOW_GENERATION: AtomicU64 = AtomicU64::new(0);

/// The window shows the interface served by the hub itself, so the page and
/// the API share one origin. Until the hub answers, it shows the waiting
/// page, and a thread switches it over; nothing waits on the main thread.
/// Closing the window really closes it (a hidden web view still holds a few
/// hundred MB); the hub keeps running, and the tray opens a new window.
fn open_window(app: &AppHandle, visible: bool) -> Result<(), Box<dyn std::error::Error>> {
    let generation = WINDOW_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let (url, waiting) = match hub_url() {
        Some(url) => (WebviewUrl::External(url), false),
        None => (WebviewUrl::CustomProtocol(WAITING_URL.parse()?), true),
    };
    let window = WebviewWindowBuilder::new(app, "main", url)
        .title("Zaklon")
        .inner_size(1200.0, 800.0)
        .min_inner_size(900.0, 600.0)
        .background_color(tauri::window::Color(11, 13, 18, 255)) // --bg, #0B0D12
        .visible(visible)
        .on_navigation(|url| {
            let ok = may_show(url);
            if !ok {
                tracing::warn!("the window does not load {url}");
            }
            ok
        })
        .build()?;
    window.on_window_event(|event| {
        if let WindowEvent::Destroyed = event {
            tracing::info!("window closed; the hub keeps running in the tray");
        }
    });
    if waiting {
        let app = app.clone();
        std::thread::spawn(move || loop {
            if WINDOW_GENERATION.load(Ordering::SeqCst) != generation {
                return;
            }
            let Some(window) = app.get_webview_window("main") else { return };
            if let Some(url) = hub_url() {
                if let Err(e) = window.navigate(url) {
                    tracing::warn!("could not show the hub's page: {e}");
                }
                return;
            }
            std::thread::sleep(Duration::from_millis(300));
        });
    }
    Ok(())
}

fn build_tray(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let l = labels();
    let open = MenuItem::with_id(app, "open", l.open, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", l.quit, true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    // Only an installed copy offers "Start with Windows" (see autostart).
    let autostart_item = if autostart::managed() {
        Some(CheckMenuItem::with_id(app, "autostart", l.autostart, true, autostart::is_enabled(), None::<&str>)?)
    } else {
        None
    };
    let menu = Menu::with_items(app, &[&open, &sep, &quit])?;
    if let Some(item) = &autostart_item {
        menu.insert(item, 1)?;
    }

    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip(tray_tooltip())
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "autostart" => {
                let on = !autostart::is_enabled();
                match autostart::set(on) {
                    Ok(()) => tracing::info!("start with Windows turned {}", if on { "on" } else { "off" }),
                    Err(e) => tracing::warn!("changing start with Windows failed: {e}"),
                }
                if let Some(item) = &autostart_item {
                    let _ = item.set_checked(autostart::is_enabled());
                }
            }
            "quit" => {
                tracing::info!("quit from the tray menu");
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_shows_only_the_hub_and_the_waiting_page() {
        let port = local_port();
        let ok = |u: &str| may_show(&u.parse().unwrap());
        assert!(ok(&format!("http://127.0.0.1:{port}/")));
        assert!(ok(&format!("http://127.0.0.1:{port}/library?x=1#top")));
        assert!(ok("http://zaklon.localhost/"));
        assert!(ok("zaklon://localhost/"));
        assert!(!ok(&format!("http://127.0.0.1:{}/", port.wrapping_add(1))));
        assert!(!ok(&format!("http://localhost:{port}/")));
        assert!(!ok("https://example.com/"));
        assert!(!ok("file:///C:/Windows/"));
        assert!(!ok("http://evil.zaklon.localhost.example/"));
    }

    #[test]
    fn waiting_page_escapes_the_folder() {
        assert_eq!(escape_html("C:\\a&b<c>\""), "C:\\a&amp;b&lt;c&gt;&quot;");
    }
}
