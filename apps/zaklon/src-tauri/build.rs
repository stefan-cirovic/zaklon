/// Every command the interface may call. Listing them gives each one an
/// `allow-<command>` permission; the capabilities grant them per window and
/// origin (the desktop window loads the hub's own page from 127.0.0.1, which
/// Tauri treats as a remote origin that needs explicit permissions).
const COMMANDS: &[&str] = &[
    "app_mode",
    "app_restart",
    "client_state",
    "client_pair",
    "client_request",
    "client_forget",
    "client_discover",
    "client_content_base",
    "local_ai_status",
    "local_ai_copy",
    "local_ai_start",
    "local_ai_stop",
    "local_ai_delete",
    "local_ai_ask",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run the Tauri build script");
}
