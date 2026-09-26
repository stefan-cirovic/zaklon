//! Tauri entry point. On desktop the hub runs inside this process and the
//! window talks to it over 127.0.0.1; on Android the app is a client of a
//! hub on the network.

mod client;

use std::sync::Arc;

use serde::Serialize;
use tauri::Manager;

use client::{ClientResponse, ClientState, DiscoveredHub, LinkSummary, PairPayload};

#[derive(Serialize)]
struct AppMode {
    /// "hub" on the laptop, "client" on phones.
    mode: &'static str,
    /// Where the interface should send API calls on desktop.
    api_base: Option<String>,
    platform: &'static str,
    version: &'static str,
}

#[tauri::command]
fn app_mode() -> AppMode {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        AppMode {
            mode: "hub",
            api_base: Some(format!("http://127.0.0.1:{}", zaklon_hub::LOCAL_PORT)),
            platform: std::env::consts::OS,
            version: env!("CARGO_PKG_VERSION"),
        }
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        AppMode { mode: "client", api_base: None, platform: std::env::consts::OS, version: env!("CARGO_PKG_VERSION") }
    }
}

#[tauri::command]
fn client_state(state: tauri::State<'_, Arc<ClientState>>) -> LinkSummary {
    state.summary()
}

#[tauri::command]
async fn client_pair(
    state: tauri::State<'_, Arc<ClientState>>,
    payload: PairPayload,
    password: String,
    device_name: String,
) -> Result<LinkSummary, String> {
    state.pair(payload, password, device_name).await
}

#[tauri::command]
async fn client_request(
    state: tauri::State<'_, Arc<ClientState>>,
    method: String,
    path: String,
    body: Option<String>,
) -> Result<ClientResponse, String> {
    state.request(method, path, body).await
}

#[tauri::command]
fn client_forget(state: tauri::State<'_, Arc<ClientState>>) -> Result<(), String> {
    state.forget()
}

/// Base URL for library articles on phones (the loopback content proxy).
#[tauri::command]
async fn client_content_base(state: tauri::State<'_, Arc<ClientState>>) -> Result<String, String> {
    state.inner().content_base().await
}

#[tauri::command]
async fn client_discover() -> Result<Vec<DiscoveredHub>, String> {
    client::discover().await
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn start_hub() {
    std::thread::Builder::new()
        .name("zaklon-hub".into())
        .spawn(|| {
            let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime");
            rt.block_on(async {
                let root = zaklon_hub::default_root();
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

/// The desktop window shows the interface served by the hub itself, so the
/// page and the API share one origin (no cross-origin requests at all).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn open_hub_window(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], zaklon_hub::LOCAL_PORT));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(300)).is_err() {
        if std::time::Instant::now() > deadline {
            tracing::error!("hub did not start listening on {addr}");
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    let url: tauri::Url = format!("http://{addr}/").parse()?;
    tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::External(url))
        .title("Zaklon")
        .inner_size(1200.0, 800.0)
        .min_inner_size(900.0, 600.0)
        .background_color(tauri::window::Color(11, 13, 16, 255))
        .build()?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "zaklon_hub=info,zaklon_core=info,zaklon_app_lib=info".into()),
        )
        .try_init();

    let builder = tauri::Builder::default();
    #[cfg(any(target_os = "android", target_os = "ios"))]
    let builder = builder.plugin(tauri_plugin_barcode_scanner::init());

    builder
        .setup(|app| {
            let dir = app.path().app_data_dir().unwrap_or_else(|_| std::env::temp_dir().join("zaklon"));
            app.manage(Arc::new(ClientState::load(dir)));
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            {
                start_hub();
                open_hub_window(app)?;
            }
            #[cfg(any(target_os = "android", target_os = "ios"))]
            {
                tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::App("index.html".into())).build()?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_mode,
            client_state,
            client_pair,
            client_request,
            client_forget,
            client_discover,
            client_content_base
        ])
        .run(tauri::generate_context!())
        .expect("error while running Zaklon");
}
