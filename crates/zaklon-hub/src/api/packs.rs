//! Add-ons (knowledge packs, maps, AI models and the programs they need),
//! the library they make, and installed AI models copied to phones.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

use super::error::{bad, not_found, ApiError};
use super::{blocking, Caller, LibraryReader, Local};
use crate::HubState;

// ---- add-ons ----------------------------------------------------------------

#[derive(Serialize)]
pub(super) struct CatalogReply {
    packs: Vec<crate::downloads::PackView>,
    system: crate::downloads::SystemInfo,
    /// The root of the drive the library is on ("D:\"), which the Add-ons
    /// screen shows as the hub's drive.
    library_drive: String,
}

pub(super) async fn catalog(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<CatalogReply>, ApiError> {
    let d = &state.downloads;
    // Map pieces (over a thousand) have their own screen and endpoint.
    let packs = d.snapshot().into_iter().filter(|v| v.pack.category != zaklon_core::catalog::Category::Maps).collect();
    Ok(Json(CatalogReply { packs, system: crate::downloads::system_info(d.library_dir()), library_drive: crate::machine::drive_root(d.library_dir()) }))
}

pub(super) async fn system(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<crate::downloads::SystemInfo>, ApiError> {
    Ok(Json(crate::downloads::system_info(state.downloads.library_dir())))
}

pub(super) async fn pack_download(State(state): State<Arc<HubState>>, _caller: Caller, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    // Knowledge packs need the library engine; queue it first if it is missing.
    if let Some(pack) = state.downloads.catalog().pack(&id) {
        if pack.category == zaklon_core::catalog::Category::Knowledge && state.downloads.needs_download("kiwix-tools") {
            let _ = state.downloads.enqueue("kiwix-tools");
        }
        // AI models need the AI engine.
        if pack.category == zaklon_core::catalog::Category::Model && state.downloads.needs_download("llama-cpp") {
            let _ = state.downloads.enqueue("llama-cpp");
        }
        // A map piece: CoMaps needs the world overview first, phones need the app.
        if pack.category == zaklon_core::catalog::Category::Maps {
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
    if let Some(pack) = state.downloads.catalog().pack(&id).cloned() {
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
    #[serde(default)]
    limit: Option<usize>,
}

pub(super) async fn library_search(
    State(state): State<Arc<HubState>>,
    _caller: Caller,
    axum::extract::Query(q): axum::extract::Query<SearchQuery>,
) -> Json<Vec<crate::kiwix::SearchResult>> {
    let limit = q.limit.unwrap_or(25).clamp(1, 50);
    Json(state.library.search(&q.q, q.book.as_deref(), limit).await)
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

/// Stream a verified file from the library with its SHA-256 (of the file
/// on disk) and `Range: bytes=N-` support.
pub(super) async fn stream_library_file(
    state: &HubState,
    f: &zaklon_core::catalog::InstalledFile,
    headers: &axum::http::HeaderMap,
    content_type: &str,
) -> Result<Response, ApiError> {
    use axum::http::header;
    use tokio::io::AsyncSeekExt;

    let path = state.downloads.library_dir().join(&f.path);
    let mut file = tokio::fs::File::open(&path).await.map_err(|e| anyhow::anyhow!(e))?;
    let total = file.metadata().await.map_err(|e| anyhow::anyhow!(e))?.len();

    let start = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .and_then(|v| v.split('-').next())
        .and_then(|v| v.trim().parse::<u64>().ok());
    let name = std::path::Path::new(&f.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();

    match start {
        Some(from) if from >= total => Ok((StatusCode::RANGE_NOT_SATISFIABLE, [(header::CONTENT_RANGE, format!("bytes */{total}"))]).into_response()),
        Some(from) => {
            file.seek(std::io::SeekFrom::Start(from)).await.map_err(|e| anyhow::anyhow!(e))?;
            let body = axum::body::Body::from_stream(tokio_util::io::ReaderStream::with_capacity(file, 1 << 20));
            Ok((
                StatusCode::PARTIAL_CONTENT,
                [
                    (header::CONTENT_TYPE, content_type.to_string()),
                    (header::CONTENT_LENGTH, (total - from).to_string()),
                    (header::CONTENT_RANGE, format!("bytes {from}-{}/{total}", total - 1)),
                    (header::ACCEPT_RANGES, "bytes".to_string()),
                    (header::HeaderName::from_static("x-zaklon-file"), name),
                    (header::HeaderName::from_static("x-zaklon-sha256"), f.sha256.clone()),
                ],
                body,
            )
                .into_response())
        }
        None => {
            let body = axum::body::Body::from_stream(tokio_util::io::ReaderStream::with_capacity(file, 1 << 20));
            Ok((
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, content_type.to_string()),
                    (header::CONTENT_LENGTH, total.to_string()),
                    (header::ACCEPT_RANGES, "bytes".to_string()),
                    (header::HeaderName::from_static("x-zaklon-file"), name),
                    (header::HeaderName::from_static("x-zaklon-sha256"), f.sha256.clone()),
                ],
                body,
            )
                .into_response())
        }
    }
}
