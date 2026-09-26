//! The Zaklon hub library. Opens the household folder, then serves:
//! - HTTPS on `config.port` (default 8484) for paired phones,
//! - HTTP on 127.0.0.1:8481 for the desktop window,
//! - HTTP on 0.0.0.0:8480 for the "install the app" page and APK files.
//!
//! It announces itself with DNS-SD plus a UDP beacon, and runs the library
//! engine (kiwix-serve) on a private loopback port.

pub mod api;
pub mod discovery;
pub mod downloads;
pub mod install;
pub mod kiwix;
pub mod ui;

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context, Result};
use tracing::info;
use zaklon_core::catalog::Catalog;
use zaklon_core::tls::Identity;

use downloads::Downloads;
use kiwix::Library;
use zaklon_core::{Config, Db};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub use zaklon_core::config::{BEACON_PORT, INSTALL_PORT, LOCAL_PORT};

/// A pairing code shown on the laptop; valid for a few minutes.
#[derive(Debug, Clone)]
pub struct PairingSession {
    pub expires_at: Instant,
    pub failed_attempts: u8,
}

pub struct HubState {
    pub config: Mutex<Config>,
    pub db: Db,
    pub identity: Identity,
    pub started: Instant,
    pub pairing: Mutex<HashMap<String, PairingSession>>,
    pub downloads: Arc<Downloads>,
    pub library: Arc<Library>,
}

impl HubState {
    pub fn config(&self) -> Config {
        self.config.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
    pub fn uptime_secs(&self) -> u64 {
        self.started.elapsed().as_secs()
    }
    /// IPv4 addresses phones can reach us on (non-loopback, non-link-local).
    pub fn lan_addresses(&self) -> Vec<Ipv4Addr> {
        discovery::lan_ipv4_addresses()
    }
}

#[derive(Clone)]
pub struct Hub {
    state: Arc<HubState>,
}

/// Default data folder when none is given: `%ZAKLON_ROOT%`, else
/// `%LOCALAPPDATA%\Zaklon`, else `./zaklon-data`.
pub fn default_root() -> PathBuf {
    if let Ok(p) = std::env::var("ZAKLON_ROOT") {
        return PathBuf::from(p);
    }
    if let Ok(p) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(p).join("Zaklon");
    }
    PathBuf::from("zaklon-data")
}

impl Hub {
    pub fn open(root: &Path) -> Result<Self> {
        let config = Config::load_or_init(root).context("loading hub configuration")?;
        config.ensure_layout()?;
        let db = Db::open(&config.db_path()).context("opening household database")?;
        let identity = zaklon_core::tls::load_or_generate(&config.tls_dir(), &config.hub_name)
            .context("loading TLS identity")?;
        let downloads = Downloads::new(
            Catalog::load(&config.catalog_dir()),
            config.library_dir(),
            config.catalog_dir().join("state.json"),
        );
        info!(root = %root.display(), hub = %config.hub_name, fp = %identity.fingerprint_display(), "hub opened");
        Ok(Self {
            state: Arc::new(HubState {
                config: Mutex::new(config),
                db,
                identity,
                started: Instant::now(),
                pairing: Mutex::new(HashMap::new()),
                library: Library::new(downloads.clone()),
                downloads,
            }),
        })
    }

    pub fn state(&self) -> Arc<HubState> {
        self.state.clone()
    }

    pub fn local_base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.state.config().local_port)
    }

    /// Serve until the process ends. Never returns Ok while healthy.
    pub async fn run(&self) -> Result<()> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let state = self.state.clone();
        let cfg = state.config();

        let tls = axum_server::tls_rustls::RustlsConfig::from_pem(
            state.identity.cert_pem.clone().into_bytes(),
            state.identity.key_pem.clone().into_bytes(),
        )
        .await
        .context("building TLS config")?;

        let network_app = api::router(state.clone(), api::Listener::Network);
        let local_app = api::router(state.clone(), api::Listener::Local);
        let tls_addr = SocketAddr::from(([0, 0, 0, 0], cfg.port));
        let local_addr = SocketAddr::from(([127, 0, 0, 1], cfg.local_port));
        let install_addr = SocketAddr::from(([0, 0, 0, 0], cfg.install_port));

        info!(%tls_addr, %local_addr, %install_addr, "listening");

        let tls_srv = axum_server::bind_rustls(tls_addr, tls)
            .serve(network_app.into_make_service_with_connect_info::<SocketAddr>());
        let local_srv = axum_server::bind(local_addr)
            .serve(local_app.into_make_service_with_connect_info::<SocketAddr>());
        let install_srv = axum_server::bind(install_addr)
            .serve(install::router(state.clone()).into_make_service_with_connect_info::<SocketAddr>());

        let _discovery = discovery::start(state.clone()).await?;
        state.downloads.start();
        state.library.start();

        tokio::select! {
            r = tls_srv => r.context("tls server")?,
            r = local_srv => r.context("local server")?,
            r = install_srv => r.context("install server")?,
        }
        Ok(())
    }
}
