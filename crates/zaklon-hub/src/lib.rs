//! The Zaklon hub library. Opens the household folder, then serves:
//! - HTTPS on `config.port` (default 8484) for paired phones,
//! - HTTP on 127.0.0.1:8481 for the desktop window,
//! - HTTP on 0.0.0.0:8480 for the "install the app" page and APK files.
//!
//! It announces itself with DNS-SD plus a UDP beacon, and runs the library
//! engine (kiwix-serve) on a private loopback port.

pub mod api;
pub mod assistant;
pub mod backup;
pub mod discovery;
pub mod downloads;
pub mod export;
pub mod firewall;
pub mod gazetteer;
pub mod hotspot;
pub mod install;
pub mod kiwix;
pub mod latin;
pub mod machine;
mod powershell;
pub mod tiles;
pub mod ui;
pub mod updates;
pub mod web;
pub mod world;

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
    /// Attempts taken, from every address together.
    pub failed_attempts: u8,
    /// Attempts taken by each address: one address may take only some of
    /// them (see `api/pairing.rs`), so a device that is not the phone being paired
    /// cannot use them all up on its own.
    pub attempts_by_ip: HashMap<IpAddr, u8>,
    /// The pairing QR code's secret (see `zaklon_core::pairing::pairing_secret`):
    /// a phone that scanned the QR code pairs with it. The 6-digit code
    /// itself works only through a code check from "Find hubs".
    pub secret: String,
    /// Code checks from "Find hubs" waiting for the phone's answer, by run
    /// id. Each one took one of the code's attempts, so there are never more
    /// than those.
    pub runs: HashMap<String, PakeRun>,
}

/// One code check from "Find hubs" (SPAKE2, see the zaklon-pake crate): the
/// hub has answered the phone's first message and waits for its proof.
#[derive(Debug, Clone)]
pub struct PakeRun {
    pub started: Instant,
    /// The address that started it.
    pub ip: IpAddr,
    /// The name the phone gave when it started the check; it pairs under it.
    pub device_name: String,
    /// The proof the phone must send back.
    pub expect: [u8; 32],
}

/// Failed pairing attempts. Each network address is blocked on its own after
/// too many failures, so one device guessing cannot cancel pairing for everyone;
/// a much higher total still cancels every open code.
#[derive(Debug, Default)]
pub struct PairingFailures {
    total: u32,
    /// Failures per address and when the last one happened.
    by_ip: HashMap<IpAddr, (u32, Instant)>,
}

/// Outcome of recording one failed pairing attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingFailure {
    /// Counted; nothing else happens.
    Counted,
    /// This address is now blocked for a while.
    AddressBlocked,
    /// Too many failures overall: open codes must be canceled.
    CancelAll,
}

impl PairingFailures {
    /// Failures from one address before it is blocked.
    pub const MAX_PER_IP: u32 = 10;
    /// How long a blocked address stays blocked (counted from its last failure).
    pub const IP_BLOCK: Duration = Duration::from_secs(10 * 60);
    /// Failures from all addresses together before every open code is canceled.
    pub const MAX_TOTAL: u32 = 200;

    /// Whether `ip` may not try pairing right now.
    pub fn is_blocked(&mut self, ip: IpAddr, now: Instant) -> bool {
        self.forget_old(now);
        self.by_ip.get(&ip).is_some_and(|(n, _)| *n >= Self::MAX_PER_IP)
    }

    /// Records one failed attempt from `ip`.
    pub fn record(&mut self, ip: IpAddr, now: Instant) -> PairingFailure {
        self.forget_old(now);
        let entry = self.by_ip.entry(ip).or_insert((0, now));
        entry.0 += 1;
        entry.1 = now;
        let per_ip = entry.0;
        self.total += 1;
        if self.total >= Self::MAX_TOTAL {
            self.total = 0;
            PairingFailure::CancelAll
        } else if per_ip >= Self::MAX_PER_IP {
            PairingFailure::AddressBlocked
        } else {
            PairingFailure::Counted
        }
    }

    /// Takes back one failure from `ip`. A code check from "Find hubs" counts
    /// as failed when it starts (only the phone learns whether the code was
    /// right) and is taken back when the phone pairs with it.
    pub fn forgive(&mut self, ip: IpAddr) {
        if let Some(entry) = self.by_ip.get_mut(&ip) {
            entry.0 = entry.0.saturating_sub(1);
            if entry.0 == 0 {
                self.by_ip.remove(&ip);
            }
        }
        self.total = self.total.saturating_sub(1);
    }

    /// A new code on the laptop starts the total over; blocked addresses stay
    /// blocked until their time runs out.
    pub fn reset_total(&mut self) {
        self.total = 0;
    }

    fn forget_old(&mut self, now: Instant) {
        self.by_ip.retain(|_, (_, last)| now.saturating_duration_since(*last) < Self::IP_BLOCK);
    }
}

