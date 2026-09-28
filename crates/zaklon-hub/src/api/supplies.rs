//! The household's supplies: items and their batches, places, barcodes, the
//! shopping list, what waits to be put away, and the history of changes.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use zaklon_core::supplies::{ItemInput, Item};

use super::error::{bad, invalid, not_found, ApiError};
use super::Caller;
use crate::HubState;

fn not_found_item() -> ApiError {
    not_found("no such item")
}

pub(super) async fn supplies_summary(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<zaklon_core::supplies::Summary>, ApiError> {
    Ok(Json(state.db.supplies_summary()?))
}

pub(super) async fn items_list(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<Vec<Item>>, ApiError> {
    Ok(Json(state.db.list_items()?))
}

pub(super) async fn items_get(State(state): State<Arc<HubState>>, _caller: Caller, Path(id): Path<String>) -> Result<Json<Item>, ApiError> {
    state.db.get_item(&id)?.map(Json).ok_or_else(not_found_item)
}

pub(super) async fn items_create(State(state): State<Arc<HubState>>, caller: Caller, Json(body): Json<ItemInput>) -> Result<(StatusCode, Json<Item>), ApiError> {
    let item = state.db.create_item(body, &caller.actor()).map_err(invalid)?;
    Ok((StatusCode::CREATED, Json(item)))
}

pub(super) async fn items_update(State(state): State<Arc<HubState>>, caller: Caller, Path(id): Path<String>, Json(body): Json<ItemInput>) -> Result<Json<Item>, ApiError> {
    state.db.update_item(&id, body, &caller.actor()).map_err(invalid)?.map(Json).ok_or_else(not_found_item)
}

#[derive(Deserialize)]
pub(super) struct AdjustBody {
    delta: f64,
}

pub(super) async fn items_adjust(State(state): State<Arc<HubState>>, caller: Caller, Path(id): Path<String>, Json(body): Json<AdjustBody>) -> Result<Json<Item>, ApiError> {
    if !body.delta.is_finite() || body.delta == 0.0 {
        return Err(bad("delta must be a non-zero number"));
    }
    state.db.adjust_item(&id, body.delta, &caller.actor())?.map(Json).ok_or_else(not_found_item)
}

pub(super) async fn items_delete(State(state): State<Arc<HubState>>, caller: Caller, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    if state.db.delete_item(&id, &caller.actor())? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found_item())
    }
}

#[derive(Serialize)]
pub(super) struct BarcodeReply {
    barcode: String,
    /// An item in stock with this barcode, if any.
    item: Option<Item>,
    /// What this barcode was called before, if it was ever used.
    known: Option<zaklon_core::supplies::KnownBarcode>,
}

pub(super) async fn barcode_lookup(State(state): State<Arc<HubState>>, _caller: Caller, Path(code): Path<String>) -> Result<Json<BarcodeReply>, ApiError> {
    let code = code.trim().to_string();
    if code.is_empty() || code.len() > 64 {
        return Err(bad("bad barcode"));
    }
    Ok(Json(BarcodeReply {
        item: state.db.find_item_by_barcode(&code)?,
        known: state.db.lookup_barcode(&code)?,
        barcode: code,
    }))
}

pub(super) async fn places_list(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<Vec<zaklon_core::supplies::Place>>, ApiError> {
    Ok(Json(state.db.list_places()?))
}

#[derive(Deserialize)]
pub(super) struct NameBody {
    name: String,
}

pub(super) async fn places_add(State(state): State<Arc<HubState>>, _caller: Caller, Json(body): Json<NameBody>) -> Result<Json<zaklon_core::supplies::Place>, ApiError> {
    Ok(Json(state.db.add_place(&body.name).map_err(invalid)?))
}

pub(super) async fn places_delete(State(state): State<Arc<HubState>>, caller: Caller, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    if state.db.delete_place(&id, &caller.actor())? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found("no such place"))
    }
}

