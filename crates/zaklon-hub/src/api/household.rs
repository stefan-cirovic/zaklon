//! The household: the hub's status, first-run setup and the household
//! password, the paired devices, and the tool pinned to the navigation bar.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use zaklon_core::db::Device;
use zaklon_core::pairing;

use super::error::{bad, forbidden, not_found, ApiError};
use super::{blocking, Caller, Local};
use crate::{HubState, VERSION};

// ---- status & setup ---------------------------------------------------------

#[derive(Serialize)]
pub(super) struct Status {
    hub_id: String,
    hub_name: String,
    version: String,
    port: u16,
    fingerprint: String,
    set_up: bool,
    language: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    devices: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    uptime_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    addresses: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    root: Option<String>,
}

/// Public for anyone on the network (needed before pairing); richer for callers we trust.
pub(super) async fn status(State(state): State<Arc<HubState>>, caller: Option<Caller>) -> Result<Json<Status>, ApiError> {
    let cfg = state.config();
    let trusted = caller.is_some();
    Ok(Json(Status {
        hub_id: cfg.hub_id.clone(),
        hub_name: cfg.hub_name.clone(),
        version: VERSION.into(),
        port: cfg.port,
        fingerprint: state.identity.fingerprint.clone(),
        set_up: state.db.is_set_up()?,
        language: cfg.language.clone(),
        devices: if trusted { Some(state.db.count_devices()?) } else { None },
        uptime_secs: if trusted { Some(state.uptime_secs()) } else { None },
        addresses: if trusted {
            Some(crate::discovery::shown_ipv4_addresses().iter().map(|a| a.to_string()).collect())
        } else {
            None
        },
        root: if trusted { Some(cfg.root.display().to_string()) } else { None },
    }))
}

#[derive(Deserialize)]
pub(super) struct SetupBody {
    password: String,
    #[serde(default)]
    hub_name: Option<String>,
    #[serde(default)]
    language: Option<String>,
    /// The answer to "check once a day for a newer Zaklon?" (asked at setup).
    #[serde(default)]
    check_updates: Option<bool>,
}

pub(super) async fn setup(
    State(state): State<Arc<HubState>>,
    _: Local,
    Json(body): Json<SetupBody>,
) -> Result<StatusCode, ApiError> {
    let _one_at_a_time = state.password_lock.lock().await;
    if state.db.is_set_up()? {
        return Err(bad("already set up; use /api/password to change the password"));
    }
    if body.password.chars().count() < pairing::MIN_PASSWORD_LEN {
        return Err(bad("password must be at least 8 characters"));
    }
    // With it, the key backups are encrypted with.
    let st = state.clone();
    let password = body.password;
    blocking(move || crate::backup::set_household_password(&st.db, &password)).await??;
    let mut cfg = state.config.lock().unwrap_or_else(|p| p.into_inner());
    let mut changed = false;
    if let Some(n) = body.hub_name.filter(|n| !n.trim().is_empty()) {
        cfg.hub_name = n.trim().to_string();
        changed = true;
    }
    if let Some(l) = body.language.filter(|l| l == "en" || l == "sr") {
        cfg.language = l;
        changed = true;
    }
    if let Some(on) = body.check_updates {
        cfg.auto_update_check = on;
        changed = true;
    }
    if changed {
        cfg.save()?;
    }
    let updates_on = cfg.auto_update_check;
    drop(cfg);
    state.updates.set_enabled(updates_on);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub(super) struct PasswordBody {
    new_password: String,
}

pub(super) async fn change_password(
    State(state): State<Arc<HubState>>,
    _: Local,
    Json(body): Json<PasswordBody>,
) -> Result<StatusCode, ApiError> {
    if body.new_password.chars().count() < pairing::MIN_PASSWORD_LEN {
        return Err(bad("password must be at least 8 characters"));
    }
    // Backups get a new key locked with the new password; those made before
    // keep opening with the old one.
    let _one_at_a_time = state.password_lock.lock().await;
    let st = state.clone();
    blocking(move || crate::backup::set_household_password(&st.db, &body.new_password)).await??;
    Ok(StatusCode::NO_CONTENT)
}

// ---- devices ----------------------------------------------------------------

pub(super) async fn list_devices(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<Vec<Device>>, ApiError> {
    Ok(Json(state.db.list_devices()?))
}

#[derive(Deserialize)]
pub(super) struct RenameBody {
    name: String,
}

/// A phone may rename or remove only itself; the laptop manages every phone.
fn own_device_or_laptop(caller: &Caller, id: &str) -> Result<(), ApiError> {
    match caller {
        Caller::Device(d) if d.id != id => Err(forbidden("only the laptop can do this")),
        _ => Ok(()),
    }
}

/// History names the laptop "laptop"; a phone must not look like it.
pub(super) fn reserved_device_name(name: &str) -> bool {
    name.trim().eq_ignore_ascii_case("laptop")
}

pub(super) async fn rename_device(
    State(state): State<Arc<HubState>>,
    caller: Caller,
    Path(id): Path<String>,
    Json(body): Json<RenameBody>,
) -> Result<StatusCode, ApiError> {
    own_device_or_laptop(&caller, &id)?;
    let name: String = body.name.trim().chars().take(60).collect();
    if name.is_empty() {
        return Err(bad("name is required"));
    }
    if reserved_device_name(&name) {
        return Err(bad("that name is reserved"));
    }
    if state.db.rename_device(&id, &name)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found("no such device"))
    }
}

