//! JSON API shared by the desktop window (over 127.0.0.1) and paired phones
//! (over pinned TLS). A caller is either *Local* (the laptop itself) or a
//! *Device* presenting its bearer token. Physical access to the laptop is
//! trust, so Local can do everything, including first-run setup.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    extract::{ConnectInfo, FromRequestParts, OptionalFromRequestParts, Path, State},
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tower_http::trace::TraceLayer;
use zaklon_core::db::{now_rfc3339, Device};
use zaklon_core::pairing;

use crate::{HubState, PairingSession, VERSION};

const PAIRING_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_PAIRING_ATTEMPTS: u8 = 3;

pub fn router(state: Arc<HubState>) -> Router {
    Router::new()
        .route("/api/status", get(status))
        .route("/api/setup", post(setup))
        .route("/api/password", post(change_password))
        .route("/api/pair/start", post(pair_start))
        .route("/api/pair/complete", post(pair_complete))
        .route("/api/devices", get(list_devices))
        .route("/api/devices/{id}", axum::routing::patch(rename_device).delete(delete_device))
        .route("/api/me", get(me))
        .route("/api/catalog", get(catalog))
        .route("/api/system", get(system))
        .route("/api/packs/import", post(packs_import))
        .route("/api/packs/{id}", axum::routing::delete(pack_remove))
        .route("/api/packs/{id}/download", post(pack_download))
        .route("/api/packs/{id}/pause", post(pack_pause))
        .route("/api/packs/{id}/export", post(pack_export))
        .fallback(crate::ui::serve)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

// ---- errors -----------------------------------------------------------------

pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        tracing::error!("internal error: {e:#}");
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, "internal error".into())
    }
}

fn bad(msg: &str) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}
fn forbidden(msg: &str) -> ApiError {
    ApiError(StatusCode::FORBIDDEN, msg.into())
}
fn unauthorized() -> ApiError {
    ApiError(StatusCode::UNAUTHORIZED, "unauthorized".into())
}
fn not_found(msg: &str) -> ApiError {
    ApiError(StatusCode::NOT_FOUND, msg.into())
}

// ---- caller -----------------------------------------------------------------

pub enum Caller {
    Local,
    Device(Device),
}

impl Caller {
    fn actor(&self) -> String {
        match self {
            Caller::Local => "laptop".into(),
            Caller::Device(d) => d.id.clone(),
        }
    }
}

impl FromRequestParts<Arc<HubState>> for Caller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &Arc<HubState>) -> Result<Self, Self::Rejection> {
        if let Some(auth) = parts.headers.get("authorization").and_then(|v| v.to_str().ok()) {
            if let Some(token) = auth.strip_prefix("Bearer ") {
                let hash = token_hash(token.trim());
                if let Some(dev) = state.db.device_by_token_hash(&hash, &now_rfc3339())? {
                    return Ok(Caller::Device(dev));
                }
                return Err(unauthorized());
            }
        }
        let peer = parts.extensions.get::<ConnectInfo<SocketAddr>>().map(|c| c.0);
        match peer {
            Some(addr) if addr.ip().is_loopback() => Ok(Caller::Local),
            _ => Err(unauthorized()),
        }
    }
}

impl OptionalFromRequestParts<Arc<HubState>> for Caller {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<HubState>,
    ) -> Result<Option<Self>, Self::Rejection> {
        match <Caller as FromRequestParts<Arc<HubState>>>::from_request_parts(parts, state).await {
            Ok(c) => Ok(Some(c)),
            Err(ApiError(StatusCode::UNAUTHORIZED, _)) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// A caller that must be the laptop itself.
pub struct Local;

impl FromRequestParts<Arc<HubState>> for Local {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &Arc<HubState>) -> Result<Self, Self::Rejection> {
        match <Caller as FromRequestParts<Arc<HubState>>>::from_request_parts(parts, state).await? {
            Caller::Local => Ok(Local),
            Caller::Device(_) => Err(forbidden("only the laptop can do this")),
        }
    }
}

fn token_hash(token: &str) -> String {
    let d = Sha256::digest(token.as_bytes());
    d.iter().map(|b| format!("{b:02x}")).collect()
}

// ---- status & setup ---------------------------------------------------------

#[derive(Serialize)]
struct Status {
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
async fn status(State(state): State<Arc<HubState>>, caller: Option<Caller>) -> Result<Json<Status>, ApiError> {
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
            Some(state.lan_addresses().iter().map(|a| a.to_string()).collect())
        } else {
            None
        },
        root: if trusted { Some(cfg.root.display().to_string()) } else { None },
    }))
}

