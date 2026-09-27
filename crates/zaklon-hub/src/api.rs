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
/// After this many wrong codes, every open code is canceled (stops guessing).
const MAX_PAIRING_FAILURES: u32 = 20;
/// How long a phone may repeat a pairing request after the reply was lost.
const PAIR_REPLAY_WINDOW: Duration = Duration::from_secs(120);

/// Which listener a request came in on. Laptop trust exists only on the
/// loopback listener used by the desktop window; the network (TLS) listener
/// always requires a device token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Listener {
    Local,
    Network,
}

pub fn router(state: Arc<HubState>, listener: Listener) -> Router {
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
        .route("/api/export", get(export_status).post(export_start))
        .route("/api/export/cancel", post(export_cancel))
        .route("/api/drives", get(drives))
        .route("/api/updates", get(updates_state))
        .route("/api/updates/check", post(updates_check))
        .route("/api/updates/settings", post(updates_settings))
        .route("/api/backups", get(backups_list).post(backups_create))
        .route("/api/backups/restore", post(backups_restore))
        .route("/api/hardware", get(hardware))
        .route("/api/supplies/summary", get(supplies_summary))
        .route("/api/items", get(items_list).post(items_create))
        .route("/api/items/{id}", get(items_get).patch(items_update).delete(items_delete))
        .route("/api/items/{id}/adjust", post(items_adjust))
        .route("/api/barcodes/{code}", get(barcode_lookup))
        .route("/api/places", get(places_list).post(places_add))
        .route("/api/places/{id}", axum::routing::delete(places_delete))
        .route("/api/items/{id}/batches", post(batch_add))
        .route("/api/batches/{id}", axum::routing::patch(batch_update).delete(batch_delete))
        .route("/api/shopping", get(shopping_list).post(shopping_add))
        .route("/api/shopping/{id}/bought", post(shopping_bought))
        .route("/api/shopping/{id}/dismiss", post(shopping_dismiss))
        .route("/api/put-away", get(put_away_list))
        .route("/api/put-away/{id}", post(put_away))
        .route("/api/history", get(history))
        .route("/api/maps", get(maps_overview))
        .route("/api/maps/{country}/download", post(maps_country_download))
        .route("/api/maps/{country}", axum::routing::delete(maps_country_remove))
        .route("/api/assistant", get(assistant_overview))
        .route("/api/assistant/model", post(assistant_select))
        .route("/api/assistant/ask", post(assistant_ask))
        .route("/api/assistant/answers/{id}", get(assistant_answer))
        .route("/api/assistant/stop", post(assistant_stop))
        .route("/api/models", get(models_list))
        .route("/api/models/{id}/file", get(model_file))
        .route("/api/library", get(library_books))
        .route("/api/library/search", get(library_search))
        .route("/kiwix/{*rest}", get(kiwix_proxy))
        .route("/kiwix-lat/{*rest}", get(kiwix_proxy_latin))
        .fallback(crate::ui::serve)
        .layer(axum::Extension(listener))
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
    /// Who did it, as shown in the history.
    fn actor(&self) -> String {
        match self {
            Caller::Local => "laptop".into(),
            Caller::Device(d) => d.name.clone(),
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
        let listener = parts.extensions.get::<Listener>().copied().unwrap_or(Listener::Network);
        if listener == Listener::Network {
            return Err(unauthorized());
        }
        let peer = parts.extensions.get::<ConnectInfo<SocketAddr>>().map(|c| c.0);
        match peer {
            Some(addr) if addr.ip().is_loopback() && local_request_is_ours(parts, state.config().local_port) => {
                Ok(Caller::Local)
            }
            Some(addr) if addr.ip().is_loopback() => Err(forbidden("request from another website")),
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
        // A device that presents a token the hub no longer accepts (it was
        // removed) must hear so clearly, even on public pages, so the phone
        // can leave the hub instead of looking connected.
        let presented_token = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("Bearer "));
        match <Caller as FromRequestParts<Arc<HubState>>>::from_request_parts(parts, state).await {
            Ok(c) => Ok(Some(c)),
            Err(e @ ApiError(StatusCode::UNAUTHORIZED, _)) if presented_token => Err(e),
            // Anyone may see the public part of what an optional caller guards.
            Err(ApiError(StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN, _)) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// Someone allowed to *read library pages*: a paired device, or the laptop.
/// Library pages are shown sandboxed (no scripts, opaque origin), so the
/// browser marks their own styles and images as cross-site requests; those
/// must still load. Only the Host check (against DNS rebinding) applies here.
/// Nothing private is behind this: it is used only for read-only /kiwix pages.
pub struct LibraryReader;

impl FromRequestParts<Arc<HubState>> for LibraryReader {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &Arc<HubState>) -> Result<Self, Self::Rejection> {
        let has_token = parts.headers.get("authorization").is_some();
        let listener = parts.extensions.get::<Listener>().copied().unwrap_or(Listener::Network);
        if has_token || listener == Listener::Network {
            <Caller as FromRequestParts<Arc<HubState>>>::from_request_parts(parts, state).await?;
            return Ok(LibraryReader);
        }
        let loopback = parts.extensions.get::<ConnectInfo<SocketAddr>>().is_some_and(|c| c.0.ip().is_loopback());
        let port = state.config().local_port;
        let host_ok = parts
            .headers
            .get("host")
            .and_then(|v| v.to_str().ok())
            .is_none_or(|h| h.eq_ignore_ascii_case(&format!("127.0.0.1:{port}")) || h.eq_ignore_ascii_case(&format!("localhost:{port}")));
        if loopback && host_ok {
            Ok(LibraryReader)
        } else {
            Err(unauthorized())
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

/// A loopback request is trusted only when it comes from the hub's own pages
/// (or from a non-browser client). This stops other websites open in a browser
/// on the laptop from driving the hub (CSRF), and stops DNS-rebinding tricks.
fn local_request_is_ours(parts: &Parts, local_port: u16) -> bool {
    let header = |name: &str| parts.headers.get(name).and_then(|v| v.to_str().ok());
    let local_hosts = [format!("127.0.0.1:{local_port}"), format!("localhost:{local_port}")];
    if let Some(host) = header("host") {
        if !local_hosts.iter().any(|h| h.eq_ignore_ascii_case(host)) {
            return false;
        }
    }
    if header("sec-fetch-site").is_some_and(|v| v.eq_ignore_ascii_case("cross-site")) {
        return false;
    }
    match header("origin") {
        None => true,
        Some(origin) => local_hosts.iter().any(|h| origin.eq_ignore_ascii_case(&format!("http://{h}"))),
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
            Some(crate::discovery::shown_ipv4_addresses().iter().map(|a| a.to_string()).collect())
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
    *state.pairing_failures.lock().unwrap_or_else(|p| p.into_inner()) = 0;
    {
        let mut sessions = state.pairing.lock().unwrap_or_else(|p| p.into_inner());
        // Only the newest code is valid: showing a new QR cancels the old one.
        sessions.clear();
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
            install_port: cfg.install_port,
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
    /// Random value chosen by the phone; repeating the same request with the
    /// same nonce returns the same result instead of failing.
    #[serde(default)]
    nonce: Option<String>,
}

fn default_platform() -> String {
    "android".into()
}

#[derive(Serialize, Clone)]
pub struct Paired {
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
    let nonce = body.nonce.as_deref().map(str::trim).filter(|n| n.len() >= 16 && n.len() <= 128).map(str::to_string);

    // A repeat of a pairing that already succeeded (the phone never got the reply).
    if let Some(n) = &nonce {
        let mut recent = state.recent_pairs.lock().unwrap_or_else(|p| p.into_inner());
        recent.retain(|_, (at, _, _)| at.elapsed() < PAIR_REPLAY_WINDOW);
        if let Some((_, code, paired)) = recent.get(n) {
            if *code == body.code {
                return Ok(Json(paired.clone()));
            }
        }
    }

    let failure = {
        let mut sessions = state.pairing.lock().unwrap_or_else(|p| p.into_inner());
        sessions.retain(|_, s| s.expires_at > now);
        match sessions.get_mut(&body.code) {
            None => Some("pairing code is invalid or expired"),
            Some(session) => {
                let hash = state.db.get_setting("household_password_hash")?.unwrap_or_default();
                if pairing::verify_password(&body.password, &hash) {
                    sessions.remove(&body.code);
                    None
                } else {
                    session.failed_attempts += 1;
                    if session.failed_attempts >= MAX_PAIRING_ATTEMPTS {
                        sessions.remove(&body.code);
                        Some("too many attempts; start pairing again on the laptop")
                    } else {
                        Some("wrong household password")
                    }
                }
            }
        }
    };
    if let Some(msg) = failure {
        let exhausted = {
            let mut f = state.pairing_failures.lock().unwrap_or_else(|p| p.into_inner());
            *f += 1;
            *f >= MAX_PAIRING_FAILURES
        };
        if exhausted {
            state.pairing.lock().unwrap_or_else(|p| p.into_inner()).clear();
            tracing::warn!("too many wrong pairing attempts; open pairing codes canceled");
        }
        // Slow down guessing.
        tokio::time::sleep(Duration::from_millis(400)).await;
        return Err(forbidden(msg));
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
    let paired = Paired {
        device_id: device.id,
        device_token: token,
        hub_id: cfg.hub_id,
        hub_name: cfg.hub_name,
        fingerprint: state.identity.fingerprint.clone(),
    };
    if let Some(n) = nonce {
        state
            .recent_pairs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(n, (Instant::now(), body.code.clone(), paired.clone()));
    }
    Ok(Json(paired))
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
    // Map pieces (over a thousand) have their own screen and endpoint.
    let packs = d.snapshot().into_iter().filter(|v| v.pack.category != zaklon_core::catalog::Category::Maps).collect();
    Ok(Json(CatalogReply { packs, system: crate::downloads::system_info(d.library_dir()) }))
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
        // AI models need the AI engine.
        if pack.category == zaklon_core::catalog::Category::Model && !state.downloads.is_installed("llama-cpp") {
            let _ = state.downloads.enqueue("llama-cpp");
        }
        // A map piece: CoMaps needs the world overview first, phones need the app.
        if pack.category == zaklon_core::catalog::Category::Maps {
            for dep in zaklon_core::maps::BASE_IDS.into_iter().chain([zaklon_core::maps::COMAPS_APK_ID]) {
                if dep != id && !state.downloads.is_installed(dep) {
                    let _ = state.downloads.enqueue(dep);
                }
            }
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
    // The library engine keeps knowledge packs (and its own files) open;
    // stop it so Windows lets us delete them. It restarts by itself.
    state.library.stop_for(Duration::from_secs(10)).await;
    let d = state.downloads.clone();
    tokio::task::spawn_blocking(move || d.remove(&id))
        .await
        .map_err(|e| anyhow::anyhow!(e))?
        .map_err(|e| bad(&e))?;
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

#[derive(Deserialize)]
struct ExportBody {
    dir: String,
    ids: Vec<String>,
}

/// Copy packs to a folder (a USB drive) in the background. Laptop only:
/// it writes to the laptop's own drives.
async fn export_start(State(state): State<Arc<HubState>>, _: Local, Json(body): Json<ExportBody>) -> Result<StatusCode, ApiError> {
    let dir = std::path::PathBuf::from(body.dir.trim());
    let (ex, d) = (state.export.clone(), state.downloads.clone());
    tokio::task::spawn_blocking(move || ex.start(&d, &body.ids, &dir))
        .await
        .map_err(|e| anyhow::anyhow!(e))?
        .map_err(|e| bad(&e))?;
    tracing::info!(files = state.export.state().files_total, "copy to drive started");
    Ok(StatusCode::ACCEPTED)
}

async fn export_status(State(state): State<Arc<HubState>>, _: Local) -> Json<crate::export::ExportState> {
    Json(state.export.state())
}

async fn export_cancel(State(state): State<Arc<HubState>>, _: Local) -> StatusCode {
    state.export.cancel();
    StatusCode::NO_CONTENT
}

/// Drives of the laptop, for copying packs to and from USB.
async fn drives(_: Local) -> Result<Json<Vec<crate::machine::Drive>>, ApiError> {
    Ok(Json(tokio::task::spawn_blocking(crate::machine::drives).await.map_err(|e| anyhow::anyhow!(e))?))
}

async fn hardware(_caller: Caller) -> Result<Json<crate::machine::Hardware>, ApiError> {
    Ok(Json(tokio::task::spawn_blocking(crate::machine::hardware).await.map_err(|e| anyhow::anyhow!(e))?))
}

// ---- library ----------------------------------------------------------------

#[derive(Serialize)]
struct LibraryReply {
    engine: crate::kiwix::EngineState,
    books: Vec<crate::kiwix::Book>,
}

async fn library_books(State(state): State<Arc<HubState>>, _caller: Caller) -> Json<LibraryReply> {
    Json(LibraryReply { engine: state.library.state(), books: state.library.books() })
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    #[serde(default)]
    book: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn library_search(
    State(state): State<Arc<HubState>>,
    _caller: Caller,
    axum::extract::Query(q): axum::extract::Query<SearchQuery>,
) -> Json<Vec<crate::kiwix::SearchResult>> {
    let limit = q.limit.unwrap_or(25).clamp(1, 50);
    Json(state.library.search(&q.q, q.book.as_deref(), limit).await)
}

/// Articles, images and styles of installed knowledge packs, read-only.
async fn kiwix_proxy(State(state): State<Arc<HubState>>, _reader: LibraryReader, uri: axum::http::Uri) -> Response {
    let path = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
    if !path.starts_with("/kiwix/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    proxy_library(&state, path, false).await
}

/// The same pages with Serbian Cyrillic text shown in Latin script. Relative
/// links inside the page stay under /kiwix-lat/, so reading on stays in Latin.
async fn kiwix_proxy_latin(State(state): State<Arc<HubState>>, _reader: LibraryReader, uri: axum::http::Uri) -> Response {
    let path = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
    let Some(rest) = path.strip_prefix("/kiwix-lat/") else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // Only pages of Serbian books are converted.
    let book = rest.strip_prefix("content/").and_then(|r| r.split('/').next()).unwrap_or_default();
    let serbian = state
        .library
        .books()
        .iter()
        .any(|b| b.name == book && b.languages.iter().any(|l| l == "srp"));
    proxy_library(&state, &format!("/kiwix/{rest}"), serbian).await
}

async fn proxy_library(state: &Arc<HubState>, path: &str, to_latin: bool) -> Response {
    match state.library.fetch(path).await {
        Ok(res) => {
            let status = StatusCode::from_u16(res.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let ctype = res
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("application/octet-stream")
                .to_string();
            let is_html = ctype.starts_with("text/html");
            let cache = if is_html { "no-cache" } else { "private, max-age=86400" };
            match res.bytes().await {
                Ok(body) => {
                    let body = if to_latin && is_html {
                        match std::str::from_utf8(&body) {
                            Ok(text) => {
                                let converted = crate::latin::html_to_latin(text);
                                axum::body::Bytes::from(converted)
                            }
                            Err(_) => body,
                        }
                    } else {
                        body
                    };
                    (
                    status,
                    [
                        (axum::http::header::CONTENT_TYPE, ctype),
                        (axum::http::header::CACHE_CONTROL, cache.to_string()),
                        (axum::http::header::HeaderName::from_static("x-content-type-options"), "nosniff".to_string()),
                        // Library pages never run scripts and get a unique origin,
                        // even if another website opens them in a new tab.
                        (axum::http::header::CONTENT_SECURITY_POLICY, "sandbox allow-popups".to_string()),
                    ],
                    body,
                )
                    .into_response()
                }
                Err(_) => StatusCode::BAD_GATEWAY.into_response(),
            }
        }
        Err(_) => (StatusCode::SERVICE_UNAVAILABLE, "library engine is not running").into_response(),
    }
}

// ---- supplies ---------------------------------------------------------------

use zaklon_core::supplies::{ItemInput, Item};

fn not_found_item() -> ApiError {
    not_found("no such item")
}

/// Validation problems from the storage layer are the caller's fault.
fn invalid(e: anyhow::Error) -> ApiError {
    let msg = e.to_string();
    if msg.contains("required") || msg.contains("unknown") || msg.contains("must be") || msg.contains("several batches") {
        bad(&msg)
    } else {
        ApiError::from(e)
    }
}

async fn supplies_summary(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<zaklon_core::supplies::Summary>, ApiError> {
    Ok(Json(state.db.supplies_summary()?))
}

async fn items_list(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<Vec<Item>>, ApiError> {
    Ok(Json(state.db.list_items()?))
}

async fn items_get(State(state): State<Arc<HubState>>, _caller: Caller, Path(id): Path<String>) -> Result<Json<Item>, ApiError> {
    state.db.get_item(&id)?.map(Json).ok_or_else(not_found_item)
}

async fn items_create(State(state): State<Arc<HubState>>, caller: Caller, Json(body): Json<ItemInput>) -> Result<(StatusCode, Json<Item>), ApiError> {
    let item = state.db.create_item(body, &caller.actor()).map_err(invalid)?;
    Ok((StatusCode::CREATED, Json(item)))
}

async fn items_update(State(state): State<Arc<HubState>>, caller: Caller, Path(id): Path<String>, Json(body): Json<ItemInput>) -> Result<Json<Item>, ApiError> {
    state.db.update_item(&id, body, &caller.actor()).map_err(invalid)?.map(Json).ok_or_else(not_found_item)
}

#[derive(Deserialize)]
struct AdjustBody {
    delta: f64,
}

async fn items_adjust(State(state): State<Arc<HubState>>, caller: Caller, Path(id): Path<String>, Json(body): Json<AdjustBody>) -> Result<Json<Item>, ApiError> {
    if !body.delta.is_finite() || body.delta == 0.0 {
        return Err(bad("delta must be a non-zero number"));
    }
    state.db.adjust_item(&id, body.delta, &caller.actor())?.map(Json).ok_or_else(not_found_item)
}

async fn items_delete(State(state): State<Arc<HubState>>, caller: Caller, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    if state.db.delete_item(&id, &caller.actor())? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found_item())
    }
}

#[derive(Serialize)]
struct BarcodeReply {
    barcode: String,
    /// An item in stock with this barcode, if any.
    item: Option<Item>,
    /// What this barcode was called before, if it was ever used.
    known: Option<zaklon_core::supplies::KnownBarcode>,
}

async fn barcode_lookup(State(state): State<Arc<HubState>>, _caller: Caller, Path(code): Path<String>) -> Result<Json<BarcodeReply>, ApiError> {
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

async fn places_list(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<Vec<zaklon_core::supplies::Place>>, ApiError> {
    Ok(Json(state.db.list_places()?))
}

#[derive(Deserialize)]
struct NameBody {
    name: String,
}

async fn places_add(State(state): State<Arc<HubState>>, _caller: Caller, Json(body): Json<NameBody>) -> Result<Json<zaklon_core::supplies::Place>, ApiError> {
    Ok(Json(state.db.add_place(&body.name).map_err(invalid)?))
}

async fn places_delete(State(state): State<Arc<HubState>>, _caller: Caller, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    if state.db.delete_place(&id)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found("no such place"))
    }
}

async fn shopping_list(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<Vec<zaklon_core::supplies::ShoppingEntry>>, ApiError> {
    Ok(Json(state.db.shopping_list()?))
}

#[derive(Deserialize)]
struct ShoppingBody {
    text: String,
    #[serde(default)]
    quantity: Option<f64>,
    #[serde(default)]
    unit: Option<String>,
    #[serde(default)]
    item_id: Option<String>,
}

async fn shopping_add(State(state): State<Arc<HubState>>, caller: Caller, Json(b): Json<ShoppingBody>) -> Result<(StatusCode, Json<zaklon_core::supplies::ShoppingEntry>), ApiError> {
    let e = state.db.add_shopping(&b.text, b.quantity, b.unit, b.item_id, &caller.actor()).map_err(invalid)?;
    Ok((StatusCode::CREATED, Json(e)))
}

/// "Bought": moves an entry (or a running-low suggestion "low:<item>") to put away.
async fn shopping_bought(State(state): State<Arc<HubState>>, caller: Caller, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    if state.db.mark_bought(&id, &caller.actor())? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found("no such entry"))
    }
}

/// "Delete" on the shopping list: not bought.
async fn shopping_dismiss(State(state): State<Arc<HubState>>, caller: Caller, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    if state.db.dismiss(&id, &caller.actor())? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found("no such entry"))
    }
}

async fn put_away_list(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<Vec<zaklon_core::supplies::ShoppingEntry>>, ApiError> {
    Ok(Json(state.db.to_put_away()?))
}

async fn put_away(
    State(state): State<Arc<HubState>>,
    caller: Caller,
    Path(id): Path<String>,
    Json(body): Json<zaklon_core::supplies::PutAwayInput>,
) -> Result<Json<Item>, ApiError> {
    state.db.put_away(&id, body, &caller.actor()).map_err(invalid)?.map(Json).ok_or_else(|| not_found("no such entry"))
}

async fn batch_add(
    State(state): State<Arc<HubState>>,
    caller: Caller,
    Path(id): Path<String>,
    Json(body): Json<zaklon_core::supplies::BatchInput>,
) -> Result<Json<Item>, ApiError> {
    state.db.add_batch(&id, body, &caller.actor()).map_err(invalid)?.map(Json).ok_or_else(not_found_item)
}

async fn batch_update(
    State(state): State<Arc<HubState>>,
    caller: Caller,
    Path(id): Path<String>,
    Json(body): Json<zaklon_core::supplies::BatchInput>,
) -> Result<Json<Item>, ApiError> {
    state.db.update_batch(&id, body, &caller.actor()).map_err(invalid)?.map(Json).ok_or_else(|| not_found("no such batch"))
}

async fn batch_delete(State(state): State<Arc<HubState>>, caller: Caller, Path(id): Path<String>) -> Result<Json<Item>, ApiError> {
    state.db.delete_batch(&id, &caller.actor()).map_err(invalid)?.map(Json).ok_or_else(|| not_found("no such batch"))
}

#[derive(Deserialize)]
struct HistoryQuery {
    #[serde(default)]
    limit: Option<usize>,
}

async fn history(State(state): State<Arc<HubState>>, _caller: Caller, axum::extract::Query(q): axum::extract::Query<HistoryQuery>) -> Result<Json<Vec<zaklon_core::supplies::HistoryEntry>>, ApiError> {
    Ok(Json(state.db.history(q.limit.unwrap_or(100).clamp(1, 500))?))
}

// ---- AI models for phones -----------------------------------------------------

#[derive(Serialize)]
struct ModelInfo {
    id: String,
    title_en: String,
    title_sr: String,
    file: String,
    size: u64,
    sha256: String,
}

/// Installed AI models a phone can copy from the hub.
async fn models_list(State(state): State<Arc<HubState>>, _caller: Caller) -> Json<Vec<ModelInfo>> {
    let models = state
        .downloads
        .snapshot()
        .into_iter()
        .filter(|v| {
            v.pack.category == zaklon_core::catalog::Category::Model
                && v.state.status == zaklon_core::catalog::PackStatus::Installed
        })
        .filter_map(|v| {
            let f = v.pack.files.first()?.clone();
            Some(ModelInfo {
                id: v.pack.id.clone(),
                title_en: v.pack.title.en.clone(),
                title_sr: v.pack.title.sr.clone(),
                file: std::path::Path::new(&f.path).file_name()?.to_string_lossy().to_string(),
                size: f.size,
                sha256: f.sha256.clone(),
            })
        })
        .collect();
    Json(models)
}

/// Stream an installed model file, with `Range: bytes=N-` support so a
/// phone can resume a large copy after the Wi-Fi drops.
async fn model_file(
    State(state): State<Arc<HubState>>,
    _caller: Caller,
    Path(id): Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<Response, ApiError> {
    use axum::http::header;
    use tokio::io::AsyncSeekExt;

    let pack = state.downloads.catalog().pack(&id).cloned().ok_or_else(|| not_found("no such model"))?;
    if pack.category != zaklon_core::catalog::Category::Model || !state.downloads.is_installed(&id) {
        return Err(not_found("model is not installed on the hub"));
    }
    let f = pack.files.first().ok_or_else(|| not_found("no file"))?;
    let path = state.downloads.library_dir().join(&f.path);
    let mut file = tokio::fs::File::open(&path).await.map_err(|e| anyhow::anyhow!(e))?;
    let total = file.metadata().await.map_err(|e| anyhow::anyhow!(e))?.len();

    let start = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .and_then(|v| v.split('-').next())
        .and_then(|v| v.trim().parse::<u64>().ok());
    let name = std::path::Path::new(&f.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();

    match start {
        Some(from) if from >= total => Ok((StatusCode::RANGE_NOT_SATISFIABLE, [(header::CONTENT_RANGE, format!("bytes */{total}"))]).into_response()),
        Some(from) => {
            file.seek(std::io::SeekFrom::Start(from)).await.map_err(|e| anyhow::anyhow!(e))?;
            let body = axum::body::Body::from_stream(tokio_util::io::ReaderStream::with_capacity(file, 1 << 20));
            Ok((
                StatusCode::PARTIAL_CONTENT,
                [
                    (header::CONTENT_TYPE, "application/octet-stream".to_string()),
                    (header::CONTENT_LENGTH, (total - from).to_string()),
                    (header::CONTENT_RANGE, format!("bytes {from}-{}/{total}", total - 1)),
                    (header::ACCEPT_RANGES, "bytes".to_string()),
                    (header::HeaderName::from_static("x-zaklon-file"), name),
                    (header::HeaderName::from_static("x-zaklon-sha256"), f.sha256.clone()),
                ],
                body,
            )
                .into_response())
        }
        None => {
            let body = axum::body::Body::from_stream(tokio_util::io::ReaderStream::with_capacity(file, 1 << 20));
            Ok((
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, "application/octet-stream".to_string()),
                    (header::CONTENT_LENGTH, total.to_string()),
                    (header::ACCEPT_RANGES, "bytes".to_string()),
                    (header::HeaderName::from_static("x-zaklon-file"), name),
                    (header::HeaderName::from_static("x-zaklon-sha256"), f.sha256.clone()),
                ],
                body,
            )
                .into_response())
        }
    }
}

// ---- maps -------------------------------------------------------------------------

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
struct MapsReply {
    version: u64,
    /// What to type into CoMaps as the map download server.
    server_urls: Vec<String>,
    /// Where phones download the CoMaps app from the hub, once it is on the hub.
    app_urls: Vec<String>,
    app: zaklon_core::catalog::PackState,
    installed_bytes: u64,
    countries: Vec<MapCountryView>,
}

async fn maps_overview(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<MapsReply>, ApiError> {
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
async fn maps_country_download(State(state): State<Arc<HubState>>, _caller: Caller, Path(country): Path<String>) -> Result<StatusCode, ApiError> {
    let ids = country_regions(&country).ok_or_else(|| not_found("no such country"))?;
    // CoMaps needs the world overview first, and phones need the app.
    for dep in zaklon_core::maps::BASE_IDS.into_iter().chain([zaklon_core::maps::COMAPS_APK_ID]) {
        if !state.downloads.is_installed(dep) {
            let _ = state.downloads.enqueue(dep);
        }
    }
    for id in ids {
        if !state.downloads.is_installed(&id) {
            state.downloads.enqueue(&id).map_err(|e| bad(&e))?;
        }
    }
    Ok(StatusCode::ACCEPTED)
}

async fn maps_country_remove(State(state): State<Arc<HubState>>, caller: Caller, Path(country): Path<String>) -> Result<StatusCode, ApiError> {
    let ids = country_regions(&country).ok_or_else(|| not_found("no such country"))?;
    tracing::info!(by = %caller.actor(), country = %country, "maps removed");
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
        tokio::task::spawn_blocking(move || d.remove(&id)).await.map_err(|e| anyhow::anyhow!(e))?.map_err(|e| bad(&e))?;
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---- assistant --------------------------------------------------------------------

async fn assistant_overview(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<crate::assistant::Overview>, ApiError> {
    let a = state.assistant.clone();
    Ok(Json(tokio::task::spawn_blocking(move || a.overview()).await.map_err(|e| anyhow::anyhow!(e))?))
}

#[derive(Deserialize)]
struct SelectModelBody {
    id: String,
}

async fn assistant_select(State(state): State<Arc<HubState>>, _caller: Caller, Json(body): Json<SelectModelBody>) -> Result<StatusCode, ApiError> {
    state.assistant.select(&body.id).map_err(|e| bad(&e))?;
    state.db.set_setting(crate::assistant::SETTING_MODEL, &body.id)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct AskBody {
    question: String,
    #[serde(default)]
    language: String,
    #[serde(default)]
    history: Vec<crate::assistant::Turn>,
}

async fn assistant_ask(State(state): State<Arc<HubState>>, caller: Caller, Json(body): Json<AskBody>) -> Result<Json<serde_json::Value>, ApiError> {
    // The assistant can answer about the supplies and propose changes to them.
    let items = state.db.list_items().unwrap_or_default();
    let id = state.assistant.ask(&body.question, &body.language, body.history, items).map_err(|e| bad(&e))?;
    tracing::info!(by = %caller.actor(), "assistant asked");
    Ok(Json(serde_json::json!({ "id": id })))
}

async fn assistant_answer(State(state): State<Arc<HubState>>, _caller: Caller, Path(id): Path<String>) -> Result<Json<crate::assistant::Answer>, ApiError> {
    state.assistant.answer(&id).map(Json).ok_or_else(|| not_found("no such answer"))
}

async fn assistant_stop(State(state): State<Arc<HubState>>, _caller: Caller) -> StatusCode {
    state.assistant.stop().await;
    StatusCode::NO_CONTENT
}

// ---- backups ----------------------------------------------------------------------

#[derive(Serialize)]
struct BackupsReply {
    backups: Vec<crate::backup::BackupFile>,
    /// A restore is waiting for the next start.
    restore_pending: bool,
    folder: String,
}

async fn backups_list(State(state): State<Arc<HubState>>, _: Local) -> Result<Json<BackupsReply>, ApiError> {
    let cfg = state.config();
    let root = cfg.root.clone();
    let backups = tokio::task::spawn_blocking(move || crate::backup::list(&cfg)).await.map_err(|e| anyhow::anyhow!(e))?;
    Ok(Json(BackupsReply { backups, restore_pending: crate::backup::restore_pending(&root), folder: state.config().backups_dir().display().to_string() }))
}

#[derive(Deserialize)]
struct BackupBody {
    /// Where to save it; the backups folder when empty.
    #[serde(default)]
    dir: String,
}

async fn backups_create(State(state): State<Arc<HubState>>, _: Local, Json(body): Json<BackupBody>) -> Result<Json<serde_json::Value>, ApiError> {
    let cfg = state.config();
    let dir = if body.dir.trim().is_empty() { cfg.backups_dir() } else { std::path::PathBuf::from(body.dir.trim()) };
    if !dir.is_dir() {
        return Err(bad("that folder does not exist"));
    }
    let st = state.clone();
    let path = tokio::task::spawn_blocking(move || crate::backup::create(&cfg, &st.db, &dir, false))
        .await
        .map_err(|e| anyhow::anyhow!(e))?
        .map_err(|e| bad(&e))?;
    Ok(Json(serde_json::json!({ "path": path.display().to_string() })))
}

#[derive(Deserialize)]
struct RestoreBody {
    path: String,
}

/// Check a backup and prepare it; it replaces the data on the next start.
async fn backups_restore(State(state): State<Arc<HubState>>, _: Local, Json(body): Json<RestoreBody>) -> Result<Json<crate::backup::Manifest>, ApiError> {
    let cfg = state.config();
    let path = std::path::PathBuf::from(body.path.trim().trim_matches('"'));
    if !path.is_file() {
        return Err(bad("that file does not exist"));
    }
    // Keep today's data too, whatever happens next.
    let st = state.clone();
    let cfg2 = cfg.clone();
    tokio::task::spawn_blocking(move || crate::backup::create(&cfg2, &st.db, &cfg2.backups_dir(), false))
        .await
        .map_err(|e| anyhow::anyhow!(e))?
        .map_err(|e| bad(&e))?;
    let manifest = tokio::task::spawn_blocking(move || crate::backup::stage_restore(&cfg, &path))
        .await
        .map_err(|e| anyhow::anyhow!(e))?
        .map_err(|e| bad(&e))?;
    Ok(Json(manifest))
}

// ---- updates ----------------------------------------------------------------------

async fn updates_state(State(state): State<Arc<HubState>>, _caller: Caller) -> Json<crate::updates::UpdateState> {
    Json(state.updates.state())
}

async fn updates_check(State(state): State<Arc<HubState>>, _caller: Caller) -> Json<crate::updates::UpdateState> {
    Json(state.updates.check().await)
}

#[derive(Deserialize)]
struct UpdateSettings {
    enabled: bool,
}

async fn updates_settings(State(state): State<Arc<HubState>>, _: Local, Json(body): Json<UpdateSettings>) -> Result<StatusCode, ApiError> {
    {
        let mut cfg = state.config.lock().unwrap_or_else(|p| p.into_inner());
        cfg.auto_update_check = body.enabled;
        cfg.save()?;
    }
    state.updates.set_enabled(body.enabled);
    Ok(StatusCode::NO_CONTENT)
}
