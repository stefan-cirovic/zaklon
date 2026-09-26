//! The library engine. Runs `kiwix-serve` (GPL-3.0, a separate program) on
//! 127.0.0.1 only, serving every installed knowledge pack, restarts it when
//! packs are added or removed or when it stops, and searches it for the API.
//! Phones and the desktop window never talk to kiwix-serve directly: the hub
//! proxies `/kiwix/...` for authenticated callers.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tokio::process::{Child, Command};
use tracing::{info, warn};
use zaklon_core::catalog::{Category, PackStatus};
use zaklon_core::translit;

use crate::downloads::Downloads;

/// URL prefix kiwix-serve uses for everything it serves; the hub proxies it unchanged.
pub const ROOT: &str = "/kiwix";

const CHECK_INTERVAL: Duration = Duration::from_secs(5);
/// After this many failed starts in a row, wait before trying again.
const MAX_FAILURES: u32 = 5;
const FAILURE_COOLDOWN: Duration = Duration::from_secs(5 * 60);

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
}

impl Library {
    pub fn new(downloads: Arc<Downloads>) -> Arc<Self> {
        let engine_dir = downloads.library_dir().join("bin").join("kiwix");
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
        })
    }

    pub fn state(&self) -> EngineState {
        *self.state.lock().unwrap_or_else(|p| p.into_inner())
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

    /// Installed knowledge packs, in catalog order.
    pub fn books(&self) -> Vec<Book> {
        let library = self.downloads.library_dir().to_path_buf();
        self.downloads
            .snapshot()
            .into_iter()
            .filter(|v| v.pack.category == Category::Knowledge && v.state.status == PackStatus::Installed)
            .flat_map(|v| {
                let library = library.clone();
                v.pack.files.clone().into_iter().filter_map(move |f| {
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
            .args(books.iter().map(|b| native_path(&b.file)))
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
        let query = query.trim();
        if query.is_empty() || self.state() != EngineState::Running {
            return Vec::new();
        }
        let books: Vec<Book> = self
            .books()
            .into_iter()
            .filter(|b| only_book.is_none_or(|n| n == b.name))
            .collect();

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
            let mut t = Vec::new();
            for q in &title_queries {
                t.extend(self.suggest(book, q, limit.min(8)).await);
            }
            // Exact title matches ("шећер" for "secer") first, keeping the rest in order.
            t.sort_by_key(|r| !wanted.contains(&translit::fold(&r.title)));
            let mut x = Vec::new();
            for q in &text_queries {
                x.extend(self.fulltext(book, q, limit).await);
            }
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
        let url = format!("{}{ROOT}/suggest", self.base());
        let res = self
            .http
            .get(url)
            .query(&[("content", book.name.as_str()), ("term", q), ("count", &limit.min(10).to_string())])
            .send()
            .await;
        let Ok(res) = res else { return Vec::new() };
        let Ok(items) = res.json::<Vec<serde_json::Value>>().await else { return Vec::new() };
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
            .collect()
    }

    async fn fulltext(&self, book: &Book, q: &str, limit: usize) -> Vec<SearchResult> {
        let url = format!("{}{ROOT}/search", self.base());
        let res = self
            .http
            .get(url)
            .query(&[
                ("books.name", book.name.as_str()),
                ("pattern", q),
                ("format", "xml"),
                ("pageLength", &limit.to_string()),
            ])
            .send()
            .await;
        let Ok(res) = res else { return Vec::new() };
        if !res.status().is_success() {
            return Vec::new();
        }
        let Ok(xml) = res.text().await else { return Vec::new() };
        parse_search_rss(&xml)
            .into_iter()
            .map(|(title, link, snippet)| SearchResult {
                title,
                url: link,
                snippet,
                book: book.name.clone(),
                book_title_en: book.title_en.clone(),
                book_title_sr: book.title_sr.clone(),
                kind: "text",
            })
            .collect()
    }
}

/// On Windows, child processes are placed in a job object that is closed when
/// the hub exits for any reason (even when killed), which makes Windows end
/// kiwix-serve too. Without this, an update or a crash leaves it running.
#[cfg(windows)]
mod job {
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
            // SAFETY: plain Win32 calls with valid, zero-initialised arguments.
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

/// kiwix-serve on Windows refuses paths written with forward slashes.
fn native_path(p: &Path) -> std::ffi::OsString {
    if cfg!(windows) {
        p.to_string_lossy().replace('/', "\\").into()
    } else {
        p.as_os_str().to_owned()
    }
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
    fn encodes_paths() {
        assert_eq!(encode_path("Vod._M."), "Vod._M.");
        assert_eq!(encode_path("вода"), "%D0%B2%D0%BE%D0%B4%D0%B0");
        assert_eq!(encode_path("A/b c"), "A/b%20c");
    }
}
