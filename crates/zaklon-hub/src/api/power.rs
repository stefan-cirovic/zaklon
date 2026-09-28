//! The household's list for the power calculator (Tools › Power calculator):
//! the appliances to keep running in a power cut, and the choices around
//! them (days without grid, battery, sun). The app does the arithmetic; the
//! hub keeps the list as one JSON document so every device shows the same
//! one. Any paired device may read and change it: it holds nothing that can
//! do harm, and a household edits it together.

use std::sync::Arc;

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zaklon_core::dates::now_rfc3339;

use super::error::{bad, ApiError};
use super::Caller;
use crate::HubState;

/// The database setting that holds the list (a backup brings it back with
/// the rest of the household's data).
const POWER_PLAN: &str = "power_plan";

/// Far more than a household's list needs: the app keeps at most 60
/// appliances, about 6 KiB.
const MAX_PLAN_BYTES: usize = 32 * 1024;

#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
pub(super) struct PowerPlan {
    /// The app's document (see ui/src/power.ts); None until a list is saved.
    plan: Option<Value>,
    /// When it was last saved, and by whom ("laptop" or the phone's name).
    updated_at: Option<String>,
    updated_by: Option<String>,
    /// New with every save (random), so a device sees that the list changed
    /// even when two saves fall in the same second of `updated_at`.
    #[serde(default)]
    rev: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct SavePlan {
    /// None (or null) clears the list.
    plan: Option<Value>,
}

/// The saved setting as the reply; a damaged one reads as no list rather
/// than breaking the screen.
fn read(saved: Option<String>) -> PowerPlan {
    saved.and_then(|s| serde_json::from_str::<PowerPlan>(&s).ok()).unwrap_or_default()
}

/// What to keep for a save, as the setting's text; refused when it is not a
/// JSON object or is too large.
fn to_save(plan: Option<Value>, at: String, by: String) -> Result<(PowerPlan, String), ApiError> {
    let plan = match plan {
        None => None,
        Some(v @ Value::Object(_)) => Some(v),
        Some(_) => return Err(bad("the power list must be a JSON object")),
    };
    let rev = format!("{:016x}", rand::random::<u64>());
    let saved = PowerPlan { plan, updated_at: Some(at), updated_by: Some(by), rev: Some(rev) };
    let text = serde_json::to_string(&saved).map_err(anyhow::Error::from)?;
    if text.len() > MAX_PLAN_BYTES {
        return Err(bad("the power list is too large"));
    }
    Ok((saved, text))
}

/// The household's list; every paired device and the laptop read the same one.
pub(super) async fn power_plan(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<PowerPlan>, ApiError> {
    Ok(Json(read(state.db.get_setting(POWER_PLAN)?)))
}

/// Replace the household's list (the laptop or any paired phone).
pub(super) async fn power_plan_save(
    State(state): State<Arc<HubState>>,
    caller: Caller,
    Json(body): Json<SavePlan>,
) -> Result<Json<PowerPlan>, ApiError> {
    let (saved, text) = to_save(body.plan, now_rfc3339(), caller.actor())?;
    state.db.set_setting(POWER_PLAN, &text)?;
    Ok(Json(saved))
}

#[cfg(test)]
mod power_plan_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_list_is_kept_with_who_saved_it() {
        let plan = json!({ "v": 1, "lines": [{ "id": "fridge", "qty": 1 }], "days": 3 });
        let (saved, text) = to_save(Some(plan.clone()), "2026-09-29T10:00:00Z".into(), "Ana's phone".into()).unwrap_or_else(|_| panic!("refused"));
        assert_eq!(saved.plan, Some(plan.clone()));
        assert_eq!(read(Some(text)), saved);
        // The same list, saved again in the same second, is still a new revision.
        let (again, _) = to_save(Some(plan), "2026-09-29T10:00:00Z".into(), "Ana's phone".into()).unwrap_or_else(|_| panic!("refused"));
        assert!(saved.rev.as_deref().is_some_and(|r| r.len() == 16), "{:?}", saved.rev);
        assert_ne!(again.rev, saved.rev);
        // Cleared: no list, but still who cleared it.
        let (cleared, text) = to_save(None, "t".into(), "laptop".into()).unwrap_or_else(|_| panic!("refused"));
        assert_eq!(read(Some(text)).plan, None);
        assert_eq!(cleared.updated_by.as_deref(), Some("laptop"));
    }

    #[test]
    fn only_a_small_object_is_kept() {
        for not_object in [json!([1, 2]), json!("text"), json!(3)] {
            assert!(to_save(Some(not_object), "t".into(), "x".into()).is_err());
        }
        let huge = json!({ "lines": "x".repeat(MAX_PLAN_BYTES) });
        assert!(to_save(Some(huge), "t".into(), "x".into()).is_err());
        assert!(to_save(Some(json!({ "lines": "x".repeat(MAX_PLAN_BYTES / 2) })), "t".into(), "x".into()).is_ok());
    }

    #[test]
    fn nothing_saved_or_damaged_reads_as_no_list() {
        assert_eq!(read(None), PowerPlan::default());
        assert_eq!(read(Some("{not json".into())), PowerPlan::default());
        assert_eq!(read(Some("[]".into())), PowerPlan::default());
    }
}
