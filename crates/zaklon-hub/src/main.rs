//! `zaklon-hub` binary: runs the hub as a plain console process.
//! The desktop app embeds the same library and starts it in-process.

use std::path::PathBuf;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "zaklon_hub=info,zaklon_core=info,tower_http=info".into()),
        )
        .init();

    let root = std::env::args()
        .skip_while(|a| a != "--root")
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(zaklon_hub::default_root);

    let hub = zaklon_hub::Hub::open(&root)?;
    hub.run().await
}
