//! Upkeep of the hub, most of it only the laptop can do: copies of add-ons
//! to a drive, backups, updates, the Wi-Fi network from the laptop, and
//! Windows Firewall.

use std::sync::Arc;
use std::time::Duration;

use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use super::error::{bad, ApiError};
use super::{blocking, Caller, Local};
use crate::HubState;

// ---- copies to a drive ----------------------------------------------------------------

#[derive(Deserialize)]
pub(super) struct ExportBody {
    dir: String,
    #[serde(default)]
    ids: Vec<String>,
    /// Also the Windows installer and the phone app, for a friend starting from nothing.
    #[serde(default)]
    with_apps: bool,
}

/// Copy packs to a folder (a USB drive) in the background. Laptop only:
/// it writes to the laptop's own drives.
pub(super) async fn export_start(State(state): State<Arc<HubState>>, _: Local, Json(body): Json<ExportBody>) -> Result<StatusCode, ApiError> {
    let dir = std::path::PathBuf::from(body.dir.trim());
    let (ex, d) = (state.export.clone(), state.downloads.clone());
    blocking(move || ex.start(&d, &body.ids, &dir, body.with_apps)).await?.map_err(|e| bad(&e))?;
    tracing::info!(files = state.export.state().files_total, "copy to drive started");
    Ok(StatusCode::ACCEPTED)
}

#[derive(Serialize)]
pub(super) struct ExportReply {
    #[serde(flatten)]
    state: crate::export::ExportState,
    /// Installer and phone app available to put on the stick, with sizes.
    apps: Vec<(String, u64)>,
}

pub(super) async fn export_status(State(state): State<Arc<HubState>>, _: Local) -> Json<ExportReply> {
    let apps = crate::export::app_files(&state.downloads)
        .into_iter()
        .map(|(p, n)| (n, std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)))
        .collect();
    Json(ExportReply { state: state.export.state(), apps })
}

pub(super) async fn export_cancel(State(state): State<Arc<HubState>>, _: Local) -> StatusCode {
    state.export.cancel();
    StatusCode::NO_CONTENT
}

// ---- backups ----------------------------------------------------------------------

#[derive(Serialize)]
pub(super) struct BackupsReply {
    backups: Vec<crate::backup::BackupFile>,
    /// A restore is waiting for the next start.
    restore_pending: bool,
    folder: String,
    /// "on", "off" (not yet: the household password turns it on) or "no_password".
    encryption: &'static str,
}

pub(super) async fn backups_list(State(state): State<Arc<HubState>>, _: Local) -> Result<Json<BackupsReply>, ApiError> {
    let cfg = state.config();
    let root = cfg.root.clone();
    let backups = blocking(move || crate::backup::list(&cfg)).await?;
    Ok(Json(BackupsReply {
        backups,
        restore_pending: crate::backup::restore_pending(&root),
        folder: state.config().backups_dir().display().to_string(),
        encryption: crate::backup::encryption_state(&state.db),
    }))
}

#[derive(Deserialize)]
pub(super) struct BackupBody {
    /// Where to save it; the backups folder when empty.
    #[serde(default)]
    dir: String,
}

pub(super) async fn backups_create(State(state): State<Arc<HubState>>, _: Local, Json(body): Json<BackupBody>) -> Result<Json<serde_json::Value>, ApiError> {
    let cfg = state.config();
    let dir = if body.dir.trim().is_empty() { cfg.backups_dir() } else { std::path::PathBuf::from(body.dir.trim()) };
    if !dir.is_dir() {
        return Err(bad("that folder does not exist"));
    }
    let st = state.clone();
    let path = blocking(move || crate::backup::create(&cfg, &st.db, &dir, false)).await?.map_err(|e| bad(&e))?;
    Ok(Json(serde_json::json!({ "path": path.display().to_string() })))
}

#[derive(Deserialize)]
pub(super) struct RestoreBody {
    path: String,
    /// For an encrypted backup: the household password from when it was made.
    #[serde(default)]
    password: String,
    /// The person saw that the backup is not encrypted and restores it
    /// anyway (see `backup::needs_confirmation`).
    #[serde(default)]
    allow_unencrypted: bool,
}

/// Check a backup and prepare it; it replaces the data on the next start
/// (keeping who may connect unless this is a new install: see
/// `backup::Access`). The reply says what the backup really holds, not what
/// its manifest claims: `encrypted` is false for a backup that was not.
pub(super) async fn backups_restore(State(state): State<Arc<HubState>>, _: Local, Json(body): Json<RestoreBody>) -> Result<Json<crate::backup::Staged>, ApiError> {
    let cfg = state.config();
    let path = std::path::PathBuf::from(body.path.trim().trim_matches('"'));
    if !path.is_file() {
        return Err(bad("that file does not exist"));
    }
    // A wrong password is found out before anything is written.
    let (password, allow_unencrypted) = (body.password, body.allow_unencrypted);
    let backup = blocking(move || crate::backup::unlock(&path, &password)).await?.map_err(|e| bad(&e))?;
    if !allow_unencrypted && crate::backup::needs_confirmation(&state.db, &backup) {
        return Err(bad(crate::backup::NOT_ENCRYPTED));
    }
    // Keep today's data too, whatever happens next.
    let st = state.clone();
    let cfg2 = cfg.clone();
    blocking(move || crate::backup::create(&cfg2, &st.db, &cfg2.backups_dir(), false)).await?.map_err(|e| bad(&e))?;
    let manifest = blocking(move || crate::backup::stage_unlocked(&cfg, backup)).await?.map_err(|e| bad(&e))?;
    Ok(Json(manifest))
}

