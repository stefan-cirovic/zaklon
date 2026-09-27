//! Tauri entry point. On desktop the hub runs inside this process and the
//! window talks to it over 127.0.0.1; on Android the app is a client of a
//! hub on the network.

#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod autostart;
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

/// Start the app again (the laptop, after choosing a backup to restore).
#[tauri::command]
fn app_restart(app: tauri::AppHandle) {
    // Desktop: a fresh copy that waits for this one to end and always shows
    // its window (even if this copy was started hidden with Windows).
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    desktop::restart(&app);
    #[cfg(any(target_os = "android", target_os = "ios"))]
    app.restart();
}

#[tauri::command]
fn app_mode() -> AppMode {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        AppMode {
            mode: "hub",
            // The port the window's page really comes from (it can be changed for a second hub).
            api_base: Some(format!("http://127.0.0.1:{}", desktop::local_port())),
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

/// Pair with a hub found on the network ("Find hubs"): the phone checks the
/// hub with the code before it sends the password (see `ClientState::pair_found`).
#[tauri::command]
async fn client_pair_found(
    state: tauri::State<'_, Arc<ClientState>>,
    host: String,
    port: u16,
    code: String,
    password: String,
    device_name: String,
) -> Result<LinkSummary, String> {
    state.pair_found(host, port, code, password, device_name).await
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

/// Phones: copy an app (the CoMaps map app) from the hub over the pinned
/// connection, check it, and return a local address to install it from.
#[tauri::command]
async fn client_fetch_app(state: tauri::State<'_, Arc<ClientState>>, name: String) -> Result<String, String> {
    state.inner().fetch_app(&name).await
}

/// Phones: shopping list changes waiting for the hub ("outbox", "parked"),
/// kept in files that are on the disk before the change counts as saved.
#[tauri::command]
fn outbox_read(state: tauri::State<'_, Arc<ClientState>>, name: String) -> Result<Option<String>, String> {
    state.read_store(&name)
}

#[tauri::command]
fn outbox_write(state: tauri::State<'_, Arc<ClientState>>, name: String, data: String) -> Result<(), String> {
    state.write_store(&name, &data)
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
    // After a restart, before the one-copy guard: wait for the old copy to end.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    desktop::wait_for_previous_copy();
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let root = desktop::data_root();
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let _log_guard = desktop::init_logging(&root);
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    desktop::log_restart();
    #[cfg(any(target_os = "android", target_os = "ios"))]
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "zaklon_app_lib=info".into()),
        )
        .try_init();

    let builder = tauri::Builder::default().plugin(tauri_plugin_opener::init());
    #[cfg(any(target_os = "android", target_os = "ios"))]
    let builder = builder.plugin(tauri_plugin_barcode_scanner::init());
    // Desktop: one copy only (a second start just shows the window), and the
    // page the window shows until the hub answers.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let builder = builder
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| desktop::show_main(app)))
        .register_uri_scheme_protocol(desktop::WAITING_SCHEME, |_ctx, _request| desktop::waiting_page());

    builder
        .setup(move |app| {
            let dir = app.path().app_data_dir().unwrap_or_else(|_| std::env::temp_dir().join("zaklon"));
            let cache = app.path().app_cache_dir().unwrap_or_else(|_| dir.join("cache"));
            app.manage(Arc::new(local_ai::LocalAi::new(&dir)));
            app.manage(Arc::new(ClientState::load(dir, cache)));
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            {
                desktop::start_hub(app.handle().clone(), root.clone());
                desktop::setup(app)?;
            }
            #[cfg(any(target_os = "android", target_os = "ios"))]
            {
                // The page's background color, so the web view never shows white while it loads.
                tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::App("index.html".into()))
                    .background_color(tauri::window::Color(11, 13, 18, 255))
                    .build()?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_mode,
            client_state,
            client_pair,
            client_pair_found,
            client_request,
            client_forget,
            client_discover,
            client_content_base,
            client_fetch_app,
            outbox_read,
            outbox_write,
            local_ai_status,
            local_ai_copy,
            local_ai_start,
            local_ai_stop,
            local_ai_delete,
            local_ai_ask,
            app_restart
        ])
        .build(tauri::generate_context!())
        .expect("error while building Zaklon")
        .run(|_app, _event| {
            // Desktop: closing the window does not end the app; the hub keeps
            // serving phones until "Quit" in the tray (which exits with a code).
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            if let tauri::RunEvent::ExitRequested { api, code: None, .. } = _event {
                api.prevent_exit();
            }
        });
}
