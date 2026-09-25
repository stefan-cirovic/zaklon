//! Tauri entry point. On desktop the hub runs inside this process and the
//! window talks to it over 127.0.0.1; on Android the app is a client of a
//! hub on the network.

use serde::Serialize;

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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "zaklon_hub=info,zaklon_core=info,zaklon_app_lib=info".into()),
        )
        .try_init();

    tauri::Builder::default()
        .setup(|_app| {
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            start_hub();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![app_mode])
        .run(tauri::generate_context!())
        .expect("error while running Zaklon");
}
