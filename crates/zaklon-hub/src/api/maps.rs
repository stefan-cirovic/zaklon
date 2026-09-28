//! Offline maps: the countries and their pieces for CoMaps, and the CoMaps
//! app for paired phones.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Response,
    Json,
};
use serde::Serialize;

use super::error::{bad, not_found, ApiError};
use super::packs::stream_library_file;
use super::{blocking, Caller, Local};
use crate::HubState;

#[derive(Serialize)]
struct MapRegionView {
    id: String,
    name: String,
    name_sr: String,
    size: u64,
    status: zaklon_core::catalog::PackStatus,
    bytes_done: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// An older map version is on the hub.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    update: bool,
}

#[derive(Serialize)]
struct MapCountryView {
    id: String,
    name: String,
    name_sr: String,
    size: u64,
    regions: Vec<MapRegionView>,
}

#[derive(Serialize)]
pub(super) struct MapsReply {
    version: u64,
    /// What to type into CoMaps as the map download server.
    server_urls: Vec<String>,
    /// Where phones download the CoMaps app from the hub, once it is on the hub.
    app_urls: Vec<String>,
    app: zaklon_core::catalog::PackState,
    installed_bytes: u64,
    countries: Vec<MapCountryView>,
}

pub(super) async fn maps_overview(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<MapsReply>, ApiError> {
    use zaklon_core::catalog::{PackState, PackStatus};
    use zaklon_core::maps;
    let tree = maps::tree();
    let states: std::collections::HashMap<String, PackState> =
        state.downloads.snapshot().into_iter().map(|v| (v.pack.id, v.state)).collect();
    let cfg = state.config();
    // Addresses phones can reach; a virtual adapter's address would only confuse.
    let hosts: Vec<String> = crate::discovery::shown_ipv4_addresses().iter().map(|a| a.to_string()).collect();
    let mut installed_bytes = 0;
    let countries = tree
        .countries
        .iter()
        .map(|c| MapCountryView {
            id: c.id.clone(),
            name: maps::local_name(&c.id, "en"),
            name_sr: maps::local_name(&c.id, "sr"),
            size: c.size,
            regions: c
                .regions
                .iter()
                .map(|r| {
                    let st = states.get(&format!("{}{}", maps::MAP_ID_PREFIX, r.id)).cloned().unwrap_or_else(|| PackState::not_installed(r.size));
                    if st.status == PackStatus::Installed {
                        installed_bytes += r.size;
                    }
                    MapRegionView {
                        id: r.id.clone(),
                        name: maps::local_name(&r.id, "en"),
                        name_sr: maps::local_name(&r.id, "sr"),
                        size: r.size,
                        status: st.status,
                        bytes_done: st.bytes_done,
                        error: st.error,
                        update: st.update_available,
                    }
                })
                .collect(),
        })
        .collect();
    for b in &tree.base {
        if states.get(&format!("{}{}", maps::MAP_ID_PREFIX, b.id)).is_some_and(|s| s.status == PackStatus::Installed) {
            installed_bytes += b.size;
        }
    }
    Ok(Json(MapsReply {
        version: tree.version,
        server_urls: hosts.iter().map(|h| format!("http://{h}:{}/", cfg.install_port)).collect(),
        app_urls: hosts.iter().map(|h| format!("http://{h}:{}/apk/comaps.apk", cfg.install_port)).collect(),
        app: states.get(zaklon_core::maps::COMAPS_APK_ID).cloned().unwrap_or_else(|| PackState::not_installed(0)),
        installed_bytes,
        countries,
    }))
}

fn country_regions(country: &str) -> Option<Vec<String>> {
    zaklon_core::maps::tree()
        .countries
        .iter()
        .find(|c| c.id == country)
        .map(|c| c.regions.iter().map(|r| format!("{}{}", zaklon_core::maps::MAP_ID_PREFIX, r.id)).collect())
}

/// Download every piece of a country (and the CoMaps app, the first time).
pub(super) async fn maps_country_download(State(state): State<Arc<HubState>>, _caller: Caller, Path(country): Path<String>) -> Result<StatusCode, ApiError> {
    let ids = country_regions(&country).ok_or_else(|| not_found("no such country"))?;
    // CoMaps needs the world overview first, and phones need the app.
    // Pieces of an older map version are downloaded again too.
    for dep in zaklon_core::maps::BASE_IDS.into_iter().chain([zaklon_core::maps::COMAPS_APK_ID]) {
        if state.downloads.needs_download(dep) {
            let _ = state.downloads.enqueue(dep);
        }
    }
    for id in ids {
        if state.downloads.needs_download(&id) {
            state.downloads.enqueue(&id).map_err(|e| bad(&e))?;
        }
    }
    Ok(StatusCode::ACCEPTED)
}

/// Laptop only, like removing a pack.
pub(super) async fn maps_country_remove(State(state): State<Arc<HubState>>, _: Local, Path(country): Path<String>) -> Result<StatusCode, ApiError> {
    let ids = country_regions(&country).ok_or_else(|| not_found("no such country"))?;
    tracing::info!(country = %country, "maps removed on the laptop");
    for id in ids {
        let st = state.downloads.state_of(&id).map(|s| s.status);
        let active = |s: Option<zaklon_core::catalog::PackStatus>| {
            matches!(s, Some(zaklon_core::catalog::PackStatus::Downloading | zaklon_core::catalog::PackStatus::Verifying))
        };
        if active(st) {
            let _ = state.downloads.pause(&id);
            // Wait until the download has really stopped and let go of its file.
            let deadline = std::time::Instant::now() + Duration::from_secs(15);
            while active(state.downloads.state_of(&id).map(|s| s.status)) && std::time::Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
        let d = state.downloads.clone();
        blocking(move || d.remove(&id)).await?.map_err(|e| bad(&e))?;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// The CoMaps app for a paired phone, over the pinned TLS connection with
/// its SHA-256, so the phone can check it before offering to install it
/// (the plain-HTTP install page is only for phones that are not paired).
pub(super) async fn maps_app_file(State(state): State<Arc<HubState>>, _caller: Caller, headers: axum::http::HeaderMap) -> Result<Response, ApiError> {
    let id = zaklon_core::maps::COMAPS_APK_ID;
    if !state.downloads.is_installed(id) {
        return Err(not_found("the map app is not on the hub yet"));
    }
    let files = state.downloads.installed_files(id);
    let f = files.first().filter(|f| !f.sha256.is_empty()).ok_or_else(|| not_found("no file"))?;
    stream_library_file(&state, f, &headers, "application/vnd.android.package-archive").await
}
