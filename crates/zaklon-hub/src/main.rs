//! `zaklon-hub` binary: runs the hub as a background process. Release builds
//! open no console window and log to `<root>/logs/hub.log`; debug builds also
//! print to the console. The desktop app embeds the same library in-process.

// No console window in release builds (the hub is started by a scheduled task or the desktop app).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let root = std::env::args()
        .skip_while(|a| a != "--root")
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(zaklon_hub::default_root);

    // Log to the console and to <root>/logs/hub.log (rotated daily), so a hub
    // started by a scheduled task still leaves a trace.
    let _ = std::fs::create_dir_all(root.join("logs"));
    let file = tracing_appender::rolling::daily(root.join("logs"), "hub.log");
    let (file_writer, _guard) = tracing_appender::non_blocking(file);
    use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "zaklon_hub=info,zaklon_core=info,tower_http=info".into());
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_ansi(false).with_writer(std::io::stdout))
        .with(fmt::layer().with_ansi(false).with_writer(file_writer))
        .init();

    let hub = zaklon_hub::Hub::open(&root)?;
    hub.run().await
}
