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
/// After this many wrong codes, every open code is cancelled (stops guessing).
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
        .route("/api/packs/{id}/export", post(pack_export))
        .route("/api/supplies/summary", get(supplies_summary))
        .route("/api/items", get(items_list).post(items_create))
        .route("/api/items/{id}", get(items_get).patch(items_update).delete(items_delete))
        .route("/api/items/{id}/adjust", post(items_adjust))
        .route("/api/barcodes/{code}", get(barcode_lookup))
        .route("/api/places", get(places_list).post(places_add))
        .route("/api/places/{id}", axum::routing::delete(places_delete))
        .route("/api/shopping", get(shopping_list).post(shopping_add))
        .route("/api/shopping/clear-done", post(shopping_clear_done))
        .route("/api/shopping/{id}", axum::routing::patch(shopping_update).delete(shopping_delete))
        .route("/api/history", get(history))
        .route("/api/library", get(library_books))
        .route("/api/library/search", get(library_search))
        .route("/kiwix/{*rest}", get(kiwix_proxy))
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
        match <Caller as FromRequestParts<Arc<HubState>>>::from_request_parts(parts, state).await {
            Ok(c) => Ok(Some(c)),
            // Anyone may see the public part of what an optional caller guards.
            Err(ApiError(StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN, _)) => Ok(None),
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
            tracing::warn!("too many wrong pairing attempts; open pairing codes cancelled");
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
async fn kiwix_proxy(State(state): State<Arc<HubState>>, _caller: Caller, uri: axum::http::Uri) -> Response {
    let path = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
    if !path.starts_with("/kiwix/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    match state.library.fetch(path).await {
        Ok(res) => {
            let status = StatusCode::from_u16(res.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let ctype = res
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("application/octet-stream")
                .to_string();
            let cache = if ctype.starts_with("text/html") { "no-cache" } else { "private, max-age=86400" };
            match res.bytes().await {
                Ok(body) => (
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
                    .into_response(),
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
    if msg.contains("required") || msg.contains("unknown") || msg.contains("must be") {
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

#[derive(Deserialize)]
struct DoneBody {
    done: bool,
}

async fn shopping_update(State(state): State<Arc<HubState>>, caller: Caller, Path(id): Path<String>, Json(b): Json<DoneBody>) -> Result<StatusCode, ApiError> {
    if state.db.set_shopping_done(&id, b.done, &caller.actor())? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found("no such entry"))
    }
}

async fn shopping_delete(State(state): State<Arc<HubState>>, _caller: Caller, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    if state.db.delete_shopping(&id)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found("no such entry"))
    }
}

async fn shopping_clear_done(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<serde_json::Value>, ApiError> {
    Ok(Json(serde_json::json!({ "removed": state.db.clear_done_shopping()? })))
}

#[derive(Deserialize)]
struct HistoryQuery {
    #[serde(default)]
    limit: Option<usize>,
}

async fn history(State(state): State<Arc<HubState>>, _caller: Caller, axum::extract::Query(q): axum::extract::Query<HistoryQuery>) -> Result<Json<Vec<zaklon_core::supplies::HistoryEntry>>, ApiError> {
    Ok(Json(state.db.history(q.limit.unwrap_or(100).clamp(1, 500))?))
}
