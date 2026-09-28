//! The library engine. Runs `kiwix-serve` (GPL-3.0, a separate program) on
//! 127.0.0.1 only, serving every installed knowledge pack, restarts it when
//! packs are added or removed or when it stops, and searches it for the API.
//! Phones and the desktop window never talk to kiwix-serve directly: the hub
//! proxies `/kiwix/...` for authenticated callers.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt;
use serde::Serialize;
use tokio::process::{Child, Command};
use tracing::{info, warn};
use zaklon_core::catalog::Category;
use zaklon_core::translit;

use crate::downloads::Downloads;

/// URL prefix kiwix-serve uses for everything it serves; the hub proxies it unchanged.
pub const ROOT: &str = "/kiwix";

const CHECK_INTERVAL: Duration = Duration::from_secs(5);
/// After this many failed starts in a row, wait before trying again.
const MAX_FAILURES: u32 = 5;
const FAILURE_COOLDOWN: Duration = Duration::from_secs(5 * 60);
/// Recent search results kept in memory, and for how long.
const RECENT_SEARCHES: usize = 200;
const RECENT_FOR: Duration = Duration::from_secs(30 * 60);

/// One installed knowledge pack as the library sees it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Book {
    /// Kiwix book name: the ZIM file name without `.zim`.
    pub name: String,
    pub pack_id: String,
    pub title_en: String,
    pub title_sr: String,
    /// ISO 639-3 languages of the content (e.g. "srp", "eng").
    pub languages: Vec<String>,
    /// Main page of the book, relative to the hub.
    pub home: String,
    #[serde(skip)]
    pub file: PathBuf,
    /// The file's path inside the library ("zim/x.zim").
    #[serde(skip)]
    pub rel: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EngineState {
    /// The library engine add-on is not installed.
    Missing,
    /// Installed, but no knowledge pack yet.
    Idle,
    Starting,
    Running,
    /// It failed to start or keeps stopping.
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub title: String,
    /// Path to the article, relative to the hub (starts with `/kiwix/content/`).
    pub url: String,
    pub snippet: String,
    pub book: String,
    pub book_title_en: String,
    pub book_title_sr: String,
    /// "title" for a title match, "text" for a full-text match.
    pub kind: &'static str,
}

/// What one search found, and whether it was remembered from a recent one.
#[derive(Debug, Clone, Default)]
pub struct Found {
    pub results: Vec<SearchResult>,
    pub cached: bool,
}

/// Recent search results, so a question asked again, or a query two
/// questions share, does not go to the disk again: on a hard disk a search
/// takes from a tenth of a second to half a minute when another program
/// keeps the disk busy. Bounded in size and age.
pub(crate) struct Recent<V> {
    entries: std::collections::VecDeque<(String, std::time::Instant, V)>,
    capacity: usize,
    ttl: Duration,
}

impl<V: Clone> Recent<V> {
    pub(crate) fn new(capacity: usize, ttl: Duration) -> Self {
        Self { entries: std::collections::VecDeque::new(), capacity, ttl }
    }

    pub(crate) fn get(&mut self, key: &str, now: std::time::Instant) -> Option<V> {
        let ttl = self.ttl;
        self.entries.retain(|(_, at, _)| now.saturating_duration_since(*at) < ttl);
        self.entries.iter().find(|(k, _, _)| k == key).map(|(_, _, v)| v.clone())
    }

    /// Remember a value; the oldest one goes when there are too many.
    pub(crate) fn put(&mut self, key: String, value: V, now: std::time::Instant) {
        self.entries.retain(|(k, _, _)| *k != key);
        self.entries.push_back((key, now, value));
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
        }
    }
}

struct Running {
    child: Child,
    books: Vec<String>,
}

/// A free loopback port, chosen fresh for every start so a leftover process
/// holding an old port can never block the engine.
fn free_port() -> std::io::Result<u16> {
    let l = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    Ok(l.local_addr()?.port())
}

