//! Packs (knowledge packs, maps, AI models and the programs they need),
//! the library they make, and installed AI models copied to phones.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

use super::error::{bad, forbidden, not_found, ApiError};
use super::{blocking, Caller, LibraryReader, Local};
use crate::HubState;

// ---- packs -------------------------------------------------------------------

#[derive(Serialize)]
pub(super) struct CatalogReply {
    packs: Vec<crate::downloads::PackView>,
    /// What a household starts with, by app language.
    starter_sets: Vec<zaklon_core::catalog::StarterSet>,
    system: crate::downloads::SystemInfo,
    /// The root of the drive the library is on ("D:\"), which Storage &
    /// Downloads shows as the hub's drive.
    library_drive: String,
    /// The world map: which build is offered, which one the hub has, and
    /// whether a newer one fits next to it.
    #[serde(skip_serializing_if = "Option::is_none")]
    world: Option<WorldReply>,
}

#[derive(Serialize)]
pub(super) struct WorldReply {
    #[serde(flatten)]
    view: crate::downloads::WorldView,
    /// When Protomaps' list of builds was last read (RFC 3339), if ever.
    checked_at: Option<String>,
}

pub(super) async fn catalog(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<CatalogReply>, ApiError> {
    let d = &state.downloads;
    // Pieces of CoMaps maps (over a thousand) have their own screen and
    // endpoint; the Zaklon map's packs (the world map) are listed here.
    d.notice_placed_maps();
    let packs = d.snapshot().into_iter().filter(|v| !v.pack.id.starts_with(zaklon_core::maps::MAP_ID_PREFIX)).collect();
    let world = d.world_view().map(|view| WorldReply { view, checked_at: state.world.checked_at() });
    Ok(Json(CatalogReply {
        packs,
        starter_sets: d.catalog().starter_sets.clone(),
        system: crate::downloads::system_info(d.library_dir()),
        library_drive: crate::machine::drive_root(d.library_dir()),
        world,
    }))
}

/// Maps or Storage & Downloads was opened, where the world map is offered:
/// the hub reads Protomaps' list of world map builds in the background when
/// it is due (at most about once a day, and only then goes online), and
/// offers the build it chooses.
pub(super) async fn world_check(State(state): State<Arc<HubState>>, _caller: Caller) -> Json<serde_json::Value> {
    let checking = state.world.check(&state.downloads);
    Json(serde_json::json!({ "checking": checking }))
}

#[derive(Deserialize)]
pub(super) struct WorldUpdateBody {
    /// The person confirmed that the old map is deleted first (there is no
    /// room for both).
    #[serde(default)]
    remove_old: bool,
}

/// Laptop only, like removing: update the world map to the newer build
/// offered (see `Downloads::update_world`).
pub(super) async fn world_update(State(state): State<Arc<HubState>>, _: Local, Json(body): Json<WorldUpdateBody>) -> Result<StatusCode, ApiError> {
    tracing::info!(remove_old = body.remove_old, "world map update asked for on the laptop");
    state.downloads.update_world(body.remove_old).await.map_err(|e| bad(&e))?;
    Ok(StatusCode::ACCEPTED)
}

pub(super) async fn system(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<crate::downloads::SystemInfo>, ApiError> {
    Ok(Json(crate::downloads::system_info(state.downloads.library_dir())))
}

#[derive(Deserialize)]
pub(super) struct DownloadQuery {
    /// The person confirmed the license of a pack they download themselves.
    #[serde(default)]
    accept_license: bool,
}

pub(super) async fn pack_download(
    State(state): State<Arc<HubState>>,
    caller: Caller,
    Path(id): Path<String>,
    axum::extract::Query(q): axum::extract::Query<DownloadQuery>,
) -> Result<StatusCode, ApiError> {
    // With a world map on the hub, a download of the world map is its
    // update: on the laptop only, like removing it, and only with room for
    // both maps (see world_update).
    if id == zaklon_core::world_map::WORLD_MAP_ID && !state.downloads.installed_files(&id).is_empty() {
        if !matches!(caller, Caller::Local) {
            return Err(forbidden("only the laptop can do this"));
        }
        state.downloads.update_world(false).await.map_err(|e| bad(&e))?;
        return Ok(StatusCode::ACCEPTED);
    }
    if let Some(pack) = state.downloads.catalog().pack(&id) {
        // A pack under a non-commercial or mixed license is only ever
        // downloaded by a person's own choice, once they confirmed its
        // license: never by a starter set or anything else that asks for many.
        if pack.offer == zaklon_core::catalog::Offer::User && !q.accept_license {
            return Err(bad("confirm the license of this pack first"));
        }
        // Knowledge packs need the library engine; queue it first if it is missing.
        if pack.category == zaklon_core::catalog::Category::Knowledge && state.downloads.needs_download("kiwix-tools") {
            let _ = state.downloads.enqueue("kiwix-tools");
        }
        // AI models need the AI engine.
        if pack.category == zaklon_core::catalog::Category::Model && state.downloads.needs_download("llama-cpp") {
            let _ = state.downloads.enqueue("llama-cpp");
        }
        // A piece of a CoMaps map: CoMaps needs the world overview first, phones need the app.
        if pack.id.starts_with(zaklon_core::maps::MAP_ID_PREFIX) {
            for dep in zaklon_core::maps::BASE_IDS.into_iter().chain([zaklon_core::maps::COMAPS_APK_ID]) {
                if dep != id && state.downloads.needs_download(dep) {
                    let _ = state.downloads.enqueue(dep);
                }
            }
        }
    }
    state.downloads.enqueue(&id).map_err(|e| bad(&e))?;
    Ok(StatusCode::ACCEPTED)
}

pub(super) async fn pack_pause(State(state): State<Arc<HubState>>, _caller: Caller, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    state.downloads.pause(&id).map_err(|e| bad(&e))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Laptop only: packs are often tens of GB and cannot be downloaded again
/// without internet, so a phone may not delete them.
pub(super) async fn pack_remove(State(state): State<Arc<HubState>>, _: Local, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    tracing::info!(pack = %id, "pack removed on the laptop");
    // The library engine keeps knowledge packs (and its own files) open, the
    // AI engine its model and its own files; stop the one concerned so
    // Windows lets us delete them. It starts again by itself.
    // Also a pack the catalog no longer offers: deleting it is the person's choice.
    if let Some(pack) = state.downloads.pack(&id) {
        state.downloads.release(&pack).await;
    }
    let d = state.downloads.clone();
    blocking(move || d.remove(&id)).await?.map_err(|e| bad(&e))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub(super) struct DirBody {
    dir: String,
}

pub(super) async fn packs_import(State(state): State<Arc<HubState>>, _: Local, Json(body): Json<DirBody>) -> Result<Json<serde_json::Value>, ApiError> {
    let dir = std::path::PathBuf::from(body.dir.trim());
    if !dir.is_dir() {
        return Err(bad("that folder does not exist"));
    }
    // Only finds and queues the packs; they are copied in the background with progress.
    let d = state.downloads.clone();
    let importing = blocking(move || d.import_from_dir(&dir)).await?.map_err(|e| bad(&e))?;
    Ok(Json(serde_json::json!({ "importing": importing })))
}

/// Drives of the laptop, for copying packs to and from USB.
pub(super) async fn drives(_: Local) -> Result<Json<Vec<crate::machine::Drive>>, ApiError> {
    Ok(Json(blocking(crate::machine::drives).await?))
}

pub(super) async fn hardware(_caller: Caller) -> Result<Json<crate::machine::Hardware>, ApiError> {
    Ok(Json(blocking(crate::machine::hardware).await?))
}

// ---- library ----------------------------------------------------------------

#[derive(Serialize)]
pub(super) struct LibraryReply {
    engine: crate::kiwix::EngineState,
    books: Vec<crate::kiwix::Book>,
}

pub(super) async fn library_books(State(state): State<Arc<HubState>>, _caller: Caller) -> Json<LibraryReply> {
    Json(LibraryReply { engine: state.library.state(), books: state.library.books() })
}

#[derive(Deserialize)]
pub(super) struct SearchQuery {
    q: String,
    #[serde(default)]
    book: Option<String>,
    /// Only the books about this topic (a topic page of the Library).
    #[serde(default)]
    topic: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

pub(super) async fn library_search(
    State(state): State<Arc<HubState>>,
    _caller: Caller,
    axum::extract::Query(q): axum::extract::Query<SearchQuery>,
) -> Json<Vec<crate::kiwix::SearchResult>> {
    let limit = q.limit.unwrap_or(25).clamp(1, 50);
    let books = state.library.books();
    let books = match q.topic.as_deref().map(zaklon_core::catalog::upgraded_topic) {
        Some(topic) => books.into_iter().filter(|b| b.topics.iter().any(|t| zaklon_core::catalog::upgraded_topic(t) == topic)).collect(),
        None => books,
    };
    Json(state.library.search_books(books, &q.q, q.book.as_deref(), limit).await)
}

/// Articles, images and styles of installed knowledge packs, read-only.
pub(super) async fn kiwix_proxy(State(state): State<Arc<HubState>>, _reader: LibraryReader, uri: axum::http::Uri) -> Response {
    let path = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
    if !path.starts_with("/kiwix/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    proxy_library(&state, path, false).await
}

/// The same pages with Serbian Cyrillic text shown in Latin script. Relative
/// links inside the page stay under /kiwix-lat/, so reading on stays in Latin.
pub(super) async fn kiwix_proxy_latin(State(state): State<Arc<HubState>>, _reader: LibraryReader, uri: axum::http::Uri) -> Response {
    let path = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
    let Some(rest) = path.strip_prefix("/kiwix-lat/") else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // Only pages of Serbian books are converted.
    let book = rest.strip_prefix("content/").and_then(|r| r.split('/').next()).unwrap_or_default();
    let serbian = state
        .library
        .books()
        .iter()
        .any(|b| b.name == book && b.languages.iter().any(|l| l == "srp"));
    proxy_library(&state, &format!("/kiwix/{rest}"), serbian).await
}

async fn proxy_library(state: &Arc<HubState>, path: &str, to_latin: bool) -> Response {
    match state.library.fetch(path).await {
        Ok(res) => {
            let status = StatusCode::from_u16(res.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let ctype = res
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("application/octet-stream")
                .to_string();
            let is_html = ctype.starts_with("text/html");
            let cache = if is_html { "no-cache" } else { "private, max-age=86400" };
            match res.bytes().await {
                Ok(body) => {
                    let body = if to_latin && is_html {
                        match std::str::from_utf8(&body) {
                            Ok(text) => {
                                let converted = crate::latin::html_to_latin(text);
                                axum::body::Bytes::from(converted)
                            }
                            Err(_) => body,
                        }
                    } else {
                        body
                    };
                    (
                    status,
                    [
                        (axum::http::header::CONTENT_TYPE, ctype),
                        (axum::http::header::CACHE_CONTROL, cache.to_string()),
                        (axum::http::header::HeaderName::from_static("x-content-type-options"), "nosniff".to_string()),
                        // Library pages never run scripts and get a unique origin,
                        // even if another website opens them in a new tab.
                        (axum::http::header::CONTENT_SECURITY_POLICY, "sandbox allow-popups".to_string()),
                    ],
                    body,
                )
                    .into_response()
                }
                Err(_) => StatusCode::BAD_GATEWAY.into_response(),
            }
        }
        Err(_) => (StatusCode::SERVICE_UNAVAILABLE, "library engine is not running").into_response(),
    }
}

// ---- AI models for phones -----------------------------------------------------

#[derive(Serialize)]
pub(super) struct ModelInfo {
    id: String,
    title_en: String,
    title_sr: String,
    file: String,
    size: u64,
    sha256: String,
}

/// Installed AI models a phone can copy from the hub. What is on disk counts
/// (with its own checksum), which may be an older version than the catalog's.
pub(super) async fn models_list(State(state): State<Arc<HubState>>, _caller: Caller) -> Json<Vec<ModelInfo>> {
    let models = state
        .downloads
        .snapshot()
        .into_iter()
        .filter(|v| v.pack.category == zaklon_core::catalog::Category::Model)
        .filter_map(|v| {
            let f = v.state.files.first()?.clone();
            Some(ModelInfo {
                id: v.pack.id.clone(),
                title_en: v.pack.title.en.clone(),
                title_sr: v.pack.title.sr.clone(),
                file: std::path::Path::new(&f.path).file_name()?.to_string_lossy().to_string(),
                size: f.size,
                sha256: f.sha256.clone(),
            })
        })
        .collect();
    Json(models)
}

/// Stream an installed model file, with `Range: bytes=N-` support so a
/// phone can resume a large copy after the Wi-Fi drops.
pub(super) async fn model_file(
    State(state): State<Arc<HubState>>,
    _caller: Caller,
    Path(id): Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<Response, ApiError> {
    let pack = state.downloads.catalog().pack(&id).cloned().ok_or_else(|| not_found("no such model"))?;
    let files = state.downloads.installed_files(&id);
    if pack.category != zaklon_core::catalog::Category::Model || files.is_empty() {
        return Err(not_found("model is not installed on the hub"));
    }
    // The file on disk, with its own checksum (the catalog may list a newer one).
    stream_library_file(&state, &files[0], &headers, "application/octet-stream").await
}

/// What a request's `Range` header asks of a file.
#[derive(Debug, PartialEq, Eq)]
enum Wanted {
    /// The whole file (no range asked for).
    Whole,
    /// Bytes `first..=last` of the file.
    Part { first: u64, last: u64 },
    /// A range that cannot be served.
    Unsatisfiable,
}

/// What a `Range` header asks of a file of `total` bytes. One range of bytes
/// is served: `bytes=N-` (from N to the end), `bytes=N-M` (N to M, or to the
/// end if the file is shorter) and `bytes=-N` (the last N bytes, or the whole
/// file if it is shorter). Anything else in bytes cannot be served: several
/// ranges, a range that starts past the end, or one that is malformed. A
/// range in another unit is ignored, as HTTP asks.
fn wanted_range(range: Option<&axum::http::HeaderValue>, total: u64) -> Wanted {
    let Some(range) = range else { return Wanted::Whole };
    let Ok(range) = range.to_str() else { return Wanted::Unsatisfiable };
    let range = range.trim();
    let Some(spec) = range.get(..6).filter(|unit| unit.eq_ignore_ascii_case("bytes=")).map(|_| &range[6..]) else {
        return Wanted::Whole;
    };
    let Some((from, to)) = spec.split_once('-') else { return Wanted::Unsatisfiable };
    let (from, to) = (from.trim(), to.trim());
    let number = |s: &str| if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) { s.parse::<u64>().ok() } else { None };
    let end = total.saturating_sub(1);
    let (first, last) = match (number(from), number(to)) {
        (Some(first), None) if to.is_empty() => (first, end),
        (Some(first), Some(last)) if first <= last => (first, last.min(end)),
        (None, Some(suffix)) if from.is_empty() && suffix > 0 => (total.saturating_sub(suffix), end),
        _ => return Wanted::Unsatisfiable,
    };
    if first < total {
        Wanted::Part { first, last }
    } else {
        Wanted::Unsatisfiable
    }
}

/// Stream a verified file from the library with its SHA-256 (of the file
/// on disk) and support for one `Range` (see `wanted_range`), so a phone can
/// resume a large copy.
pub(super) async fn stream_library_file(
    state: &HubState,
    f: &zaklon_core::catalog::InstalledFile,
    headers: &axum::http::HeaderMap,
    content_type: &str,
) -> Result<Response, ApiError> {
    use axum::http::header;
    use tokio::io::{AsyncReadExt, AsyncSeekExt};

    let path = state.downloads.library_dir().join(&f.path);
    let mut file = tokio::fs::File::open(&path).await.map_err(|e| anyhow::anyhow!(e))?;
    let total = file.metadata().await.map_err(|e| anyhow::anyhow!(e))?.len();
    let name = std::path::Path::new(&f.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();

    let (status, first, len) = match wanted_range(headers.get(header::RANGE), total) {
        Wanted::Unsatisfiable => {
            return Ok((StatusCode::RANGE_NOT_SATISFIABLE, [(header::CONTENT_RANGE, format!("bytes */{total}"))]).into_response());
        }
        Wanted::Whole => (StatusCode::OK, 0, total),
        Wanted::Part { first, last } => (StatusCode::PARTIAL_CONTENT, first, last - first + 1),
    };
    if first > 0 {
        file.seek(std::io::SeekFrom::Start(first)).await.map_err(|e| anyhow::anyhow!(e))?;
    }
    // Only the bytes asked for, even if a range ends before the file does.
    let body = axum::body::Body::from_stream(tokio_util::io::ReaderStream::with_capacity(file.take(len), 1 << 20));
    let mut response = (
        status,
        [
            (header::CONTENT_TYPE, content_type.to_string()),
            (header::CONTENT_LENGTH, len.to_string()),
            (header::ACCEPT_RANGES, "bytes".to_string()),
            (header::HeaderName::from_static("x-zaklon-file"), name),
            (header::HeaderName::from_static("x-zaklon-sha256"), f.sha256.clone()),
        ],
        body,
    )
        .into_response();
    if status == StatusCode::PARTIAL_CONTENT {
        let range = format!("bytes {first}-{}/{total}", first + len - 1);
        response.headers_mut().insert(header::CONTENT_RANGE, header::HeaderValue::from_str(&range).expect("digits make a header value"));
    }
    Ok(response)
}

#[cfg(test)]
mod range_tests {
    use super::*;
    use axum::http::HeaderValue;

    fn wanted(range: &str, total: u64) -> Wanted {
        wanted_range(Some(&HeaderValue::from_str(range).unwrap()), total)
    }

    #[test]
    fn one_range_of_bytes_is_served() {
        let part = |first, last| Wanted::Part { first, last };
        assert_eq!(wanted_range(None, 10), Wanted::Whole);
        // From a byte to the end: resuming a copy.
        assert_eq!(wanted("bytes=4-", 10), part(4, 9));
        assert_eq!(wanted("bytes=0-", 10), part(0, 9));
        // From a byte to a byte: only up to it, or to the end of a shorter file.
        assert_eq!(wanted("bytes=2-5", 10), part(2, 5));
        assert_eq!(wanted("bytes=3-3", 10), part(3, 3));
        assert_eq!(wanted("bytes=6-100", 10), part(6, 9));
        assert_eq!(wanted("Bytes= 2 - 5 ", 10), part(2, 5), "the unit in any case, spaces around the numbers");
        // The last bytes, or all of a shorter file.
        assert_eq!(wanted("bytes=-3", 10), part(7, 9));
        assert_eq!(wanted("bytes=-10", 10), part(0, 9));
        assert_eq!(wanted("bytes=-50", 10), part(0, 9));
        // Another unit is ignored.
        assert_eq!(wanted("items=0-5", 10), Wanted::Whole);
    }

    #[test]
    fn a_range_that_cannot_be_served_is_refused() {
        for range in [
            "bytes=10-",
            "bytes=10-12",
            "bytes=5-2",
            "bytes=-0",
            "bytes=-",
            "bytes=",
            "bytes=5",
            "bytes=x-",
            "bytes=+1-2",
            "bytes=0-5,7-8",
            "bytes=99999999999999999999999-",
        ] {
            assert_eq!(wanted(range, 10), Wanted::Unsatisfiable, "{range}");
        }
        assert_eq!(wanted("bytes=0-", 0), Wanted::Unsatisfiable, "an empty file has no bytes to send");
        assert_eq!(wanted("bytes=-5", 0), Wanted::Unsatisfiable);
    }
}
