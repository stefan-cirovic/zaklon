//! The assistant: questions, their answers and the model, and the saved
//! conversations each device keeps on the hub.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;

use super::error::{bad, invalid, not_found, ApiError};
use super::{blocking, Caller};
use crate::HubState;

// ---- assistant --------------------------------------------------------------------

pub(super) async fn assistant_overview(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<crate::assistant::Overview>, ApiError> {
    let a = state.assistant.clone();
    Ok(Json(blocking(move || a.overview()).await?))
}

#[derive(Deserialize)]
pub(super) struct SelectModelBody {
    id: String,
}

pub(super) async fn assistant_select(State(state): State<Arc<HubState>>, _caller: Caller, Json(body): Json<SelectModelBody>) -> Result<StatusCode, ApiError> {
    state.assistant.select(&body.id).map_err(|e| bad(&e))?;
    state.db.set_setting(crate::assistant::SETTING_MODEL, &body.id)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub(super) struct AskBody {
    question: String,
    #[serde(default)]
    language: String,
    /// The conversation so far, from an app that does not save conversations
    /// on the hub; a saved conversation's own turns are used instead.
    #[serde(default)]
    history: Vec<crate::assistant::Turn>,
    /// Look things up on the internet too (switched on per conversation).
    #[serde(default)]
    online: bool,
    /// The saved conversation (this device's) the question continues.
    #[serde(default)]
    conversation: Option<String>,
    /// Save the question in a new conversation, titled from it.
    #[serde(default)]
    new_conversation: bool,
}

/// How many earlier answers of a saved conversation go with a question (as
/// many as the assistant uses).
const HISTORY_TURNS: usize = 2;

pub(super) async fn assistant_ask(State(state): State<Arc<HubState>>, caller: Caller, Json(body): Json<AskBody>) -> Result<Json<serde_json::Value>, ApiError> {
    let owner = caller.owner();
    // A saved conversation brings its own history. It must be this device's
    // and have room for one more question.
    let history = match body.conversation.as_deref() {
        Some(id) => {
            let conv = state.db.get_conversation(&owner, id)?.ok_or_else(|| not_found("no such conversation"))?;
            if conv.turns.len() >= zaklon_core::conversations::MAX_TURNS {
                return Err(bad("the conversation is too long; start a new one"));
            }
            let answered: Vec<_> = conv.turns.iter().filter(|t| t.status == "done" && !t.answer.is_empty()).collect();
            answered[answered.len().saturating_sub(HISTORY_TURNS)..]
                .iter()
                .map(|t| crate::assistant::Turn { question: t.question.clone(), answer: t.answer.clone() })
                .collect()
        }
        None => body.history,
    };
    // The assistant can answer about the supplies and propose changes to them.
    // It reads places by name, and a database failure fails the question:
    // an empty list would be answered as "nothing in the supplies", and the
    // household's notes (allergies) would be left out.
    // Place names in the language the answer will be in (as `Assistant::ask` picks it).
    let answer_language = zaklon_core::lang::question_language(&body.question).unwrap_or(if body.language == "sr" { "sr" } else { "en" });
    let items = state.db.list_items_for_reading(answer_language)?;
    let notes = state.db.list_notes()?;
    let ctx = crate::assistant::AskContext { history, items, notes, online: body.online };
    let id = state.assistant.ask(&body.question, &body.language, ctx).map_err(|e| bad(&e))?;
    tracing::info!(by = %caller.actor(), online = body.online, "assistant asked");
    if body.conversation.is_none() && !body.new_conversation {
        return Ok(Json(serde_json::json!({ "id": id })));
    }
    let (conversation, turn) = match state.db.add_turn(&owner, body.conversation.as_deref(), &body.question, &id) {
        Ok(saved) => saved,
        Err(e) => {
            // Not saved, so not answered either.
            state.assistant.cancel(&id);
            return Err(invalid(e));
        }
    };
    save_when_finished(state.clone(), id.clone());
    Ok(Json(serde_json::json!({ "id": id, "conversation": conversation, "turn": turn })))
}

/// An answer so far. Asking for it tells the hub that somebody still waits
/// for it; a question nobody asks about gives way to the next one.
pub(super) async fn assistant_answer(State(state): State<Arc<HubState>>, _caller: Caller, Path(id): Path<String>) -> Result<Json<crate::assistant::Answer>, ApiError> {
    state.assistant.poll(&id).map(Json).ok_or_else(|| not_found("no such answer"))
}

/// Stop an answer that waits or is being written; what was written so far stays.
pub(super) async fn assistant_cancel(State(state): State<Arc<HubState>>, _caller: Caller, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    if state.assistant.cancel(&id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found("no such answer"))
    }
}

#[derive(Deserialize, Default)]
pub(super) struct WarmBody {
    #[serde(default)]
    language: String,
}

pub(super) async fn assistant_warm(State(state): State<Arc<HubState>>, _caller: Caller, body: Option<Json<WarmBody>>) -> StatusCode {
    let lang = body.map(|b| b.0.language).unwrap_or_default();
    state.assistant.warm_up(&lang);
    StatusCode::ACCEPTED
}

pub(super) async fn assistant_stop(State(state): State<Arc<HubState>>, _caller: Caller) -> StatusCode {
    state.assistant.stop().await;
    StatusCode::NO_CONTENT
}

// ---- saved conversations -------------------------------------------------------------
//
// Each device sees only its own (see `Caller::owner`): another device's
// conversation is "no such conversation", whatever is asked of it.

/// How often an asked question's answer is looked at until it is finished,
/// and for how long at most (it may wait for the questions asked before it).
const SAVE_EVERY: Duration = Duration::from_millis(500);
const SAVE_WITHIN: Duration = Duration::from_secs(2 * 60 * 60);
/// The error of a turn whose answer the hub no longer has (it was restarted).
const ANSWER_LOST: &str = "the hub stopped before the answer was finished";

fn answer_finished(a: &crate::assistant::Answer) -> bool {
    matches!(a.status, crate::assistant::AnswerStatus::Done | crate::assistant::AnswerStatus::Failed)
}

/// Save a finished answer in the turn waiting for it; true when one waited.
fn save_answer(db: &zaklon_core::Db, a: &crate::assistant::Answer) -> bool {
    let all = serde_json::to_value(a).unwrap_or_default();
    // What the app shows with an answer, besides its text and sources.
    let mut details = serde_json::Map::new();
    for key in ["grounded", "cited", "fixed", "from_supplies", "used_internet", "safety", "searched", "proposal", "language", "tokens_per_second"] {
        if let Some(v) = all.get(key) {
            details.insert(key.into(), v.clone());
        }
    }
    let done = zaklon_core::conversations::Finished {
        failed: a.status == crate::assistant::AnswerStatus::Failed,
        answer: a.text.clone(),
        error: a.error.clone(),
        sources: all.get("sources").cloned().unwrap_or_default(),
        details: serde_json::Value::Object(details),
    };
    db.finish_turn(&a.id, &done).unwrap_or_else(|e| {
        tracing::warn!("saving an answer in its conversation: {e:#}");
        false
    })
}

fn lost_answer() -> zaklon_core::conversations::Finished {
    zaklon_core::conversations::Finished { failed: true, error: Some(ANSWER_LOST.into()), ..Default::default() }
}

/// Save the answer in its conversation once it is finished, whether or not
/// anyone still looks at it.
fn save_when_finished(state: Arc<HubState>, answer_id: String) {
    tokio::spawn(async move {
        let until = Instant::now() + SAVE_WITHIN;
        while Instant::now() < until {
            tokio::time::sleep(SAVE_EVERY).await;
            match state.assistant.answer(&answer_id) {
                Some(a) if answer_finished(&a) => {
                    save_answer(&state.db, &a);
                    return;
                }
                Some(_) => {}
                None => {
                    let _ = state.db.finish_turn(&answer_id, &lost_answer());
                    return;
                }
            }
        }
    });
}

/// Turns still waiting for their answer: save those that are finished, and
/// close those the hub lost (it was restarted). True when any changed.
fn settle_pending(state: &HubState, conv: &zaklon_core::conversations::Conversation) -> bool {
    let mut changed = false;
    for answer_id in conv.turns.iter().filter(|t| t.status == "pending").filter_map(|t| t.answer_id.as_deref()) {
        match state.assistant.answer(answer_id) {
            Some(a) if answer_finished(&a) => changed |= save_answer(&state.db, &a),
            Some(_) => {}
            None => changed |= state.db.finish_turn(answer_id, &lost_answer()).unwrap_or(false),
        }
    }
    changed
}

#[derive(Deserialize)]
pub(super) struct ConversationQuery {
    #[serde(default)]
    q: Option<String>,
}

pub(super) async fn conversations_list(
    State(state): State<Arc<HubState>>,
    caller: Caller,
    axum::extract::Query(query): axum::extract::Query<ConversationQuery>,
) -> Result<Json<Vec<zaklon_core::conversations::Summary>>, ApiError> {
    let q: Option<String> = query.q.map(|q| q.chars().take(200).collect());
    Ok(Json(state.db.list_conversations(&caller.owner(), q.as_deref())?))
}

#[derive(Deserialize, Default)]
pub(super) struct NewConversationBody {
    #[serde(default)]
    title: String,
}

pub(super) async fn conversations_create(
    State(state): State<Arc<HubState>>,
    caller: Caller,
    body: Option<Json<NewConversationBody>>,
) -> Result<(StatusCode, Json<zaklon_core::conversations::Summary>), ApiError> {
    let title = body.map(|b| b.0.title).unwrap_or_default();
    let conv = state.db.create_conversation(&caller.owner(), &title).map_err(invalid)?;
    Ok((StatusCode::CREATED, Json(conv)))
}

pub(super) async fn conversations_get(
    State(state): State<Arc<HubState>>,
    caller: Caller,
    Path(id): Path<String>,
) -> Result<Json<zaklon_core::conversations::Conversation>, ApiError> {
    let owner = caller.owner();
    let missing = || not_found("no such conversation");
    let conv = state.db.get_conversation(&owner, &id)?.ok_or_else(missing)?;
    if settle_pending(&state, &conv) {
        return Ok(Json(state.db.get_conversation(&owner, &id)?.ok_or_else(missing)?));
    }
    Ok(Json(conv))
}

#[derive(Deserialize)]
pub(super) struct TitleBody {
    title: String,
}

pub(super) async fn conversations_rename(
    State(state): State<Arc<HubState>>,
    caller: Caller,
    Path(id): Path<String>,
    Json(body): Json<TitleBody>,
) -> Result<Json<zaklon_core::conversations::Summary>, ApiError> {
    let renamed = state.db.rename_conversation(&caller.owner(), &id, &body.title).map_err(invalid)?;
    renamed.map(Json).ok_or_else(|| not_found("no such conversation"))
}

pub(super) async fn conversations_delete(State(state): State<Arc<HubState>>, caller: Caller, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    if state.db.delete_conversation(&caller.owner(), &id)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found("no such conversation"))
    }
}

