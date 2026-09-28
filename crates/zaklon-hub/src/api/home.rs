//! The household's home location: one place, kept on this hub for the whole
//! household. Any paired device may read it, and the laptop and phones may
//! set it (from a phone's GPS, a place found by name, or a tap on the map).
//! It is never sent anywhere: nothing on the hub talks to the internet about it.

use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use zaklon_core::dates::now_rfc3339;

use super::error::{bad, ApiError};
use super::{blocking, Caller};
use crate::gazetteer::{Place, MAX_RESULTS};
use crate::HubState;

const SETTING: &str = "home_location";
/// A place name is kept this short (it is only a reminder of where it is).
const LABEL_MAX: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(super) struct HomeLocation {
    pub lat: f64,
    pub lon: f64,
    /// What the place is called, when it was found by name ("Novi Sad, Vojvodina").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// How it was set: "gps", "place" or "map".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default)]
    pub set_at: String,
    /// Who set it: "laptop" or the phone's name.
    #[serde(default)]
    pub set_by: String,
}

#[derive(Serialize)]
pub(super) struct HomeReply {
    home: Option<HomeLocation>,
}

#[derive(Deserialize)]
pub(super) struct SetHome {
    lat: f64,
    lon: f64,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    source: Option<String>,
}

/// The home as kept, if there is one (a damaged value counts as none).
pub(super) fn read_home(state: &HubState) -> Result<Option<HomeLocation>, ApiError> {
    Ok(state.db.get_setting(SETTING)?.filter(|t| !t.is_empty()).and_then(|t| serde_json::from_str(&t).ok()))
}

pub(super) async fn home_get(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<HomeReply>, ApiError> {
    Ok(Json(HomeReply { home: read_home(&state)? }))
}

/// Round to about ten centimeters: more would only look precise.
fn round6(x: f64) -> f64 {
    (x * 1e6).round() / 1e6
}

pub(super) async fn home_set(State(state): State<Arc<HubState>>, caller: Caller, Json(body): Json<SetHome>) -> Result<Json<HomeReply>, ApiError> {
    if !body.lat.is_finite() || !(-90.0..=90.0).contains(&body.lat) {
        return Err(bad("a home location needs a latitude between -90 and 90"));
    }
    if !body.lon.is_finite() || !(-180.0..=180.0).contains(&body.lon) {
        return Err(bad("a home location needs a longitude between -180 and 180"));
    }
    let label = body
        .label
        .map(|l| l.chars().filter(|c| !c.is_control()).take(LABEL_MAX).collect::<String>().trim().to_string())
        .filter(|l| !l.is_empty());
    let source = body.source.filter(|s| ["gps", "place", "map"].contains(&s.as_str()));
    let home = HomeLocation { lat: round6(body.lat), lon: round6(body.lon), label, source, set_at: now_rfc3339(), set_by: caller.actor() };
    let text = serde_json::to_string(&home).map_err(|e| ApiError::from(anyhow::Error::new(e)))?;
    state.db.set_setting(SETTING, &text)?;
    tracing::info!(by = %home.set_by, "home location set");
    Ok(Json(HomeReply { home: Some(home) }))
}

pub(super) async fn home_clear(State(state): State<Arc<HubState>>, caller: Caller) -> Result<StatusCode, ApiError> {
    state.db.set_setting(SETTING, "")?;
    tracing::info!(by = %caller.actor(), "home location removed");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub(super) struct PlaceQuery {
    #[serde(default)]
    q: String,
}

/// "Find a place": towns and cities by name, from the list in the map
/// assets (none when the app came without it).
pub(super) async fn places_search(State(state): State<Arc<HubState>>, _caller: Caller, Query(query): Query<PlaceQuery>) -> Result<Json<Vec<Place>>, ApiError> {
    let tiles = state.tiles.clone();
    let q = query.q.chars().take(100).collect::<String>();
    blocking(move || tiles.gazetteer().map(|g| g.search(&q, MAX_RESULTS)).unwrap_or_default()).await.map(Json)
}