#[derive(Deserialize)]
struct SetupBody {
    password: String,
    #[serde(default)]
    hub_name: Option<String>,
    #[serde(default)]
    language: Option<String>,
}

async fn setup(
    State(state): State<Arc<HubState>>,
    _: Local,
    Json(body): Json<SetupBody>,
) -> Result<StatusCode, ApiError> {
    if state.db.is_set_up()? {
        return Err(bad("already set up; use /api/password to change the password"));
    }
    if body.password.chars().count() < pairing::MIN_PASSWORD_LEN {
        return Err(bad("password must be at least 8 characters"));
    }
    state
        .db
        .set_setting("household_password_hash", &pairing::hash_password(&body.password)?)?;
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
    if changed {
        cfg.save()?;
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct PasswordBody {
    new_password: String,
}

async fn change_password(
    State(state): State<Arc<HubState>>,
    _: Local,
    Json(body): Json<PasswordBody>,
) -> Result<StatusCode, ApiError> {
    if body.new_password.chars().count() < pairing::MIN_PASSWORD_LEN {
        return Err(bad("password must be at least 8 characters"));
    }
    state
        .db
        .set_setting("household_password_hash", &pairing::hash_password(&body.new_password)?)?;
    Ok(StatusCode::NO_CONTENT)
}

// ---- pairing ----------------------------------------------------------------

#[derive(Serialize)]
struct PairStart {
    code: String,
    expires_in_secs: u64,
    /// What goes into the QR code.
    payload: PairPayload,
}

#[derive(Serialize)]
struct PairPayload {
    v: u8,
    hosts: Vec<String>,
    port: u16,
    fp: String,
    code: String,
    name: String,
    install_port: u16,
}

async fn pair_start(State(state): State<Arc<HubState>>, _: Local) -> Result<Json<PairStart>, ApiError> {
    if !state.db.is_set_up()? {
        return Err(bad("set a household password first"));
    }
    let code = pairing::pairing_code();
    {
        let mut sessions = state.pairing.lock().unwrap_or_else(|p| p.into_inner());
        sessions.retain(|_, s| s.expires_at > Instant::now());
        sessions.insert(
            code.clone(),
            PairingSession { expires_at: Instant::now() + PAIRING_TTL, failed_attempts: 0 },
        );
    }
    let cfg = state.config();
    Ok(Json(PairStart {
        code: code.clone(),
        expires_in_secs: PAIRING_TTL.as_secs(),
        payload: PairPayload {
            v: 1,
            hosts: state.lan_addresses().iter().map(|a| a.to_string()).collect(),
            port: cfg.port,
            fp: state.identity.fingerprint.clone(),
            code,
            name: cfg.hub_name,
            install_port: zaklon_core::config::INSTALL_PORT,
        },
    }))
}

#[derive(Deserialize)]
struct PairComplete {
    code: String,
    password: String,
    device_name: String,
    #[serde(default = "default_platform")]
    platform: String,
}

fn default_platform() -> String {
    "android".into()
}

#[derive(Serialize)]
struct Paired {
    device_id: String,
    device_token: String,
    hub_id: String,
    hub_name: String,
    fingerprint: String,
}

async fn pair_complete(
    State(state): State<Arc<HubState>>,
    Json(body): Json<PairComplete>,
) -> Result<Json<Paired>, ApiError> {
    let now = Instant::now();
    {
        let mut sessions = state.pairing.lock().unwrap_or_else(|p| p.into_inner());
        sessions.retain(|_, s| s.expires_at > now);
        let Some(session) = sessions.get_mut(&body.code) else {
            return Err(forbidden("pairing code is invalid or expired"));
        };
        let hash = state.db.get_setting("household_password_hash")?.unwrap_or_default();
        if !pairing::verify_password(&body.password, &hash) {
            session.failed_attempts += 1;
            if session.failed_attempts >= MAX_PAIRING_ATTEMPTS {
                sessions.remove(&body.code);
                return Err(forbidden("too many attempts; start pairing again on the laptop"));
            }
            return Err(forbidden("wrong household password"));
        }
        sessions.remove(&body.code);
    }
    let token = pairing::random_token(32);
    let mut name: String = body.device_name.trim().chars().take(60).collect();
    if name.is_empty() {
        name = "Phone".to_string();
    }
    let device = Device {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        platform: body.platform,
        created_at: now_rfc3339(),
        last_seen: Some(now_rfc3339()),
    };
    state.db.insert_device(&device, &token_hash(&token))?;
    let cfg = state.config();
    tracing::info!(device = %device.name, "device paired");
    Ok(Json(Paired {
        device_id: device.id,
        device_token: token,
        hub_id: cfg.hub_id,
        hub_name: cfg.hub_name,
        fingerprint: state.identity.fingerprint.clone(),
    }))
}

// ---- devices ----------------------------------------------------------------

async fn list_devices(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<Vec<Device>>, ApiError> {
    Ok(Json(state.db.list_devices()?))
}

#[derive(Deserialize)]
struct RenameBody {
    name: String,
}

async fn rename_device(
    State(state): State<Arc<HubState>>,
    _caller: Caller,
    Path(id): Path<String>,
    Json(body): Json<RenameBody>,
) -> Result<StatusCode, ApiError> {
    let name: String = body.name.trim().chars().take(60).collect();
    if name.is_empty() {
        return Err(bad("name is required"));
    }
    if state.db.rename_device(&id, &name)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found("no such device"))
    }
}

async fn delete_device(
    State(state): State<Arc<HubState>>,
    caller: Caller,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    tracing::info!(by = %caller.actor(), device = %id, "device removed");
    if state.db.delete_device(&id)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found("no such device"))
    }
}