pub(super) async fn delete_device(
    State(state): State<Arc<HubState>>,
    caller: Caller,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    own_device_or_laptop(&caller, &id)?;
    tracing::info!(by = %caller.actor(), device = %id, "device removed");
    if state.db.delete_device(&id)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found("no such device"))
    }
}

pub(super) async fn me(caller: Caller) -> Result<Json<serde_json::Value>, ApiError> {
    match caller {
        Caller::Local => Ok(Json(serde_json::json!({ "kind": "laptop" }))),
        Caller::Device(d) => Ok(Json(serde_json::json!({ "kind": "device", "device": d }))),
    }
}

// ---- the tool pinned to the bar --------------------------------------------------

/// The database setting that holds the pinned tool (a backup brings it back
/// with the rest of the household's data).
const PINNED_TOOL: &str = "pinned_tool";

/// A tool is named by the id the app uses in its addresses ("supplies",
/// "maps"): short, lowercase letters, digits and hyphens. The hub keeps no
/// list of the app's tools; the app ignores an id it does not know.
fn is_tool_id(id: &str) -> bool {
    (1..=32).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[derive(Serialize, Deserialize)]
pub(super) struct PinnedTool {
    /// None: nothing is pinned.
    tool: Option<String>,
}

/// The one tool the household pinned to the navigation bar, next to Home,
/// Assistant, Tools and Household. Every device shows the same one.
pub(super) async fn pinned_tool(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<PinnedTool>, ApiError> {
    let tool = state.db.get_setting(PINNED_TOOL)?.filter(|t| is_tool_id(t));
    Ok(Json(PinnedTool { tool }))
}

/// Laptop only: pin a tool (it replaces the one pinned before), or none.
pub(super) async fn pin_tool(State(state): State<Arc<HubState>>, _: Local, Json(body): Json<PinnedTool>) -> Result<Json<PinnedTool>, ApiError> {
    let tool = body.tool.filter(|t| !t.is_empty());
    if tool.as_deref().is_some_and(|t| !is_tool_id(t)) {
        return Err(bad("no such tool"));
    }
    state.db.set_setting(PINNED_TOOL, tool.as_deref().unwrap_or(""))?;
    Ok(Json(PinnedTool { tool }))
}

#[cfg(test)]
mod pinned_tool_tests {
    use super::is_tool_id;

    #[test]
    fn tool_ids_are_short_lowercase_names() {
        for ok in ["supplies", "maps", "addons", "first-aid", "tool2"] {
            assert!(is_tool_id(ok), "{ok}");
        }
        let long = "x".repeat(33);
        for bad in ["", "Supplies", "../evil", "a b", "maps/", long.as_str(), "šuma", "#maps"] {
            assert!(!is_tool_id(bad), "{bad:?}");
        }
        assert!(is_tool_id(&"x".repeat(32)));
    }
}