pub struct Library {
    downloads: Arc<Downloads>,
    engine_dir: PathBuf,
    running: tokio::sync::Mutex<Option<Running>>,
    state: Mutex<EngineState>,
    failures: Mutex<u32>,
    last_failure: Mutex<Option<std::time::Instant>>,
    /// While set and in the future, the engine is kept stopped (e.g. during a pack removal).
    hold_until: Mutex<Option<std::time::Instant>>,
    /// Port kiwix-serve currently listens on (0 when not running).
    port: std::sync::atomic::AtomicU16,
    #[cfg(windows)]
    job: job::Job,
    http: reqwest::Client,
    /// The assistant's recent searches (keyed by kind, books, size and query).
    recent: Mutex<Recent<Vec<SearchResult>>>,
}

impl Library {
    pub fn new(downloads: Arc<Downloads>) -> Arc<Self> {
        let engine_dir = ENGINE_DIR.iter().fold(downloads.library_dir().to_path_buf(), |p, c| p.join(c));
        Arc::new(Self {
            downloads,
            engine_dir,
            running: tokio::sync::Mutex::new(None),
            state: Mutex::new(EngineState::Missing),
            failures: Mutex::new(0),
            last_failure: Mutex::new(None),
            hold_until: Mutex::new(None),
            port: std::sync::atomic::AtomicU16::new(0),
            #[cfg(windows)]
            job: job::Job::new(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .no_proxy()
                .build()
                .expect("http client"),
            recent: Mutex::new(Recent::new(RECENT_SEARCHES, RECENT_FOR)),
        })
    }

    pub fn state(&self) -> EngineState {
        *self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Search a kiwix-serve that already runs on this port (for measuring a
    /// real library in tests; nothing is started or stopped).
    #[cfg(test)]
    pub(crate) fn attach(&self, port: u16) {
        self.port.store(port, std::sync::atomic::Ordering::Relaxed);
        self.set_state(EngineState::Running);
    }

    fn set_state(&self, s: EngineState) {
        *self.state.lock().unwrap_or_else(|p| p.into_inner()) = s;
    }

    fn fail(&self) {
        *self.failures.lock().unwrap_or_else(|p| p.into_inner()) += 1;
        *self.last_failure.lock().unwrap_or_else(|p| p.into_inner()) = Some(std::time::Instant::now());
    }

    fn base(&self) -> String {
        let port = self.port.load(std::sync::atomic::Ordering::Relaxed);
        format!("http://127.0.0.1:{port}")
    }

    fn exe(&self) -> PathBuf {
        let name = if cfg!(windows) { "kiwix-serve.exe" } else { "kiwix-serve" };
        self.engine_dir.join(name)
    }

    /// Installed knowledge packs, in catalog order. Their verified files on
    /// disk count, so a pack keeps its older version while a newer one downloads.
    pub fn books(&self) -> Vec<Book> {
        let library = self.downloads.library_dir().to_path_buf();
        self.downloads
            .snapshot()
            .into_iter()
            .filter(|v| v.pack.category == Category::Knowledge)
            .flat_map(|v| {
                let library = library.clone();
                v.state.files.into_iter().filter_map(move |f| {
                    let file = library.join(&f.path);
                    let name = Path::new(&f.path).file_stem()?.to_string_lossy().to_string();
                    file.is_file().then(|| Book {
                        home: format!("{ROOT}/content/{name}/"),
                        name,
                        pack_id: v.pack.id.clone(),
                        title_en: v.pack.title.en.clone(),
                        title_sr: v.pack.title.sr.clone(),
                        languages: v.pack.languages.clone(),
                        file,
                        rel: f.path,
                    })
                })
            })
            .collect()
    }

    /// Keep kiwix-serve in line with the installed packs. Call from a Tokio runtime.
    pub fn start(self: &Arc<Self>) {
        let me = self.clone();
        tokio::spawn(async move {
            loop {
                me.reconcile().await;
                tokio::time::sleep(CHECK_INTERVAL).await;
            }
        });
    }

    /// Stop the engine and keep it stopped for `hold` (it then restarts with
    /// whatever packs are installed at that time).
    pub async fn stop_for(&self, hold: Duration) {
        *self.hold_until.lock().unwrap_or_else(|p| p.into_inner()) = Some(std::time::Instant::now() + hold);
        let mut running = self.running.lock().await;
        if let Some(mut r) = running.take() {
            let _ = r.child.kill().await;
            let _ = r.child.wait().await;
            info!("library engine stopped for maintenance");
            // It comes back by itself once the hold ends.
            self.set_state(EngineState::Starting);
        }
    }

    async fn reconcile(&self) {
        let held = self
            .hold_until
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some_and(|t| std::time::Instant::now() < t);
        if held {
            return;
        }
        let mut running = self.running.lock().await;

        if !self.exe().is_file() {
            if let Some(mut r) = running.take() {
                let _ = r.child.kill().await;
            }
            self.set_state(EngineState::Missing);
            return;
        }
        let books = self.books();
        let wanted: Vec<String> = books.iter().map(|b| b.name.clone()).collect();

        // Is the current process still alive and serving the right books?
        if let Some(r) = running.as_mut() {
            let exited = matches!(r.child.try_wait(), Ok(Some(_)) | Err(_));
            if exited {
                warn!("library engine stopped; restarting");
                self.fail();
                *running = None;
            } else if r.books == wanted {
                if self.state() == EngineState::Starting && self.ping().await {
                    self.set_state(EngineState::Running);
                    *self.failures.lock().unwrap_or_else(|p| p.into_inner()) = 0;
                    info!(books = wanted.len(), "library engine ready");
                    // It skips files it cannot open (--skipInvalid); have those checked again.
                    if let Some(served) = self.served_books().await {
                        for b in books.iter().filter(|b| !served.contains(&b.name)) {
                            warn!(book = %b.name, "the library engine could not open this book; checking its file");
                            self.downloads.recheck(&b.pack_id);
                        }
                    }
                }
                return;
            } else {
                info!("knowledge packs changed; restarting the library engine");
                let _ = r.child.kill().await;
                *running = None;
            }
        }

        if wanted.is_empty() {
            self.set_state(EngineState::Idle);
            return;
        }
        if *self.failures.lock().unwrap_or_else(|p| p.into_inner()) >= MAX_FAILURES {
            let cooled = self
                .last_failure
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .map(|t| t.elapsed() >= FAILURE_COOLDOWN)
                .unwrap_or(true);
            if !cooled {
                self.set_state(EngineState::Failed);
                return;
            }
            *self.failures.lock().unwrap_or_else(|p| p.into_inner()) = 0;
        }
        let port = match free_port() {
            Ok(p) => p,
            Err(e) => {
                warn!("no free port for the library engine: {e}");
                self.fail();
                self.set_state(EngineState::Failed);
                return;
            }
        };

        let mut cmd = Command::new(self.exe());
        cmd.arg("--address=127.0.0.1")
            .arg(format!("--port={port}"))
            .arg(format!("--urlRootLocation={ROOT}"))
            .arg("--nosearchbar")
            // One damaged or unreadable file must not take every book down.
            .arg("--skipInvalid")
            .args(books.iter().map(|b| engine_relative(&b.rel)))
            .current_dir(&self.engine_dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        {
            // CREATE_NO_WINDOW: no console window for the child process.
            cmd.creation_flags(0x0800_0000);
        }
        match cmd.spawn() {
            Ok(child) => {
                #[cfg(windows)]
                if let Some(pid) = child.id() {
                    self.job.adopt(pid);
                }
                self.port.store(port, std::sync::atomic::Ordering::Relaxed);
                info!(books = wanted.len(), port, "library engine starting");
                *running = Some(Running { child, books: wanted });
                self.set_state(EngineState::Starting);
            }
            Err(e) => {
                warn!("could not start the library engine: {e}");
                self.fail();
                self.set_state(EngineState::Failed);
            }
        }
    }

    /// Names of the books kiwix-serve actually serves, from its catalog.
    async fn served_books(&self) -> Option<std::collections::HashSet<String>> {
        let res = self.http.get(format!("{}{ROOT}/catalog/v2/entries?count=-1", self.base())).send().await.ok()?;
        if !res.status().is_success() {
            return None;
        }
        Some(parse_served_books(&res.text().await.ok()?))
    }

    async fn ping(&self) -> bool {
        self.http
            .get(format!("{}{ROOT}/catalog/v2/root.xml", self.base()))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    /// Forward a GET for `/kiwix/...` to kiwix-serve.
    pub async fn fetch(&self, path_and_query: &str) -> Result<reqwest::Response, String> {
        self.http
            .get(format!("{}{path_and_query}", self.base()))
            .send()
            .await
            .map_err(|e| e.to_string())
    }

    /// Title matches first, then full-text matches, across the chosen books.
    /// Serbian books are searched with the query in both Latin and Cyrillic.
    pub async fn search(&self, query: &str, only_book: Option<&str>, limit: usize) -> Vec<SearchResult> {
        self.search_books(self.books(), query, only_book, limit).await
    }

    /// `search` over the given books.
    pub(crate) async fn search_books(&self, books: Vec<Book>, query: &str, only_book: Option<&str>, limit: usize) -> Vec<SearchResult> {
        let query = query.trim();
        if query.is_empty() || self.state() != EngineState::Running {
            return Vec::new();
        }
        let books: Vec<Book> = books.into_iter().filter(|b| only_book.is_none_or(|n| n == b.name)).collect();

        // Collect per book, then take turns between books so one big book
        // (e.g. Wikipedia) does not push the others out of the first page.
        let mut titles: Vec<Vec<SearchResult>> = Vec::new();
        let mut texts: Vec<Vec<SearchResult>> = Vec::new();
        let wanted: Vec<String> = {
            let mut w = vec![translit::fold(query)];
            if translit::has_serbian_latin(query) {
                w.extend(translit::cyrillic_candidates(query, 12).iter().map(|c| translit::fold(c)));
            }
            w
        };
        for book in &books {
            let serbian = book.languages.iter().any(|l| l == "srp") && translit::has_serbian_latin(query);
            // Full-text: the query as typed, plus its plain Cyrillic form for Serbian books.
            let mut text_queries = vec![query.to_string()];
            // Titles: also the spellings a query without diacritics may stand for.
            let mut title_queries = vec![query.to_string()];
            if serbian {
                let cands = translit::cyrillic_candidates(query, 12);
                text_queries.insert(0, cands[0].clone());
                title_queries = cands.into_iter().chain(std::iter::once(query.to_string())).collect();
            }
            // A few lookups at a time: fast, without flooding kiwix-serve.
            let lookups: Vec<_> = title_queries.iter().map(|q| self.suggest(book, q, limit.min(8))).collect();
            let mut t: Vec<SearchResult> = futures_util::stream::iter(lookups).buffered(4).collect::<Vec<_>>().await.into_iter().flatten().collect();
            // Exact title matches ("шећер" for "secer") first, keeping the rest in order.
            t.sort_by_key(|r| !wanted.contains(&translit::fold(&r.title)));
            // A title spelled like the query without diacritics ("Осигурач"
            // for "osigurac") is the better full-text query too.
            if serbian {
                let loose = translit::fold_loose(query);
                if let Some(r) = t.iter().find(|r| translit::fold_loose(&r.title) == loose) {
                    text_queries[0] = r.title.clone();
                }
            }
            let x: Vec<SearchResult> = futures_util::future::join_all(text_queries.iter().map(|q| self.fulltext(book, q, limit))).await.into_iter().flatten().collect();
            titles.push(t);
            texts.push(x);
        }

        let mut out: Vec<SearchResult> = Vec::new();
        for group in [titles, texts] {
            for r in round_robin(group) {
                if out.len() >= limit {
                    break;
                }
                if !out.iter().any(|o| o.url == r.url) {
                    out.push(r);
                }
            }
        }
        out
    }

    async fn suggest(&self, book: &Book, q: &str, limit: usize) -> Vec<SearchResult> {
        self.titles_in(book, q, limit).await.unwrap_or_default()
    }

    async fn fulltext(&self, book: &Book, q: &str, limit: usize) -> Vec<SearchResult> {
        self.text_in(&[book], q, limit).await.unwrap_or_default()
    }

    /// Full-text search in books of one language with a single request;
    /// kiwix ranks their articles together. It refuses books in different
    /// languages; then each book is asked on its own. The results of recent
    /// searches are remembered.
    pub async fn search_text(&self, books: &[&Book], query: &str, limit: usize) -> Found {
        let query = query.trim();
        if query.is_empty() || books.is_empty() || self.state() != EngineState::Running {
            return Found::default();
        }
        let names: Vec<&str> = books.iter().map(|b| b.name.as_str()).collect();
        let key = format!("text|{}|{limit}|{query}", names.join(","));
        if let Some(results) = self.recall(&key) {
            return Found { results, cached: true };
        }
        let results = match self.text_in(books, query, limit).await {
            Ok(r) => r,
            Err(Some(status)) if status == reqwest::StatusCode::BAD_REQUEST && books.len() > 1 => {
                let mut all = Vec::new();
                for &b in books {
                    match self.text_in(&[b], query, limit).await {
                        Ok(r) => all.extend(r),
                        Err(_) => return Found { results: all, cached: false },
                    }
                }
                all
            }
            Err(_) => return Found::default(),
        };
        self.remember(key, &results);
        Found { results, cached: false }
    }

    /// Titles in one book that start like the query. Remembered like `search_text`.
    pub async fn search_titles(&self, book: &Book, query: &str, limit: usize) -> Found {
        let query = query.trim();
        if query.is_empty() || self.state() != EngineState::Running {
            return Found::default();
        }
        let key = format!("titles|{}|{limit}|{query}", book.name);
        if let Some(results) = self.recall(&key) {
            return Found { results, cached: true };
        }
        match self.titles_in(book, query, limit).await {
            Some(results) => {
                self.remember(key, &results);
                Found { results, cached: false }
            }
            None => Found::default(),
        }
    }

    fn recall(&self, key: &str) -> Option<Vec<SearchResult>> {
        self.recent.lock().unwrap_or_else(|p| p.into_inner()).get(key, std::time::Instant::now())
    }

    fn remember(&self, key: String, results: &[SearchResult]) {
        self.recent.lock().unwrap_or_else(|p| p.into_inner()).put(key, results.to_vec(), std::time::Instant::now());
    }

    /// Title suggestions from one book; None when kiwix-serve did not answer.
    async fn titles_in(&self, book: &Book, q: &str, limit: usize) -> Option<Vec<SearchResult>> {
        let url = format!("{}{ROOT}/suggest", self.base());
        let res = self
            .http
            .get(url)
            .query(&[("content", book.name.as_str()), ("term", q), ("count", &limit.min(10).to_string())])
            .send()
            .await
            .ok()?;
        if !res.status().is_success() {
            return None;
        }
        let items = res.json::<Vec<serde_json::Value>>().await.ok()?;
        Some(
            items
                .into_iter()
                .filter(|v| v.get("kind").and_then(|k| k.as_str()) == Some("path"))
                .filter_map(|v| {
                    let title = v.get("value")?.as_str()?.to_string();
                    let path = v.get("path")?.as_str()?;
                    Some(SearchResult {
                        title,
                        url: format!("{ROOT}/content/{}/{}", book.name, encode_path(path)),
                        snippet: String::new(),
                        book: book.name.clone(),
                        book_title_en: book.title_en.clone(),
                        book_title_sr: book.title_sr.clone(),
                        kind: "title",
                    })
                })
                .collect(),
        )
    }

    /// One full-text request over the books; the error is the HTTP status
    /// kiwix-serve answered with, or None when it did not answer.
    async fn text_in(&self, books: &[&Book], q: &str, limit: usize) -> Result<Vec<SearchResult>, Option<reqwest::StatusCode>> {
        let url = format!("{}{ROOT}/search", self.base());
        let mut query: Vec<(&str, String)> = books.iter().map(|b| ("books.name", b.name.clone())).collect();
        query.extend([("pattern", q.to_string()), ("format", "xml".into()), ("pageLength", limit.to_string())]);
        let res = self.http.get(url).query(&query).send().await.map_err(|_| None)?;
        if !res.status().is_success() {
            return Err(Some(res.status()));
        }
        let xml = res.text().await.map_err(|_| None)?;
        Ok(parse_search_rss(&xml)
            .into_iter()
            .filter_map(|(title, link, snippet)| {
                // Each result names its book in its link.
                let book = book_of_link(&link)
                    .and_then(|name| books.iter().find(|b| b.name == name))
                    .or_else(|| (books.len() == 1).then(|| &books[0]))?;
                Some(SearchResult {
                    title,
                    url: link,
                    snippet,
                    book: book.name.clone(),
                    book_title_en: book.title_en.clone(),
                    book_title_sr: book.title_sr.clone(),
                    kind: "text",
                })
            })
            .collect())
    }
}

/// On Windows, child processes are placed in a job object that is closed when
/// the hub exits for any reason (even when killed), which makes Windows end
/// kiwix-serve too. Without this, an update or a crash leaves it running.
#[cfg(windows)]
pub(crate) mod job {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};

    pub struct Job(HANDLE);
    // SAFETY: a job handle can be used from any thread.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Job {
        pub fn new() -> Self {
            // SAFETY: plain Win32 calls with valid, zero-initialized arguments.
            unsafe {
                let h = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if !h.is_null() {
                    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                    SetInformationJobObject(
                        h,
                        JobObjectExtendedLimitInformation,
                        &info as *const _ as *const core::ffi::c_void,
                        std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                    );
                }
                Job(h)
            }
        }

        pub fn adopt(&self, pid: u32) {
            if self.0.is_null() {
                return;
            }
            // SAFETY: the handle is only used for the assignment and then closed.
            unsafe {
                let p = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
                if !p.is_null() {
                    if AssignProcessToJobObject(self.0, p) == 0 {
                        tracing::warn!("could not tie the library engine to the hub");
                    }
                    CloseHandle(p);
                }
            }
        }
    }
}

/// Interleave several lists: first of each, then second of each, and so on.
fn round_robin<T>(lists: Vec<Vec<T>>) -> Vec<T> {
    let mut iters: Vec<_> = lists.into_iter().map(|l| l.into_iter()).collect();
    let mut out = Vec::new();
    loop {
        let mut any = false;
        for it in iters.iter_mut() {
            if let Some(x) = it.next() {
                out.push(x);
                any = true;
            }
        }
        if !any {
            return out;
        }
    }
}

/// Where kiwix-serve lives inside the library; it runs from that folder.
const ENGINE_DIR: [&str; 2] = ["bin", "kiwix"];

/// A library file as kiwix-serve finds it from its own folder
/// (`..\..\zim\x.zim`). kiwix-serve reads its command line in the ANSI code
/// page, so an absolute path under a folder like `C:\Users\Đorđe` arrives
/// broken and the engine exits; library paths are ASCII. On Windows it also
/// refuses forward slashes.
fn engine_relative(rel: &str) -> std::ffi::OsString {
    let path = format!("{}{rel}", "../".repeat(ENGINE_DIR.len()));
    if cfg!(windows) {
        path.replace('/', "\\").into()
    } else {
        path.into()
    }
}

/// Book names in kiwix-serve's catalog feed: each entry links its content
/// as `href="/kiwix/content/<name>"`.
fn parse_served_books(xml: &str) -> std::collections::HashSet<String> {
    let marker = format!("href=\"{ROOT}/content/");
    xml.split(marker.as_str())
        .skip(1)
        .filter_map(|rest| rest.split(['"', '/']).next())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect()
}

/// The book an article link points into: `/kiwix/content/<name>/...`.
fn book_of_link(link: &str) -> Option<&str> {
    let rest = link.strip_prefix(ROOT)?.strip_prefix("/content/")?;
    rest.split('/').next().filter(|name| !name.is_empty())
}

/// Percent-encode an article path for a URL, keeping `/`.
fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for b in path.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Pull (title, link, plain-text snippet) out of kiwix-serve's OpenSearch RSS.
fn parse_search_rss(xml: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for item in xml.split("<item>").skip(1) {
        let item = item.split("</item>").next().unwrap_or(item);
        let title = tag(item, "title").map(unescape).unwrap_or_default();
        let link = tag(item, "link").map(unescape).unwrap_or_default();
        let desc = tag(item, "description").map(|d| clean_text(&unescape(d))).unwrap_or_default();
        if !link.is_empty() {
            out.push((title, link, desc));
        }
    }
    out
}

fn tag<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = s.find(&open)? + open.len();
    let end = s[start..].find(&close)? + start;
    Some(&s[start..end])
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// Drop HTML tags and collapse whitespace; keep it short.
fn clean_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let collapsed: String = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim_end_matches('.').to_string();
    if trimmed.chars().count() > 240 {
        let cut: String = trimmed.chars().take(240).collect();
        format!("{cut}…")
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSS: &str = r#"<rss><channel><title>Search: voda</title>
<item>
  <title>voda</title>
  <link>/kiwix/content/wiktionary_sr_all_nopic_2026-07/voda</link>
  <description><b>voda</b> (српски, ћир. вода) &amp; more......</description>
</item>
<item><title>a &amp; b</title><link>/kiwix/content/x/a_%26_b</link><description>x</description></item>
</channel></rss>"#;

    #[test]
    fn parses_rss() {
        let r = parse_search_rss(RSS);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].0, "voda");
        assert_eq!(r[0].1, "/kiwix/content/wiktionary_sr_all_nopic_2026-07/voda");
        assert_eq!(r[0].2, "voda (српски, ћир. вода) & more");
        assert_eq!(r[1].0, "a & b");
    }

    #[test]
    fn interleaves() {
        assert_eq!(round_robin(vec![vec![1, 2, 3], vec![10], vec![20, 21]]), vec![1, 10, 20, 2, 21, 3]);
    }

    #[test]
    fn engine_finds_books_under_a_non_ascii_folder() {
        // Windows accounts named like this are common for our users.
        let root = std::env::temp_dir().join(format!("zaklon-Ђорђе-Ćirović-{}", std::process::id()));
        let engine = ENGINE_DIR.iter().fold(root.clone(), |p, c| p.join(c));
        std::fs::create_dir_all(&engine).unwrap();
        std::fs::create_dir_all(root.join("zim")).unwrap();
        std::fs::write(root.join("zim").join("w_2026-01.zim"), b"zim").unwrap();
        let arg = engine_relative("zim/w_2026-01.zim");
        let text = arg.to_str().unwrap();
        assert!(text.is_ascii(), "the command line stays ASCII: {text}");
        assert!(!text.contains(if cfg!(windows) { '/' } else { '\\' }), "{text}");
        assert!(engine.join(&arg).is_file(), "kiwix-serve finds the file from its own folder");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn reads_served_books() {
        let feed = r#"<feed><entry><name>zimgit-water_en</name>
            <link type="text/html" href="/kiwix/content/zimgit-water_en_2024-08" /></entry>
            <entry><link type="text/html" href="/kiwix/content/wikibooks_sr_all_nopic_2026-07/" /></entry></feed>"#;
        let served = parse_served_books(feed);
        assert_eq!(served.len(), 2);
        assert!(served.contains("zimgit-water_en_2024-08"));
        assert!(served.contains("wikibooks_sr_all_nopic_2026-07"));
        assert!(parse_served_books("<feed></feed>").is_empty());
    }

    #[test]
    fn results_name_their_book() {
        assert_eq!(book_of_link("/kiwix/content/wiktionary_sr_all_nopic_2026-07/voda"), Some("wiktionary_sr_all_nopic_2026-07"));
        assert_eq!(book_of_link("/kiwix/content/w_2026-01/A/b%20c"), Some("w_2026-01"));
        assert_eq!(book_of_link("/other/content/w/x"), None);
        assert_eq!(book_of_link("/kiwix/content//x"), None);
    }

    #[test]
    fn recent_searches_are_kept_for_a_while_and_only_a_few() {
        let t0 = std::time::Instant::now();
        let later = |s: u64| t0 + Duration::from_secs(s);
        let mut r: Recent<u32> = Recent::new(2, Duration::from_secs(60));
        assert_eq!(r.get("a", t0), None);
        r.put("a".into(), 1, t0);
        assert_eq!(r.get("a", later(59)), Some(1), "remembered");
        assert_eq!(r.get("a", later(60)), None, "too old");
        r.put("a".into(), 1, later(100));
        r.put("a".into(), 2, later(101));
        assert_eq!(r.get("a", later(102)), Some(2), "the newest value wins, kept once");
        r.put("b".into(), 3, later(102));
        r.put("c".into(), 4, later(103));
        assert_eq!(r.get("a", later(104)), None, "the oldest goes when there are too many");
        assert_eq!((r.get("b", later(104)), r.get("c", later(104))), (Some(3), Some(4)));
        assert_eq!(r.entries.len(), 2);
    }

    #[test]
    fn encodes_paths() {
        assert_eq!(encode_path("Vod._M."), "Vod._M.");
        assert_eq!(encode_path("вода"), "%D0%B2%D0%BE%D0%B4%D0%B0");
        assert_eq!(encode_path("A/b c"), "A/b%20c");
    }
}