async fn me(caller: Caller) -> Result<Json<serde_json::Value>, ApiError> {
    match caller {
        Caller::Local => Ok(Json(serde_json::json!({ "kind": "laptop" }))),
        Caller::Device(d) => Ok(Json(serde_json::json!({ "kind": "device", "device": d }))),
    }
}

// ---- add-ons ----------------------------------------------------------------

#[derive(Serialize)]
struct CatalogReply {
    packs: Vec<crate::downloads::PackView>,
    system: crate::downloads::SystemInfo,
}

async fn catalog(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<CatalogReply>, ApiError> {
    let d = &state.downloads;
    Ok(Json(CatalogReply { packs: d.snapshot(), system: crate::downloads::system_info(d.library_dir()) }))
}

async fn system(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<crate::downloads::SystemInfo>, ApiError> {
    Ok(Json(crate::downloads::system_info(state.downloads.library_dir())))
}

async fn pack_download(State(state): State<Arc<HubState>>, _caller: Caller, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    // Knowledge packs need the library engine; queue it first if it is missing.
    if let Some(pack) = state.downloads.catalog().pack(&id) {
        if pack.category == zaklon_core::catalog::Category::Knowledge && !state.downloads.is_installed("kiwix-tools") {
            let _ = state.downloads.enqueue("kiwix-tools");
        }
    }
    state.downloads.enqueue(&id).map_err(|e| bad(&e))?;
    Ok(StatusCode::ACCEPTED)
}

async fn pack_pause(State(state): State<Arc<HubState>>, _caller: Caller, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    state.downloads.pause(&id).map_err(|e| bad(&e))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn pack_remove(State(state): State<Arc<HubState>>, caller: Caller, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    tracing::info!(by = %caller.actor(), pack = %id, "pack removed");
    state.downloads.remove(&id).map_err(|e| bad(&e))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct DirBody {
    dir: String,
}

async fn packs_import(State(state): State<Arc<HubState>>, _: Local, Json(body): Json<DirBody>) -> Result<Json<serde_json::Value>, ApiError> {
    let dir = std::path::PathBuf::from(body.dir.trim());
    if !dir.is_dir() {
        return Err(bad("that folder does not exist"));
    }
    let d = state.downloads.clone();
    let imported = tokio::task::spawn_blocking(move || d.import_from_dir(&dir))
        .await
        .map_err(|e| anyhow::anyhow!(e))?
        .map_err(|e| bad(&e))?;
    Ok(Json(serde_json::json!({ "imported": imported })))
}

async fn pack_export(State(state): State<Arc<HubState>>, _: Local, Path(id): Path<String>, Json(body): Json<DirBody>) -> Result<Json<serde_json::Value>, ApiError> {
    let dir = std::path::PathBuf::from(body.dir.trim());
    if !dir.is_dir() {
        return Err(bad("that folder does not exist"));
    }
    let d = state.downloads.clone();
    let target = tokio::task::spawn_blocking(move || d.export_to_dir(&id, &dir))
        .await
        .map_err(|e| anyhow::anyhow!(e))?
        .map_err(|e| bad(&e))?;
    Ok(Json(serde_json::json!({ "exported_to": target.display().to_string() })))
}
