//! Desktop (Windows) behavior of the Zaklon app: the hub runs inside this
//! process, the window can be hidden to the tray while the hub keeps serving
//! phones, the app starts with Windows (on by default), and only one copy
//! runs at a time.

use std::path::{Path, PathBuf};

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_autostart::ManagerExt;

/// Passed by the Windows autostart entry: start hidden in the tray.
pub const MINIMIZED_ARG: &str = "--minimized";

/// Where the household's data lives.
/// - `ZAKLON_ROOT` if set (tests, a second hub on one machine);
/// - `<install folder>\data` for an installed copy (the folder chosen in the
///   installer), so program and data sit together as agreed;
/// - otherwise the default (`%LOCALAPPDATA%\Zaklon`), e.g. for development builds.
pub fn data_root() -> PathBuf {
    if let Ok(p) = std::env::var("ZAKLON_ROOT") {
        return PathBuf::from(p);
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        if dir.join("uninstall.exe").is_file() {
            return dir.join("data");
        }
    }
    zaklon_hub::default_root()
}

/// Log to `<root>/logs/app.log` (the release build has no console).
pub fn init_logging(root: &Path) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
    let _ = std::fs::create_dir_all(root.join("logs"));
    let file = tracing_appender::rolling::daily(root.join("logs"), "app.log");
    let (writer, guard) = tracing_appender::non_blocking(file);
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "zaklon_hub=info,zaklon_core=info,zaklon_app_lib=info".into());
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_ansi(false).with_writer(std::io::stdout))
        .with(fmt::layer().with_ansi(false).with_writer(writer))
        .try_init()
        .ok()?;
    Some(guard)
}

pub fn start_hub(root: PathBuf) {
    std::thread::Builder::new()
        .name("zaklon-hub".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime");
            rt.block_on(async {
                match zaklon_hub::Hub::open(&root) {
                    Ok(hub) => {
                        if let Err(e) = hub.run().await {
                            tracing::error!("hub stopped: {e:#}");
                        }
                    }
                    Err(e) => tracing::error!("hub failed to open {}: {e:#}", root.display()),
                }
            });
        })
        .expect("spawn hub thread");
}

/// Tray and menu texts follow the language chosen in the app.
struct Labels {
    open: &'static str,
    autostart: &'static str,
    quit: &'static str,
}

fn labels(root: &Path) -> Labels {
    let lang = std::fs::read_to_string(root.join("household").join("hub.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("language").and_then(|l| l.as_str()).map(String::from))
        .unwrap_or_else(|| "en".into());
    if lang == "sr" {
        Labels { open: "Otvori Zaklon", autostart: "Pokreni sa Windowsom", quit: "Ugasi Zaklon" }
    } else {
        Labels { open: "Open Zaklon", autostart: "Start with Windows", quit: "Quit Zaklon" }
    }
}

/// Show the window, making it again if it was closed.
pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    if let Err(e) = open_hub_window(app, true) {
        tracing::error!("could not open the window: {e}");
    }
}

/// Everything the desktop app sets up at start.
pub fn setup(app: &mut tauri::App, root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    // Start with Windows: on by default, set once on the first run; after
    // that the person's choice in the tray menu is kept.
    let marker = root.join("household").join(".autostart-configured");
    if !marker.exists() {
        if let Err(e) = app.autolaunch().enable() {
            tracing::warn!("could not enable start with Windows: {e}");
        }
        let _ = std::fs::create_dir_all(root.join("household"));
        let _ = std::fs::write(&marker, b"1");
    }

    // Started with Windows: only the tray icon; the window (and the memory a
    // web view takes) comes when someone opens it.
    let start_hidden = std::env::args().any(|a| a == MINIMIZED_ARG);
    if !start_hidden {
        open_hub_window(app.handle(), true)?;
    }
    build_tray(app, root)?;
    Ok(())
}

/// The window shows the interface served by the hub itself, so the page and
/// the API share one origin. Closing it really closes it (a hidden web view
/// still holds a few hundred MB); the hub keeps running, and the tray opens
/// a new window.
fn open_hub_window(app: &AppHandle, visible: bool) -> Result<(), Box<dyn std::error::Error>> {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], local_port()));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(300)).is_err() {
        if std::time::Instant::now() > deadline {
            tracing::error!("hub did not start listening on {addr}");
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    let url: tauri::Url = format!("http://{addr}/").parse()?;
    let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
        .title("Zaklon")
        .inner_size(1200.0, 800.0)
        .min_inner_size(900.0, 600.0)
        .background_color(tauri::window::Color(11, 13, 16, 255))
        .visible(visible)
        .build()?;
    window.on_window_event(|event| {
        if let WindowEvent::Destroyed = event {
            tracing::info!("window closed; the hub keeps running in the tray");
        }
    });
    Ok(())
}

fn local_port() -> u16 {
    std::env::var("ZAKLON_LOCAL_PORT")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(zaklon_hub::LOCAL_PORT)
}

fn build_tray(app: &mut tauri::App, root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let l = labels(root);
    let autostart_on = app.autolaunch().is_enabled().unwrap_or(false);
    let open = MenuItem::with_id(app, "open", l.open, true, None::<&str>)?;
    let autostart = CheckMenuItem::with_id(app, "autostart", l.autostart, true, autostart_on, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", l.quit, true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &autostart, &sep, &quit])?;

    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?;
    let autostart_item = autostart.clone();
    TrayIconBuilder::with_id("zaklon")
        .icon(icon)
        .tooltip("Zaklon")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "autostart" => {
                let launcher = app.autolaunch();
                let now_on = launcher.is_enabled().unwrap_or(false);
                let result = if now_on { launcher.disable() } else { launcher.enable() };
                if let Err(e) = result {
                    tracing::warn!("changing start with Windows failed: {e}");
                }
                let _ = autostart_item.set_checked(launcher.is_enabled().unwrap_or(false));
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