#[derive(Deserialize)]
pub(super) struct SendBody {
    /// "laptop", or the id of a paired phone.
    to: String,
}

/// Send a copy of a conversation to another device of the household, where
/// it shows as a new conversation marked with the sender's name.
pub(super) async fn conversations_send(
    State(state): State<Arc<HubState>>,
    caller: Caller,
    Path(id): Path<String>,
    Json(body): Json<SendBody>,
) -> Result<StatusCode, ApiError> {
    use zaklon_core::conversations::LAPTOP;
    let owner = caller.owner();
    let to = body.to.trim();
    let in_household = to == LAPTOP || state.db.list_devices()?.iter().any(|d| d.id == to);
    if !in_household || to == owner {
        return Err(not_found("no such device"));
    }
    let from_name = match &caller {
        Caller::Local => LAPTOP.to_string(),
        Caller::Device(d) => d.name.clone(),
    };
    if state.db.send_conversation(&owner, &id, to, &from_name)?.is_none() {
        return Err(not_found("no such conversation"));
    }
    tracing::info!(by = %caller.actor(), "conversation sent to another device");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub(super) struct OutcomeBody {
    /// "done" or "canceled".
    outcome: String,
}

/// What was decided about a supplies change the assistant proposed in a turn.
pub(super) async fn conversations_outcome(
    State(state): State<Arc<HubState>>,
    caller: Caller,
    Path((id, turn)): Path<(String, String)>,
    Json(body): Json<OutcomeBody>,
) -> Result<StatusCode, ApiError> {
    if state.db.set_turn_outcome(&caller.owner(), &id, &turn, &body.outcome).map_err(invalid)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found("no such conversation"))
    }
}
