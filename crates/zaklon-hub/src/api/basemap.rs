//! The Zaklon map: its tiles (from the map archives, see `crate::tiles`),
//! the fonts and icons its style draws with, and what the app needs to know
//! to draw it. Everything comes from this hub; the map never asks the
//! internet for anything.

use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

use super::error::ApiError;
use super::home::{read_home, HomeLocation};
use super::Caller;
use crate::tiles::{self, Found};
use crate::HubState;

/// Tiles and style files do not change while their address stays the same
/// (the app puts the set of archives into the tiles' address).
const CACHE: &str = "public, max-age=604800";
const TILE_TYPE: &str = "application/vnd.mapbox-vector-tile";

/// A map pack of the catalog and where it stands, for the app's notes
/// ("download the world map", "checking the world map").
#[derive(Serialize)]
struct PackView {
    id: String,
    size: u64,
    status: zaklon_core::catalog::PackStatus,
    bytes_done: u64,
    bytes_total: u64,
}

#[derive(Serialize)]
pub(super) struct MapReply {
    tiles: tiles::Summary,
    /// The fonts for labels are on the hub.
    glyphs: bool,
    /// The icons of the map's style are on the hub.
    sprites: bool,
    /// The largest map pack of the catalog (the world map), if any.
    world: Option<PackView>,
    /// "Find a place" has its list of places.
    places: bool,
    /// The household's home, if it was set.
    home: Option<HomeLocation>,
}

pub(super) async fn map_info(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<MapReply>, ApiError> {
    let summary = state.tiles.summary().await;
    let assets = state.tiles.assets().map(PathBuf::from);
    let has = |rel: &str| assets.as_ref().is_some_and(|d| d.join(rel).exists());
    let world = state
        .downloads
        .catalog()
        .packs
        .iter()
        .filter(|p| crate::downloads::is_map_archive_pack(p))
        .max_by_key(|p| p.size)
        .map(|p| {
            let st = state.downloads.state_of(&p.id).unwrap_or_else(|| zaklon_core::catalog::PackState::not_installed(p.size));
            PackView { id: p.id.clone(), size: p.size, status: st.status, bytes_done: st.bytes_done, bytes_total: st.bytes_total }
        });
    Ok(Json(MapReply {
        tiles: summary,
        glyphs: has("fonts"),
        sprites: has("sprites"),
        world,
        places: has(crate::gazetteer::PLACES_FILE),
        home: read_home(&state)?,
    }))
}

/// `/tiles/{z}/{x}/{y}.mvt`: a vector tile, gzip-compressed as it is
/// stored. 204 for a tile no archive has inside the map's zoom levels (the
/// open sea), 404 outside them.
pub(super) async fn tile(
    State(state): State<Arc<HubState>>,
    _caller: Caller,
    Path((z, x, file)): Path<(u8, u32, String)>,
    headers: HeaderMap,
) -> Response {
    let Some(y) = file.strip_suffix(".mvt").and_then(|y| y.parse::<u32>().ok()) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match state.tiles.tile(z, x, y).await {
        Found::Tile(t) => {
            if not_modified(&headers, &t.etag) {
                return (StatusCode::NOT_MODIFIED, [(header::ETAG, t.etag), (header::CACHE_CONTROL, CACHE.to_string())]).into_response();
            }
            let mut res = (
                StatusCode::OK,
                [(header::CONTENT_TYPE, TILE_TYPE.to_string()), (header::CACHE_CONTROL, CACHE.to_string()), (header::ETAG, t.etag)],
                Body::from(t.data),
            )
                .into_response();
            if let Some(enc) = t.encoding {
                res.headers_mut().insert(header::CONTENT_ENCODING, HeaderValue::from_static(enc));
            }
            res
        }
        Found::Empty => (StatusCode::NO_CONTENT, [(header::CACHE_CONTROL, CACHE)]).into_response(),
        Found::Outside => StatusCode::NOT_FOUND.into_response(),
    }
}

/// `/map/fonts/{fontstack}/{start}-{end}.pbf`: the glyphs of a font.
pub(super) async fn glyphs(
    State(state): State<Arc<HubState>>,
    _caller: Caller,
    Path((fontstack, range)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let Some(dir) = state.tiles.assets() else { return StatusCode::NOT_FOUND.into_response() };
    let Some(path) = tiles::glyph_path(dir, &fontstack, &range) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // A font that has no glyphs of its own in this range (the Devanagari
    // font for Latin letters) is drawn with the regular one there.
    let path = match tiles::glyph_path(dir, tiles::FALLBACK_FONT, &range) {
        Some(fallback) if !path.is_file() && fallback.is_file() => fallback,
        _ => path,
    };
    static_file(path, "application/x-protobuf", &headers).await
}

/// `/map/sprites/{name}[@2x].json|png`: the style's icons.
pub(super) async fn sprite(State(state): State<Arc<HubState>>, _caller: Caller, Path(file): Path<String>, headers: HeaderMap) -> Response {
    let Some(path) = state.tiles.assets().and_then(|dir| tiles::sprite_path(dir, &file)) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let kind = if file.ends_with(".png") { "image/png" } else { "application/json" };
    static_file(path, kind, &headers).await
}

/// The request already has this version (If-None-Match).
fn not_modified(headers: &HeaderMap, etag: &str) -> bool {
    headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim() == etag || t.trim() == "*"))
}

/// A small file of the map assets, with an ETag from its size and time.
async fn static_file(path: PathBuf, kind: &'static str, headers: &HeaderMap) -> Response {
    let Ok(meta) = tokio::fs::metadata(&path).await else { return StatusCode::NOT_FOUND.into_response() };
    if !meta.is_file() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let stamp = meta.modified().ok().and_then(|m| m.duration_since(std::time::SystemTime::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    let etag = format!("\"{:x}-{:x}\"", meta.len(), stamp);
    if not_modified(headers, &etag) {
        return (StatusCode::NOT_MODIFIED, [(header::ETAG, etag), (header::CACHE_CONTROL, CACHE.to_string())]).into_response();
    }
    match tokio::fs::read(&path).await {
        Ok(bytes) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, kind.to_string()), (header::CACHE_CONTROL, CACHE.to_string()), (header::ETAG, etag)],
            Body::from(bytes),
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}
