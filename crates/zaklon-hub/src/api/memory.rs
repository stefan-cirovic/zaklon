//! The assistant's memory: what the household asked it to remember.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;

use super::error::{invalid, not_found, ApiError};
use super::Caller;
use crate::HubState;

pub(super) async fn memory_list(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<Vec<zaklon_core::memory::Note>>, ApiError> {
    Ok(Json(state.db.list_notes()?))
}

#[derive(Deserialize)]
pub(super) struct NoteBody {
    text: String,
}

pub(super) async fn memory_add(State(state): State<Arc<HubState>>, caller: Caller, Json(body): Json<NoteBody>) -> Result<(StatusCode, Json<zaklon_core::memory::Note>), ApiError> {
    let note = state.db.add_note(&body.text, &caller.actor()).map_err(invalid)?;
    Ok((StatusCode::CREATED, Json(note)))
}

pub(super) async fn memory_delete(State(state): State<Arc<HubState>>, caller: Caller, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    if state.db.delete_note(&id, &caller.actor())? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found("no such note"))
    }
}
