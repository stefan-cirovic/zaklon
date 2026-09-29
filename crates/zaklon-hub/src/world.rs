//! Which build of the world map the hub offers. Protomaps lists the builds
//! it keeps in builds.json; the hub reads that list when Maps or Storage &
//! Downloads is opened, at most about once a day (sooner only when the build being downloaded
//! was deleted), and keeps the last good list in
//! `<root>/catalog/world-builds.json`, so a hub without internet still
//! offers the last build it knew. Without any list it offers the build
//! pinned in the app. The choice itself is in `zaklon_core::world_map`;
//! what happens on disk (updates, unfinished downloads) in downloads.rs.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tracing::{info, warn};
use zaklon_core::world_map::{self, Build, WorldOffer, WORLD_MAP_ID};

use crate::downloads::{Downloads, GONE};

/// The last good list, in the hub's catalog folder.
pub const CACHE_FILE: &str = "world-builds.json";
/// A list this old is read again (when Maps or Storage & Downloads is opened).
const EVERY: time::Duration = time::Duration::hours(24);
/// After an attempt (one that failed without internet, say), the next one waits this long.
const RETRY: Duration = Duration::from_secs(3600);
/// Far more than a real list (about 12 kB in 2026).
const MAX_LIST: usize = 4 << 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Listed {
    /// When the list was read (RFC 3339, UTC).
    fetched_at: String,
    builds: Vec<Build>,
}

pub struct WorldBuilds {
    cache: PathBuf,
    /// Where the list is read from; None: never (`ZAKLON_WORLD_BUILDS_URL=off`, for tests).
    url: Option<String>,
    client: reqwest::Client,
    listed: Mutex<Option<Listed>>,
    last_attempt: Mutex<Option<Instant>>,
    busy: AtomicBool,
}

impl WorldBuilds {
    /// For the hub whose catalog folder is `catalog_dir`. The list comes from
    /// Protomaps, or from `ZAKLON_WORLD_BUILDS_URL` when that is set ("off":
    /// it is never read, as in the interface tests).
    pub fn open(catalog_dir: &Path) -> Arc<Self> {
        let url = match std::env::var("ZAKLON_WORLD_BUILDS_URL") {
            Ok(v) if v.trim().is_empty() || v.trim().eq_ignore_ascii_case("off") => None,
            Ok(v) => Some(v.trim().to_string()),
            Err(_) => Some(world_map::BUILDS_URL.to_string()),
        };
        Self::new(catalog_dir.join(CACHE_FILE), url)
    }

    pub fn new(cache: PathBuf, url: Option<String>) -> Arc<Self> {
        let listed = std::fs::read_to_string(&cache).ok().and_then(|t| serde_json::from_str::<Listed>(&t).ok());
        if let Some(l) = &listed {
            info!(builds = l.builds.len(), read = %l.fetched_at, "world map builds, from the last list");
        }
        let client = reqwest::Client::builder()
            .user_agent(crate::downloads::user_agent())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("http client");
        Arc::new(Self { cache, url, client, listed: Mutex::new(listed), last_attempt: Mutex::new(None), busy: AtomicBool::new(false) })
    }

    /// What to offer now.
    pub fn offer(&self) -> WorldOffer {
        self.offer_at(OffsetDateTime::now_utc())
    }

    /// What to offer at `now`. A build's week is counted up to when the list
    /// was read: only a build that was a week old then is known to have
    /// survived Protomaps' weekly cleanup (a daily build in a list read days
    /// ago may be gone by now).
    pub fn offer_at(&self, now: OffsetDateTime) -> WorldOffer {
        let listed = self.listed.lock().unwrap_or_else(|p| p.into_inner());
        let read = listed.as_ref().and_then(|l| OffsetDateTime::parse(&l.fetched_at, &Rfc3339).ok());
        world_map::offer(listed.as_ref().map(|l| l.builds.as_slice()), read.map_or(now, |t| t.min(now)))
    }

    /// When the list was last read, if ever.
    pub fn checked_at(&self) -> Option<String> {
        self.listed.lock().unwrap_or_else(|p| p.into_inner()).as_ref().map(|l| l.fetched_at.clone())
    }

    /// Whether to read the list now: never read yet, or a day ago, or `soon`
    /// (the build being downloaded is gone); but not within an hour of the
    /// last attempt, and never when reading it is switched off.
    fn due(&self, now: OffsetDateTime, soon: bool) -> bool {
        if self.url.is_none() {
            return false;
        }
        if self.last_attempt.lock().unwrap_or_else(|p| p.into_inner()).is_some_and(|t| t.elapsed() < RETRY) {
            return false;
        }
        let read = self.listed.lock().unwrap_or_else(|p| p.into_inner()).as_ref().and_then(|l| OffsetDateTime::parse(&l.fetched_at, &Rfc3339).ok());
        // (A time in the future: the clock was changed.)
        soon || read.is_none_or(|t| now - t >= EVERY || t > now)
    }

