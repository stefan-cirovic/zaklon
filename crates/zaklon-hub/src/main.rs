//! `zaklon-hub` binary: runs the hub as a plain console process.
//! The desktop app embeds the same library and starts it in-process.

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
        .with(fmt::layer().with_writer(std::io::stdout))
        .with(fmt::layer().with_ansi(false).with_writer(file_writer))
        .init();

    let hub = zaklon_hub::Hub::open(&root)?;
    hub.run().await
}