pub(super) async fn shopping_list(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<Vec<zaklon_core::supplies::ShoppingEntry>>, ApiError> {
    Ok(Json(state.db.shopping_list()?))
}

#[derive(Deserialize)]
pub(super) struct ShoppingBody {
    text: String,
    #[serde(default)]
    quantity: Option<f64>,
    #[serde(default)]
    unit: Option<String>,
    #[serde(default)]
    item_id: Option<String>,
    /// Set by phones: the same add sent twice (a reply lost on the way, or
    /// resent from the phone's outbox) makes one entry, also after a restart.
    #[serde(default)]
    client_id: Option<String>,
}

pub(super) async fn shopping_add(State(state): State<Arc<HubState>>, caller: Caller, Json(b): Json<ShoppingBody>) -> Result<(StatusCode, Json<zaklon_core::supplies::ShoppingEntry>), ApiError> {
    let e = match b.client_id.as_deref().filter(|c| !c.is_empty()) {
        Some(cid) => state.db.add_shopping_once(cid, &b.text, b.quantity, b.unit, b.item_id, &caller.actor()),
        None => state.db.add_shopping(&b.text, b.quantity, b.unit, b.item_id, &caller.actor()),
    }
    .map_err(invalid)?;
    Ok((StatusCode::CREATED, Json(e)))
}

/// "Bought": moves an entry (or a running-low suggestion "low:<item>") to put away.
pub(super) async fn shopping_bought(State(state): State<Arc<HubState>>, caller: Caller, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    if state.db.mark_bought(&id, &caller.actor())? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found("no such entry"))
    }
}

/// "Delete" on the shopping list: not bought.
pub(super) async fn shopping_dismiss(State(state): State<Arc<HubState>>, caller: Caller, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    if state.db.dismiss(&id, &caller.actor())? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found("no such entry"))
    }
}

pub(super) async fn put_away_list(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<Vec<zaklon_core::supplies::ShoppingEntry>>, ApiError> {
    Ok(Json(state.db.to_put_away()?))
}

pub(super) async fn put_away(
    State(state): State<Arc<HubState>>,
    caller: Caller,
    Path(id): Path<String>,
    Json(body): Json<zaklon_core::supplies::PutAwayInput>,
) -> Result<Json<Item>, ApiError> {
    state.db.put_away(&id, body, &caller.actor()).map_err(invalid)?.map(Json).ok_or_else(|| not_found("no such entry"))
}

pub(super) async fn batch_add(
    State(state): State<Arc<HubState>>,
    caller: Caller,
    Path(id): Path<String>,
    Json(body): Json<zaklon_core::supplies::BatchInput>,
) -> Result<Json<Item>, ApiError> {
    state.db.add_batch(&id, body, &caller.actor()).map_err(invalid)?.map(Json).ok_or_else(not_found_item)
}

pub(super) async fn batch_update(
    State(state): State<Arc<HubState>>,
    caller: Caller,
    Path(id): Path<String>,
    Json(body): Json<zaklon_core::supplies::BatchInput>,
) -> Result<Json<Item>, ApiError> {
    state.db.update_batch(&id, body, &caller.actor()).map_err(invalid)?.map(Json).ok_or_else(|| not_found("no such batch"))
}

pub(super) async fn batch_delete(State(state): State<Arc<HubState>>, caller: Caller, Path(id): Path<String>) -> Result<Json<Item>, ApiError> {
    state.db.delete_batch(&id, &caller.actor()).map_err(invalid)?.map(Json).ok_or_else(|| not_found("no such batch"))
}

#[derive(Deserialize)]
pub(super) struct HistoryQuery {
    #[serde(default)]
    limit: Option<usize>,
}

pub(super) async fn history(State(state): State<Arc<HubState>>, _caller: Caller, axum::extract::Query(q): axum::extract::Query<HistoryQuery>) -> Result<Json<Vec<zaklon_core::supplies::HistoryEntry>>, ApiError> {
    Ok(Json(state.db.history(q.limit.unwrap_or(100).clamp(1, 500))?))
}