    /// Read the list now. A good one is kept, in memory and in the cache
    /// file; anything else leaves the last good one as it was.
    pub async fn fetch(&self) -> Result<(), String> {
        let Some(url) = self.url.clone() else { return Err("reading the list is switched off".into()) };
        *self.last_attempt.lock().unwrap_or_else(|p| p.into_inner()) = Some(Instant::now());
        let res = self.client.get(&url).header("accept", "application/json").send().await.map_err(|e| e.to_string())?;
        if !res.status().is_success() {
            return Err(format!("server replied {}", res.status()));
        }
        let mut body = Vec::new();
        let mut stream = res.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| e.to_string())?;
            if body.len() + chunk.len() > MAX_LIST {
                return Err("the list is far too large".into());
            }
            body.extend_from_slice(&chunk);
        }
        let text = String::from_utf8(body).map_err(|_| "the list is not text".to_string())?;
        let builds = world_map::parse_builds(&text)?;
        if builds.is_empty() {
            return Err("the list is empty".into());
        }
        let listed = Listed { fetched_at: zaklon_core::dates::now_rfc3339(), builds };
        let (path, json) = (self.cache.clone(), serde_json::to_vec_pretty(&listed).map_err(|e| e.to_string())?);
        let saved = tokio::task::spawn_blocking(move || {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            zaklon_core::config::write_atomic(&path, &json)
        })
        .await;
        if !matches!(saved, Ok(Ok(()))) {
            warn!("could not keep the list of world map builds: {saved:?}");
        }
        info!(builds = listed.builds.len(), "list of world map builds read");
        *self.listed.lock().unwrap_or_else(|p| p.into_inner()) = Some(listed);
        Ok(())
    }

    /// Maps or Storage & Downloads was opened: read the list in the background when it is due,
    /// then offer what it says. When it is not due the offer is brought up
    /// to date all the same (a build may have grown a week old since).
    /// Returns true when the list is being read.
    pub fn check(self: &Arc<Self>, downloads: &Arc<Downloads>) -> bool {
        let gone = downloads.state_of(WORLD_MAP_ID).is_some_and(|s| s.error.as_deref() == Some(GONE));
        if !self.due(OffsetDateTime::now_utc(), gone) || self.busy.swap(true, Ordering::SeqCst) {
            downloads.set_world(self.offer());
            return false;
        }
        let (me, downloads) = (self.clone(), downloads.clone());
        tokio::spawn(async move {
            if let Err(e) = me.fetch().await {
                // Without internet this is expected: the last list (or the pinned build) stays.
                info!("could not read the list of world map builds: {e}");
            }
            downloads.set_world(me.offer());
            me.busy.store(false, Ordering::SeqCst);
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../zaklon-core/testdata/protomaps-builds-20260929.json");

    fn temp(name: &str) -> PathBuf {
        use std::sync::atomic::AtomicU64;
        static N: AtomicU64 = AtomicU64::new(0);
        let d = std::env::temp_dir().join(format!("zaklon-world-{name}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn at(ts: &str) -> OffsetDateTime {
        OffsetDateTime::parse(ts, &Rfc3339).unwrap()
    }

    fn today() -> OffsetDateTime {
        at("2026-09-29T09:00:00Z")
    }

    /// Nothing listens here: a hub without internet.
    const OFFLINE: &str = "http://127.0.0.1:9/builds.json";

    fn cache_with(fetched_at: &str) -> PathBuf {
        let cache = temp("cache").join(CACHE_FILE);
        let listed = Listed { fetched_at: fetched_at.into(), builds: world_map::parse_builds(FIXTURE).unwrap() };
        std::fs::write(&cache, serde_json::to_vec(&listed).unwrap()).unwrap();
        cache
    }

    /// Serves `body` as the list, and tells which User-Agent asked for it.
    async fn serve(body: &'static str, content_type: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
        let agents = Arc::new(Mutex::new(Vec::new()));
        let seen = agents.clone();
        let app = axum::Router::new().route(
            "/builds.json",
            axum::routing::get(move |headers: axum::http::HeaderMap| {
                let seen = seen.clone();
                async move {
                    seen.lock().unwrap().push(headers.get("user-agent").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string());
                    ([(axum::http::header::CONTENT_TYPE, content_type)], body)
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{addr}/builds.json"), agents)
    }

    #[tokio::test]
    async fn the_last_list_is_offered_without_internet() {
        let cache = cache_with("2026-09-01T10:00:00Z");
        let w = WorldBuilds::new(cache.clone(), Some(OFFLINE.into()));
        assert!(w.due(today(), false), "four weeks old");
        assert!(w.fetch().await.is_err());
        let offer = w.offer_at(today());
        assert!(offer.listed);
        assert_eq!(offer.offer.version, "20260811");
        assert!(offer.offer.files[0].blake3.is_some());
        assert_eq!(w.checked_at().as_deref(), Some("2026-09-01T10:00:00Z"));
        assert!(!w.due(today(), true), "not again within the hour, even when asked soon");
        let kept: Listed = serde_json::from_str(&std::fs::read_to_string(&cache).unwrap()).unwrap();
        assert_eq!(kept.fetched_at, "2026-09-01T10:00:00Z", "a failure leaves the last list as it was");
    }

    #[test]
    fn a_week_counts_up_to_when_the_list_was_read() {
        // Read on 2026-09-29: its daily builds of the last week may be gone
        // a week later, when they would be a week old; the list cannot tell.
        let w = WorldBuilds::new(cache_with("2026-09-29T09:00:00Z"), None);
        assert_eq!(w.offer_at(at("2026-10-10T09:00:00Z")).offer.version, "20260811");
        // A list read on 2026-10-06 has kept 20260928 for a week: it stays.
        let w = WorldBuilds::new(cache_with("2026-10-06T00:00:00Z"), None);
        assert_eq!(w.offer_at(at("2026-10-10T09:00:00Z")).offer.version, "20260928");
        // A list from the future (the clock was set back) counts from now.
        let w = WorldBuilds::new(cache_with("2027-01-01T00:00:00Z"), None);
        assert_eq!(w.offer_at(today()).offer.version, "20260811");
    }

    #[tokio::test]
    async fn without_any_list_the_pinned_build_is_offered() {
        let cache = temp("none").join(CACHE_FILE);
        let w = WorldBuilds::new(cache.clone(), Some(OFFLINE.into()));
        assert!(w.fetch().await.is_err());
        let offer = w.offer();
        assert!(!offer.listed);
        assert_eq!(offer.offer.files[0].path, "maps/protomaps-world-20260928.pmtiles");
        assert_eq!(offer.offer.files[0].sha256, world_map::PINNED_SHA256);
        assert!(w.checked_at().is_none());
        assert!(!cache.exists());
        // A damaged cache file is no list either.
        std::fs::write(&cache, "{ not json").unwrap();
        assert!(!WorldBuilds::new(cache, None).offer().listed);
    }

    #[tokio::test]
    async fn the_list_is_read_with_zaklons_name_and_kept() {
        let (url, agents) = serve(FIXTURE, "application/json").await;
        let cache = temp("read").join(CACHE_FILE);
        let w = WorldBuilds::new(cache.clone(), Some(url.clone()));
        assert!(w.due(OffsetDateTime::now_utc(), false), "never read");
        w.fetch().await.unwrap();
        assert!(agents.lock().unwrap()[0].starts_with("Zaklon/"), "{:?}", agents.lock().unwrap());
        assert!(w.checked_at().is_some());
        assert!(!w.due(OffsetDateTime::now_utc(), false), "read just now");
        assert!(!w.due(OffsetDateTime::now_utc() + EVERY, false), "and not again within the hour of trying");
        // A restarted hub offers the same without reading it again.
        let again = WorldBuilds::new(cache.clone(), Some(OFFLINE.into()));
        assert_eq!(again.offer_at(today()).offer.version, "20260811");
        assert!(!again.due(OffsetDateTime::now_utc(), false), "read today");
        assert!(again.due(OffsetDateTime::now_utc() + EVERY, false), "a day later");
        assert!(!WorldBuilds::new(cache.clone(), None).due(OffsetDateTime::now_utc() + EVERY, true), "switched off: never");

        // A web page or an empty list in its place: the last good list stays.
        let before = std::fs::read_to_string(&cache).unwrap();
        for (body, kind) in [("<html>Maintenance</html>", "text/html"), ("[]", "application/json")] {
            let (url, _) = serve(body, kind).await;
            let w = WorldBuilds::new(cache.clone(), Some(url));
            assert!(w.fetch().await.is_err(), "{body}");
            assert_eq!(w.offer_at(today()).offer.version, "20260811");
        }
        assert_eq!(std::fs::read_to_string(&cache).unwrap(), before);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn opening_add_ons_reads_the_list_and_offers_its_build() {
        let (url, agents) = serve(FIXTURE, "application/json").await;
        let root = temp("hub");
        let w = WorldBuilds::new(root.join(CACHE_FILE), Some(url));
        let mut catalog = zaklon_core::catalog::Catalog { version: 1, generated: "2999-01-01".into(), starter_sets: Vec::new(), packs: Vec::new(), withdrawn: Vec::new() };
        catalog.packs.push(world_map::pinned());
        let d = Downloads::with_world(catalog, root.join("library"), root.join("state.json"), Some(w.offer()));
        assert_eq!(d.catalog().pack(WORLD_MAP_ID).unwrap().version, "20260928", "no list yet: the pinned build");
        assert!(w.check(&d), "read in the background");
        let deadline = Instant::now() + Duration::from_secs(10);
        while w.busy.load(Ordering::SeqCst) || w.checked_at().is_none() {
            assert!(Instant::now() < deadline, "the list was not read");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        // The newest build a week old on the day the test runs (the fixture's
        // newest ones are, from 2026-10-06 on).
        let chosen = world_map::choose(&world_map::parse_builds(FIXTURE).unwrap(), OffsetDateTime::now_utc()).unwrap().date().unwrap().to_string();
        assert_eq!(d.catalog().pack(WORLD_MAP_ID).unwrap().version, chosen);
        assert!(d.world_view().unwrap().listed);
        // Opened again: not read again.
        assert!(!w.check(&d));
        assert_eq!(agents.lock().unwrap().len(), 1);
    }
}
