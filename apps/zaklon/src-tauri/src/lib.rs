//! Tauri entry point. On desktop the hub runs inside this process and the
//! window talks to it over 127.0.0.1; on Android the app is a client of a
//! hub on the network.

mod client;
mod local_ai;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod desktop;

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
async fn client_forget(state: tauri::State<'_, Arc<ClientState>>) -> Result<(), String> {
    state.forget().await
}

/// Base URL for library articles on phones (the loopback content proxy).
#[tauri::command]
async fn client_content_base(state: tauri::State<'_, Arc<ClientState>>) -> Result<String, String> {
    state.inner().content_base().await
}

// ---- on-device AI (phones; on desktop the engine is simply absent) -------------

#[tauri::command]
fn local_ai_status(ai: tauri::State<'_, Arc<local_ai::LocalAi>>) -> local_ai::Status {
    ai.status()
}

#[tauri::command]
fn local_ai_copy(
    ai: tauri::State<'_, Arc<local_ai::LocalAi>>,
    client: tauri::State<'_, Arc<ClientState>>,
    model_id: String,
    file: String,
) -> Result<(), String> {
    ai.start_copy(client.inner().clone(), model_id, file)
}

#[tauri::command]
async fn local_ai_start(ai: tauri::State<'_, Arc<local_ai::LocalAi>>, file: String) -> Result<(), String> {
    let ai = ai.inner().clone();
    ai.start(&file).await
}

#[tauri::command]
fn local_ai_stop(ai: tauri::State<'_, Arc<local_ai::LocalAi>>) {
    ai.stop()
}

#[tauri::command]
fn local_ai_delete(ai: tauri::State<'_, Arc<local_ai::LocalAi>>, file: String) -> Result<(), String> {
    ai.delete_model(&file)
}

#[tauri::command]
async fn local_ai_ask(
    ai: tauri::State<'_, Arc<local_ai::LocalAi>>,
    prompt: String,
    language: String,
) -> Result<local_ai::Answer, String> {
    let ai = ai.inner().clone();
    ai.ask(&prompt, &language).await
}

#[tauri::command]
async fn client_discover() -> Result<Vec<DiscoveredHub>, String> {
    client::discover().await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let root = desktop::data_root();
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let _log_guard = desktop::init_logging(&root);
    #[cfg(any(target_os = "android", target_os = "ios"))]
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "zaklon_app_lib=info".into()),
        )
        .try_init();

    let builder = tauri::Builder::default();
    #[cfg(any(target_os = "android", target_os = "ios"))]
    let builder = builder.plugin(tauri_plugin_barcode_scanner::init());
    // Desktop: one copy only (a second start just shows the window), and start with Windows.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let builder = builder
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| desktop::show_main(app)))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![desktop::MINIMIZED_ARG]),
        ));

    builder
        .setup(move |app| {
            let dir = app.path().app_data_dir().unwrap_or_else(|_| std::env::temp_dir().join("zaklon"));
            app.manage(Arc::new(local_ai::LocalAi::new(&dir)));
            app.manage(Arc::new(ClientState::load(dir)));
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            {
                desktop::start_hub(root.clone());
                desktop::setup(app, &root)?;
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
            client_content_base,
            local_ai_status,
            local_ai_copy,
            local_ai_start,
            local_ai_stop,
            local_ai_delete,
            local_ai_ask
        ])
        .run(tauri::generate_context!())
        .expect("error while running Zaklon");
}