pub struct HubState {
    pub config: Mutex<Config>,
    pub db: Db,
    pub identity: Identity,
    pub started: Instant,
    pub pairing: Mutex<HashMap<String, PairingSession>>,
    /// Failed pairing attempts since the last "Add a phone", per phone address.
    pub pairing_failures: Mutex<PairingFailures>,
    /// Recent successful pairings by the phone's nonce, so a phone whose
    /// reply got lost can ask again and get the same answer (no ghost device).
    /// The text is what the repeat must match: the code, or the code check.
    pub recent_pairs: Mutex<HashMap<String, (Instant, String, api::Paired)>>,
    /// Setting or changing the household password and turning on backup
    /// encryption happen one at a time, so the password and the backup key
    /// locked with it always match.
    pub password_lock: tokio::sync::Mutex<()>,
    pub downloads: Arc<Downloads>,
    pub library: Arc<Library>,
    pub export: Arc<export::Exporter>,
    pub assistant: Arc<assistant::Assistant>,
    pub updates: Arc<updates::Updates>,
    /// The Zaklon map's tiles, fonts and icons.
    pub tiles: Arc<tiles::Tiles>,
    /// Which build of the world map is offered (Protomaps' list of builds).
    pub world: Arc<world::WorldBuilds>,
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

/// How many daily log files are kept (about a month).
pub const LOG_FILES_KEPT: usize = 30;

/// A log file in `<root>/logs` that starts anew every day (`<name>.<date>`);
/// the oldest are deleted, so a hub that runs for years does not collect
/// them forever.
pub fn log_file(root: &Path, name: &str) -> Result<tracing_appender::rolling::RollingFileAppender> {
    let dir = root.join("logs");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix(name)
        .max_log_files(LOG_FILES_KEPT)
        .build(&dir)
        .with_context(|| format!("opening the log in {}", dir.display()))
}

/// Before a pack's files are replaced or deleted, the program holding them
/// open stops: kiwix-serve for knowledge packs and itself, llama-server for
/// AI models and itself. Both start again by themselves when needed.
fn release_engines(library: &Arc<Library>, assistant: &Arc<assistant::Assistant>, tiles: &Arc<tiles::Tiles>) -> Box<downloads::ReleaseFn> {
    use zaklon_core::catalog::Category;
    // Weak: the downloads must not keep the engines (which hold the downloads) alive.
    let (library, assistant, tiles) = (Arc::downgrade(library), Arc::downgrade(assistant), Arc::downgrade(tiles));
    Box::new(move |pack| {
        let (library, assistant, tiles) = (library.upgrade(), assistant.upgrade(), tiles.upgrade());
        Box::pin(async move {
            // The map reads its archives without holding them against a
            // delete, but a file being replaced should not stay open.
            if downloads::is_map_archive_pack(&pack) {
                if let Some(t) = tiles {
                    t.close_all().await;
                }
            }
            if pack.category == Category::Knowledge || pack.id == "kiwix-tools" {
                if let Some(l) = library {
                    l.stop_for(Duration::from_secs(10)).await;
                }
            }
            if pack.category == Category::Model || pack.id == "llama-cpp" {
                if let Some(a) = assistant {
                    a.stop().await;
                }
            }
        })
    })
}

impl Hub {
    pub fn open(root: &Path) -> Result<Self> {
        // A restore chosen before the last restart is swapped in before anything opens the data.
        match backup::finish_pending_restore(root) {
            Ok(true) => info!("restored the household data from a backup"),
            Ok(false) => {}
            // The swap either happened or was rolled back; the household keeps its data.
            Err(e) => tracing::error!("could not finish the restore, keeping the current data: {e}"),
        }
        // Unencrypted copies left by a backup or a restore that stopped halfway.
        backup::clean_leftovers(root);
        let config = Config::load_or_init(root).context("loading hub configuration")?;
        config.ensure_layout()?;
        let db = Db::open(&config.db_path()).context("opening household database")?;
        let identity = zaklon_core::tls::load_or_generate(&config.tls_dir(), &config.hub_name)
            .context("loading TLS identity")?;
        // The world map build to offer: from the last list of builds this hub
        // read, else the build pinned in the app. The list is read again
        // when Maps or Storage & Downloads is opened (see world.rs).
        let world = world::WorldBuilds::open(&config.catalog_dir());
        let downloads = Downloads::with_world(
            Catalog::load(&config.catalog_dir()),
            config.library_dir(),
            config.catalog_dir().join("state.json"),
            Some(world.offer()),
        );
        info!(root = %root.display(), hub = %config.hub_name, fp = %identity.fingerprint_display(), "hub opened");
        let library = Library::new(downloads.clone());
        let chosen = db.get_setting(assistant::SETTING_MODEL).ok().flatten();
        let assistant = assistant::Assistant::new(downloads.clone(), library.clone(), chosen);
        let tiles = tiles::Tiles::new(downloads.clone(), tiles::map_assets_dir());
        downloads.set_release(release_engines(&library, &assistant, &tiles));
        let updates = updates::Updates::new(config.auto_update_check);
        Ok(Self {
            state: Arc::new(HubState {
                config: Mutex::new(config),
                db,
                identity,
                started: Instant::now(),
                pairing: Mutex::new(HashMap::new()),
                pairing_failures: Mutex::new(PairingFailures::default()),
                recent_pairs: Mutex::new(HashMap::new()),
                password_lock: tokio::sync::Mutex::new(()),
                library,
                export: export::Exporter::new(),
                assistant,
                updates,
                downloads,
                tiles,
                world,
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
    ///
    /// Only the laptop's own window (local) and the phones' connection (TLS)
    /// end the run when they fail. The install page and discovery are extras:
    /// if they cannot start (another program holds the port, say), that is
    /// logged, they are tried again later, and everything else keeps serving.
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
        let network = if zaklon_core::config::loopback_only() { [127, 0, 0, 1] } else { [0, 0, 0, 0] };
        let tls_addr = SocketAddr::from((network, cfg.port));
        let local_addr = SocketAddr::from(([127, 0, 0, 1], cfg.local_port));
        let install_addr = SocketAddr::from((network, cfg.install_port));

        info!(%tls_addr, %local_addr, %install_addr, "listening");

        let tls_srv = axum_server::bind_rustls(tls_addr, tls)
            .serve(network_app.into_make_service_with_connect_info::<SocketAddr>());
        let local_srv = axum_server::bind(local_addr)
            .serve(local_app.into_make_service_with_connect_info::<SocketAddr>());
        let install_app = install::router(state.clone());
        tokio::spawn(keep_serving("install page", move || {
            axum_server::bind(install_addr).serve(install_app.clone().into_make_service_with_connect_info::<SocketAddr>())
        }));
        let _discovery = discovery::start(state.clone()).await;
        state.downloads.start();
        state.library.start();
        state.assistant.start();
        state.updates.start(state.clone());
        // A backup a day, checked every hour.
        let st = state.clone();
        tokio::spawn(async move {
            loop {
                let s2 = st.clone();
                let _ = tokio::task::spawn_blocking(move || backup::auto_backup_if_due(&s2.config(), &s2.db)).await;
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            }
        });

        tokio::select! {
            r = tls_srv => r.context("tls server")?,
            r = local_srv => r.context("local server")?,
        }
        Ok(())
    }
}

/// How long a part that is not essential waits before it is tried again.
const RETRY_EXTRA: Duration = Duration::from_secs(30);

/// Run a part that is not essential (see [`Hub::run`]) for as long as the
/// hub runs: when it cannot start or stops, log it (in full the first time,
/// briefly after that) and try again a little later.
pub(crate) async fn keep_serving<F, Fut>(what: &'static str, mut serve: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = std::io::Result<()>>,
{
    let mut failures = 0u32;
    loop {
        let result = serve().await;
        failures += 1;
        match result {
            Err(e) if failures == 1 => tracing::error!("{what} is not available, trying again every {} s: {e}", RETRY_EXTRA.as_secs()),
            Err(e) => tracing::debug!("{what} is still not available: {e}"),
            Ok(()) => tracing::warn!("{what} stopped; starting it again"),
        }
        tokio::time::sleep(RETRY_EXTRA).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_failures_block_one_address_only() {
        let mut f = PairingFailures::default();
        let now = Instant::now();
        let bad: IpAddr = "192.168.1.50".parse().unwrap();
        let good: IpAddr = "192.168.1.60".parse().unwrap();
        for _ in 1..PairingFailures::MAX_PER_IP {
            assert_eq!(f.record(bad, now), PairingFailure::Counted);
        }
        assert_eq!(f.record(bad, now), PairingFailure::AddressBlocked);
        assert!(f.is_blocked(bad, now));
        assert!(!f.is_blocked(good, now), "other phones can still pair");
        // The block runs out.
        assert!(!f.is_blocked(bad, now + PairingFailures::IP_BLOCK));
    }

    #[test]
    fn a_pairing_that_succeeds_is_not_held_against_the_address() {
        let mut f = PairingFailures::default();
        let now = Instant::now();
        let ip: IpAddr = "192.168.1.50".parse().unwrap();
        for _ in 1..PairingFailures::MAX_PER_IP {
            f.record(ip, now);
            f.forgive(ip);
        }
        assert_eq!(f.record(ip, now), PairingFailure::Counted, "only the failures count");
        f.forgive(ip);
        f.forgive(ip);
        assert!(f.by_ip.is_empty() && f.total == 0, "never below zero");
    }

    #[test]
    fn pairing_failures_total_cap_cancels() {
        let mut f = PairingFailures::default();
        let now = Instant::now();
        let mut outcomes = Vec::new();
        for i in 0..PairingFailures::MAX_TOTAL {
            let ip = IpAddr::from([10, 0, (i / 250) as u8, (i % 250) as u8]);
            outcomes.push(f.record(ip, now));
        }
        assert_eq!(outcomes.last(), Some(&PairingFailure::CancelAll));
        assert!(outcomes[..outcomes.len() - 1].iter().all(|o| *o == PairingFailure::Counted));
    }
}