#[derive(Deserialize)]
pub(super) struct EncryptionBody {
    password: String,
}

/// For a hub set up before backups were encrypted: the household password,
/// typed once, encrypts every backup from now on.
pub(super) async fn backups_encryption(State(state): State<Arc<HubState>>, _: Local, Json(body): Json<EncryptionBody>) -> Result<StatusCode, ApiError> {
    let _one_at_a_time = state.password_lock.lock().await;
    let st = state.clone();
    blocking(move || crate::backup::turn_on_encryption(&st.db, &body.password)).await?.map_err(|e| bad(&e))?;
    Ok(StatusCode::NO_CONTENT)
}

// ---- updates ----------------------------------------------------------------------

pub(super) async fn updates_state(State(state): State<Arc<HubState>>, _caller: Caller) -> Json<crate::updates::UpdateState> {
    Json(state.updates.state())
}

pub(super) async fn updates_check(State(state): State<Arc<HubState>>, _caller: Caller) -> Json<crate::updates::UpdateState> {
    // At most one question to GitHub every ten minutes, whoever asks.
    if state.updates.checked_recently(Duration::from_secs(600)) {
        return Json(state.updates.state());
    }
    Json(state.updates.check().await)
}

#[derive(Deserialize)]
pub(super) struct UpdateSettings {
    enabled: bool,
}

pub(super) async fn updates_settings(State(state): State<Arc<HubState>>, _: Local, Json(body): Json<UpdateSettings>) -> Result<StatusCode, ApiError> {
    {
        let mut cfg = state.config.lock().unwrap_or_else(|p| p.into_inner());
        cfg.auto_update_check = body.enabled;
        cfg.save()?;
    }
    state.updates.set_enabled(body.enabled);
    Ok(StatusCode::NO_CONTENT)
}

// ---- Wi-Fi network from the laptop ------------------------------------------------

#[derive(Serialize)]
pub(super) struct HotspotReply {
    #[serde(flatten)]
    state: crate::hotspot::HotspotState,
    /// For the "join this Wi-Fi" QR code, when ours is on.
    qr: Option<String>,
}

fn hotspot_reply(mut state: crate::hotspot::HotspotState) -> HotspotReply {
    // Only our own network's password is shown, not whatever Windows had before.
    if state.ssid != crate::hotspot::SSID {
        state.passphrase.clear();
    }
    let qr = (state.on && state.ssid == crate::hotspot::SSID).then(|| crate::hotspot::wifi_qr(&state.ssid, &state.passphrase));
    HotspotReply { state, qr }
}

pub(super) async fn hotspot_status(_: Local) -> Result<Json<HotspotReply>, ApiError> {
    let s = blocking(crate::hotspot::status).await?;
    Ok(Json(hotspot_reply(s)))
}

/// Laptop only: it changes this computer's network.
pub(super) async fn hotspot_start(State(state): State<Arc<HubState>>, _: Local) -> Result<Json<HotspotReply>, ApiError> {
    let pass = match state.db.get_setting(crate::hotspot::SETTING_PASSPHRASE)? {
        Some(p) if crate::hotspot::is_safe_passphrase(&p) => p,
        _ => {
            let p = crate::hotspot::new_passphrase();
            state.db.set_setting(crate::hotspot::SETTING_PASSPHRASE, &p)?;
            p
        }
    };
    tracing::info!("starting the Wi-Fi network");
    let s = blocking(move || crate::hotspot::start(&pass)).await?;
    if let Some(previous) = &s.previous {
        // The person's own hotspot name and password; they come back when ours is turned off.
        state.db.set_setting(crate::hotspot::SETTING_PREVIOUS, &serde_json::to_string(previous).map_err(|e| anyhow::anyhow!(e))?)?;
    }
    Ok(Json(hotspot_reply(s)))
}

pub(super) async fn hotspot_stop(State(state): State<Arc<HubState>>, _: Local) -> Result<Json<HotspotReply>, ApiError> {
    tracing::info!("stopping the Wi-Fi network");
    let previous = state
        .db
        .get_setting(crate::hotspot::SETTING_PREVIOUS)?
        .and_then(|t| serde_json::from_str::<crate::hotspot::AccessPoint>(&t).ok());
    let restore = previous.clone();
    let s = blocking(move || crate::hotspot::stop(restore.as_ref())).await?;
    if previous.is_some() && s.error.is_none() && s.ssid != crate::hotspot::SSID {
        state.db.set_setting(crate::hotspot::SETTING_PREVIOUS, "")?;
    }
    Ok(Json(hotspot_reply(s)))
}

// ---- Windows Firewall ---------------------------------------------------------------

#[derive(Serialize)]
pub(super) struct FirewallReply {
    #[serde(flatten)]
    state: crate::firewall::FirewallState,
    ok: bool,
}

pub(super) async fn firewall_status(_: Local) -> Result<Json<FirewallReply>, ApiError> {
    let state = blocking(crate::firewall::status).await?;
    Ok(Json(FirewallReply { ok: state.ok(), state }))
}

/// Laptop only: changes this computer's firewall (Windows asks for consent).
pub(super) async fn firewall_allow(State(state): State<Arc<HubState>>, _: Local) -> Result<Json<FirewallReply>, ApiError> {
    tracing::info!("asking Windows to let phones in through the firewall");
    let ports = crate::firewall::Ports::of(&state.config());
    let state = blocking(move || crate::firewall::allow(&ports)).await?;
    Ok(Json(FirewallReply { ok: state.ok(), state }))
}
