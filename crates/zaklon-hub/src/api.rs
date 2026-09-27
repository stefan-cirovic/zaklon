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
use zaklon_core::dates::now_rfc3339;
use zaklon_core::db::Device;
use zaklon_core::pairing;

use crate::{HubState, PairingFailure, PairingSession, VERSION};

const PAIRING_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_PAIRING_ATTEMPTS: u8 = 3;
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
        .route("/api/firewall", get(firewall_status))
        .route("/api/firewall/allow", post(firewall_allow))
        .route("/api/hotspot", get(hotspot_status))
        .route("/api/hotspot/start", post(hotspot_start))
        .route("/api/hotspot/stop", post(hotspot_stop))
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
        .route("/api/maps-app", get(maps_app_file))
        .route("/api/maps/{country}/download", post(maps_country_download))
        .route("/api/maps/{country}", axum::routing::delete(maps_country_remove))
        .route("/api/assistant", get(assistant_overview))
        .route("/api/assistant/model", post(assistant_select))
        .route("/api/assistant/ask", post(assistant_ask))
        .route("/api/assistant/answers/{id}", get(assistant_answer))
        .route("/api/assistant/answers/{id}/cancel", post(assistant_cancel))
        .route("/api/assistant/stop", post(assistant_stop))
        .route("/api/assistant/warm", post(assistant_warm))
        .route("/api/memory", get(memory_list).post(memory_add))
        .route("/api/memory/{id}", axum::routing::delete(memory_delete))
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
        // The sentence is for people reading logs; the code is what the app
        // translates, so rewording a message can never break a translation.
        let code = error_code(self.0, &self.1);
        (self.0, Json(serde_json::json!({ "error": self.1, "code": code }))).into_response()
    }
}

/// Stable codes for the app to translate, found from the message; the first
/// entry the message contains wins. One table, next to where the messages
/// come from. The tests below keep it honest: every message gets its code,
/// every entry still matches a message, and the app translates every code.
const ERROR_CODES: &[(&str, &str)] = &[
    ("wrong household password", "wrong_password"),
    ("pairing code is invalid or expired", "code_expired"),
    ("too many wrong attempts from this device", "device_blocked"),
    ("too many attempts", "too_many_attempts"),
    ("password must be at least", "password_too_short"),
    ("set a household password first", "not_set_up"),
    ("already set up", "already_set_up"),
    ("only the laptop can do this", "laptop_only"),
    ("request from another website", "cross_site"),
    ("not enough free disk space", "no_disk_space"),
    ("not enough space", "drive_full"),
    ("formatted as FAT32", "fat32"),
    ("checksum mismatch", "checksum"),
    ("expiry must be a date", "bad_date"),
    ("must be a date", "bad_date"),
    ("name is required", "name_required"),
    ("text is required", "text_required"),
    ("that name is reserved", "name_reserved"),
    ("that folder does not exist", "no_folder"),
    ("that file does not exist", "no_file"),
    ("pause the download first", "pause_first"),
    ("could not delete", "delete_failed"),
    ("not a Zaklon backup", "not_a_backup"),
    ("backup is incomplete", "not_a_backup"),
    ("database is damaged", "not_a_backup"),
    ("settings are damaged", "not_a_backup"),
    ("made by a newer Zaklon", "newer_backup"),
    ("a copy is already running", "copy_running"),
    ("writing to the drive", "drive_write"),
    ("outside the library", "outside_library"),
    ("nothing selected", "nothing_selected"),
    ("no AI model is installed", "no_model"),
    ("AI engine is not installed", "no_ai_engine"),
    ("stopped while loading the model", "ai_memory"),
    ("AI engine was stopped", "ai_stopped"),
    ("assistant is busy", "ai_busy"),
    ("question is too long", "question_too_long"),
    ("ask something first", "question_empty"),
    ("note is too long", "note_too_long"),
    ("remembers too much", "notes_full"),
    ("model is not installed on the hub", "model_not_on_hub"),
    ("is not installed", "not_installed"),
    ("cannot be copied", "cannot_copy"),
    ("delta must be", "bad_quantity"),
    ("quantity must be", "bad_quantity"),
    ("several batches", "several_batches"),
    ("unknown category", "bad_category"),
    ("bad barcode", "bad_barcode"),
    ("no such", "not_found"),
    ("no file", "not_found"),
    ("unauthorized", "unauthorized"),
    ("internal error", "internal"),
];

pub fn error_code(status: StatusCode, msg: &str) -> &'static str {
    if let Some((_, code)) = ERROR_CODES.iter().find(|(needle, _)| msg.contains(needle)) {
        return code;
    }
    match status {
        StatusCode::NOT_FOUND => "not_found",
        StatusCode::UNAUTHORIZED => "unauthorized",
        StatusCode::FORBIDDEN => "forbidden",
        StatusCode::TOO_MANY_REQUESTS => "too_many_attempts",
        s if s.is_server_error() => "internal",
        _ => "other",
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

/// Runs slow work that blocks (disk, Argon2, PowerShell, copying the
/// database) on a thread made for it, so the async workers keep answering
/// phones and the laptop window meanwhile.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f).await.map_err(|e| ApiError::from(anyhow::anyhow!("background task failed: {e}")))
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
    let hash = blocking(move || pairing::hash_password(&body.password)).await??;
    state.db.set_setting("household_password_hash", &hash)?;
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
    let hash = blocking(move || pairing::hash_password(&body.new_password)).await??;
    state.db.set_setting("household_password_hash", &hash)?;
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
    state.pairing_failures.lock().unwrap_or_else(|p| p.into_inner()).reset_total();
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

/// Checks a pairing code and the household password. Returns the reason on
/// failure. The password check (Argon2, slow on purpose) runs on a blocking
/// thread without holding the pairing lock; an attempt is reserved before it
/// starts, so parallel guesses still count toward the per-code limit.
async fn pair_check(
    state: &Arc<HubState>,
    code: &str,
    password: &str,
    now: Instant,
) -> Result<Option<&'static str>, ApiError> {
    {
        let mut sessions = state.pairing.lock().unwrap_or_else(|p| p.into_inner());
        sessions.retain(|_, s| s.expires_at > now);
        match sessions.get_mut(code) {
            None => return Ok(Some("pairing code is invalid or expired")),
            Some(session) if session.failed_attempts >= MAX_PAIRING_ATTEMPTS => {
                sessions.remove(code);
                return Ok(Some("too many attempts; start pairing again on the laptop"));
            }
            Some(session) => session.failed_attempts += 1,
        }
    }
    let hash = state.db.get_setting("household_password_hash")?.unwrap_or_default();
    let password = password.to_string();
    let ok = blocking(move || pairing::verify_password(&password, &hash)).await?;

    let mut sessions = state.pairing.lock().unwrap_or_else(|p| p.into_inner());
    sessions.retain(|_, s| s.expires_at > Instant::now());
    // The code may have been used, replaced, canceled or burned meanwhile.
    let Some(session) = sessions.get(code) else {
        return Ok(Some("pairing code is invalid or expired"));
    };
    if ok {
        sessions.remove(code);
        Ok(None)
    } else if session.failed_attempts >= MAX_PAIRING_ATTEMPTS {
        sessions.remove(code);
        Ok(Some("too many attempts; start pairing again on the laptop"))
    } else {
        Ok(Some("wrong household password"))
    }
}

async fn pair_complete(
    State(state): State<Arc<HubState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<PairComplete>,
) -> Result<Json<Paired>, ApiError> {
    let now = Instant::now();
    let ip = peer.ip();
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

    // A device that failed too often waits; others can still pair.
    if state.pairing_failures.lock().unwrap_or_else(|p| p.into_inner()).is_blocked(ip, now) {
        tokio::time::sleep(Duration::from_millis(400)).await;
        return Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "too many wrong attempts from this device; try again in a few minutes".into(),
        ));
    }

    let failure = pair_check(&state, &body.code, &body.password, now).await?;
    if let Some(msg) = failure {
        let outcome = state.pairing_failures.lock().unwrap_or_else(|p| p.into_inner()).record(ip, now);
        match outcome {
            PairingFailure::Counted => {}
            PairingFailure::AddressBlocked => {
                tracing::warn!(%ip, "too many wrong pairing attempts; this address is blocked for a while");
            }
            PairingFailure::CancelAll => {
                state.pairing.lock().unwrap_or_else(|p| p.into_inner()).clear();
                tracing::warn!("too many wrong pairing attempts overall; open pairing codes canceled");
            }
        }
        // Slow down guessing.
        tokio::time::sleep(Duration::from_millis(400)).await;
        return Err(forbidden(msg));
    }
    let token = pairing::random_token(32);
    let mut name: String = body.device_name.trim().chars().take(60).collect();
    if name.is_empty() || reserved_device_name(&name) {
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

/// A phone may rename or remove only itself; the laptop manages every phone.
fn own_device_or_laptop(caller: &Caller, id: &str) -> Result<(), ApiError> {
    match caller {
        Caller::Device(d) if d.id != id => Err(forbidden("only the laptop can do this")),
        _ => Ok(()),
    }
}

/// History names the laptop "laptop"; a phone must not look like it.
fn reserved_device_name(name: &str) -> bool {
    name.trim().eq_ignore_ascii_case("laptop")
}

async fn rename_device(
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

async fn delete_device(
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
    blocking(move || d.remove(&id)).await?.map_err(|e| bad(&e))?;
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
    let imported = blocking(move || d.import_from_dir(&dir)).await?.map_err(|e| bad(&e))?;
    Ok(Json(serde_json::json!({ "imported": imported })))
}

#[derive(Deserialize)]
struct ExportBody {
    dir: String,
    #[serde(default)]
    ids: Vec<String>,
    /// Also the Windows installer and the phone app, for a friend starting from nothing.
    #[serde(default)]
    with_apps: bool,
}

/// Copy packs to a folder (a USB drive) in the background. Laptop only:
/// it writes to the laptop's own drives.
async fn export_start(State(state): State<Arc<HubState>>, _: Local, Json(body): Json<ExportBody>) -> Result<StatusCode, ApiError> {
    let dir = std::path::PathBuf::from(body.dir.trim());
    let (ex, d) = (state.export.clone(), state.downloads.clone());
    blocking(move || ex.start(&d, &body.ids, &dir, body.with_apps)).await?.map_err(|e| bad(&e))?;
    tracing::info!(files = state.export.state().files_total, "copy to drive started");
    Ok(StatusCode::ACCEPTED)
}

#[derive(Serialize)]
struct ExportReply {
    #[serde(flatten)]
    state: crate::export::ExportState,
    /// Installer and phone app available to put on the stick, with sizes.
    apps: Vec<(String, u64)>,
}

async fn export_status(State(state): State<Arc<HubState>>, _: Local) -> Json<ExportReply> {
    let apps = crate::export::app_files(&state.downloads)
        .into_iter()
        .map(|(p, n)| (n, std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)))
        .collect();
    Json(ExportReply { state: state.export.state(), apps })
}

async fn export_cancel(State(state): State<Arc<HubState>>, _: Local) -> StatusCode {
    state.export.cancel();
    StatusCode::NO_CONTENT
}

/// Drives of the laptop, for copying packs to and from USB.
async fn drives(_: Local) -> Result<Json<Vec<crate::machine::Drive>>, ApiError> {
    Ok(Json(blocking(crate::machine::drives).await?))
}

async fn hardware(_caller: Caller) -> Result<Json<crate::machine::Hardware>, ApiError> {
    Ok(Json(blocking(crate::machine::hardware).await?))
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
/// A failure from the household data: a problem with what was asked (a
/// missing name, a bad date) is the asker's to fix; a failure of the database
/// or the disk is ours. Decided by the kind of error, not by its wording.
fn invalid(e: anyhow::Error) -> ApiError {
    let ours = e.chain().any(|c| c.downcast_ref::<zaklon_core::rusqlite::Error>().is_some() || c.downcast_ref::<std::io::Error>().is_some());
    if ours {
        ApiError::from(e)
    } else {
        bad(&e.to_string())
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

async fn places_delete(State(state): State<Arc<HubState>>, caller: Caller, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    if state.db.delete_place(&id, &caller.actor())? {
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
    /// Set by phones: the same add sent twice (a reply lost on the way, or
    /// resent from the phone's outbox) makes one entry, also after a restart.
    #[serde(default)]
    client_id: Option<String>,
}

async fn shopping_add(State(state): State<Arc<HubState>>, caller: Caller, Json(b): Json<ShoppingBody>) -> Result<(StatusCode, Json<zaklon_core::supplies::ShoppingEntry>), ApiError> {
    let e = match b.client_id.as_deref().filter(|c| !c.is_empty()) {
        Some(cid) => state.db.add_shopping_once(cid, &b.text, b.quantity, b.unit, b.item_id, &caller.actor()),
        None => state.db.add_shopping(&b.text, b.quantity, b.unit, b.item_id, &caller.actor()),
    }
    .map_err(invalid)?;
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
    let pack = state.downloads.catalog().pack(&id).cloned().ok_or_else(|| not_found("no such model"))?;
    if pack.category != zaklon_core::catalog::Category::Model || !state.downloads.is_installed(&id) {
        return Err(not_found("model is not installed on the hub"));
    }
    let f = pack.files.first().ok_or_else(|| not_found("no file"))?;
    stream_library_file(&state, f, &headers, "application/octet-stream").await
}

/// The CoMaps app for a paired phone, over the pinned TLS connection with
/// its SHA-256, so the phone can check it before offering to install it
/// (the plain-HTTP install page is only for phones that are not paired).
async fn maps_app_file(State(state): State<Arc<HubState>>, _caller: Caller, headers: axum::http::HeaderMap) -> Result<Response, ApiError> {
    let id = zaklon_core::maps::COMAPS_APK_ID;
    let pack = state.downloads.catalog().pack(id).cloned().ok_or_else(|| not_found("no such app"))?;
    if !state.downloads.is_installed(id) {
        return Err(not_found("the map app is not on the hub yet"));
    }
    let f = pack.files.first().filter(|f| !f.sha256.is_empty()).ok_or_else(|| not_found("no file"))?;
    stream_library_file(&state, f, &headers, "application/vnd.android.package-archive").await
}

/// Stream a verified file from the library with its SHA-256 (from the
/// catalog) and `Range: bytes=N-` support.
async fn stream_library_file(
    state: &HubState,
    f: &zaklon_core::catalog::PackFile,
    headers: &axum::http::HeaderMap,
    content_type: &str,
) -> Result<Response, ApiError> {
    use axum::http::header;
    use tokio::io::AsyncSeekExt;

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
                    (header::CONTENT_TYPE, content_type.to_string()),
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
                    (header::CONTENT_TYPE, content_type.to_string()),
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
        blocking(move || d.remove(&id)).await?.map_err(|e| bad(&e))?;
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---- assistant --------------------------------------------------------------------

async fn assistant_overview(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<crate::assistant::Overview>, ApiError> {
    let a = state.assistant.clone();
    Ok(Json(blocking(move || a.overview()).await?))
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
    /// Look things up on the internet too (switched on per conversation).
    #[serde(default)]
    online: bool,
}

async fn assistant_ask(State(state): State<Arc<HubState>>, caller: Caller, Json(body): Json<AskBody>) -> Result<Json<serde_json::Value>, ApiError> {
    // The assistant can answer about the supplies and propose changes to them.
    // It reads places by name, and a database failure fails the question:
    // an empty list would be answered as "nothing in the supplies", and the
    // household's notes (allergies) would be left out.
    // Place names in the language the answer will be in (as `Assistant::ask` picks it).
    let answer_language = zaklon_core::lang::question_language(&body.question).unwrap_or(if body.language == "sr" { "sr" } else { "en" });
    let items = state.db.list_items_for_reading(answer_language)?;
    let notes = state.db.list_notes()?;
    let ctx = crate::assistant::AskContext { history: body.history, items, notes, online: body.online };
    let id = state.assistant.ask(&body.question, &body.language, ctx).map_err(|e| bad(&e))?;
    tracing::info!(by = %caller.actor(), online = body.online, "assistant asked");
    Ok(Json(serde_json::json!({ "id": id })))
}

async fn assistant_answer(State(state): State<Arc<HubState>>, _caller: Caller, Path(id): Path<String>) -> Result<Json<crate::assistant::Answer>, ApiError> {
    state.assistant.answer(&id).map(Json).ok_or_else(|| not_found("no such answer"))
}

/// Stop an answer that waits or is being written; what was written so far stays.
async fn assistant_cancel(State(state): State<Arc<HubState>>, _caller: Caller, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    if state.assistant.cancel(&id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found("no such answer"))
    }
}

#[derive(Deserialize, Default)]
struct WarmBody {
    #[serde(default)]
    language: String,
}

async fn assistant_warm(State(state): State<Arc<HubState>>, _caller: Caller, body: Option<Json<WarmBody>>) -> StatusCode {
    let lang = body.map(|b| b.0.language).unwrap_or_default();
    state.assistant.warm_up(&lang);
    StatusCode::ACCEPTED
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
    let backups = blocking(move || crate::backup::list(&cfg)).await?;
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
    let path = blocking(move || crate::backup::create(&cfg, &st.db, &dir, false)).await?.map_err(|e| bad(&e))?;
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
    blocking(move || crate::backup::create(&cfg2, &st.db, &cfg2.backups_dir(), false)).await?.map_err(|e| bad(&e))?;
    let manifest = blocking(move || crate::backup::stage_restore(&cfg, &path)).await?.map_err(|e| bad(&e))?;
    Ok(Json(manifest))
}

// ---- updates ----------------------------------------------------------------------

async fn updates_state(State(state): State<Arc<HubState>>, _caller: Caller) -> Json<crate::updates::UpdateState> {
    Json(state.updates.state())
}

async fn updates_check(State(state): State<Arc<HubState>>, _caller: Caller) -> Json<crate::updates::UpdateState> {
    // At most one question to GitHub every ten minutes, whoever asks.
    if state.updates.checked_recently(Duration::from_secs(600)) {
        return Json(state.updates.state());
    }
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

// ---- the assistant's memory ------------------------------------------------------

async fn memory_list(State(state): State<Arc<HubState>>, _caller: Caller) -> Result<Json<Vec<zaklon_core::memory::Note>>, ApiError> {
    Ok(Json(state.db.list_notes()?))
}

#[derive(Deserialize)]
struct NoteBody {
    text: String,
}

async fn memory_add(State(state): State<Arc<HubState>>, caller: Caller, Json(body): Json<NoteBody>) -> Result<(StatusCode, Json<zaklon_core::memory::Note>), ApiError> {
    let note = state.db.add_note(&body.text, &caller.actor()).map_err(invalid)?;
    Ok((StatusCode::CREATED, Json(note)))
}

async fn memory_delete(State(state): State<Arc<HubState>>, caller: Caller, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    if state.db.delete_note(&id, &caller.actor())? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found("no such note"))
    }
}

// ---- Wi-Fi network from the laptop ------------------------------------------------

#[derive(Serialize)]
struct HotspotReply {
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

async fn hotspot_status(_: Local) -> Result<Json<HotspotReply>, ApiError> {
    let s = blocking(crate::hotspot::status).await?;
    Ok(Json(hotspot_reply(s)))
}

/// Laptop only: it changes this computer's network.
async fn hotspot_start(State(state): State<Arc<HubState>>, _: Local) -> Result<Json<HotspotReply>, ApiError> {
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

async fn hotspot_stop(State(state): State<Arc<HubState>>, _: Local) -> Result<Json<HotspotReply>, ApiError> {
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
struct FirewallReply {
    #[serde(flatten)]
    state: crate::firewall::FirewallState,
    ok: bool,
}

async fn firewall_status(_: Local) -> Result<Json<FirewallReply>, ApiError> {
    let state = blocking(crate::firewall::status).await?;
    Ok(Json(FirewallReply { ok: state.ok(), state }))
}

/// Laptop only: changes this computer's firewall (Windows asks for consent).
async fn firewall_allow(State(state): State<Arc<HubState>>, _: Local) -> Result<Json<FirewallReply>, ApiError> {
    tracing::info!("asking Windows to let phones in through the firewall");
    let ports = crate::firewall::Ports::of(&state.config());
    let state = blocking(move || crate::firewall::allow(&ports)).await?;
    Ok(Json(FirewallReply { ok: state.ok(), state }))
}

#[cfg(test)]
mod error_code_tests {
    use super::*;

    const BAD: StatusCode = StatusCode::BAD_REQUEST;
    const FORBIDDEN: StatusCode = StatusCode::FORBIDDEN;
    const NOT_FOUND: StatusCode = StatusCode::NOT_FOUND;

    /// The source of the hub and its core, without the code table and these
    /// tests, so a message counts as sent only if the code really sends it.
    fn hub_sources() -> String {
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut all = String::new();
        for dir in ["zaklon-core/src", "zaklon-hub/src"] {
            for e in std::fs::read_dir(crates.join(dir)).unwrap().flatten() {
                let text = std::fs::read_to_string(e.path()).unwrap();
                match (text.find("const ERROR_CODES"), text.find("mod error_code_tests")) {
                    (Some(table), Some(tests)) => {
                        let after_table = table + text[table..].find("];").unwrap();
                        all.push_str(&text[..table]);
                        all.push_str(&text[after_table..tests]);
                    }
                    _ => all.push_str(&text),
                }
            }
        }
        all
    }

    /// Every code the hub can send: the table's, and those that come from
    /// the status alone. "other" is not one: the app then shows the message.
    fn hub_codes() -> Vec<&'static str> {
        let by_status = [NOT_FOUND, StatusCode::UNAUTHORIZED, FORBIDDEN, StatusCode::TOO_MANY_REQUESTS, StatusCode::INTERNAL_SERVER_ERROR]
            .map(|s| error_code(s, ""));
        let mut codes: Vec<&str> = ERROR_CODES.iter().map(|(_, c)| *c).chain(by_status).collect();
        codes.sort_unstable();
        codes.dedup();
        codes
    }

    #[test]
    fn every_message_gets_its_code() {
        // Messages the hub sends, as written where they are made (the fixed
        // part of a message built with format!), and the code each must get.
        let messages = [
            (BAD, "password must be at least 8 characters", "password_too_short"),
            (BAD, "already set up; use /api/password to change the password", "already_set_up"),
            (BAD, "set a household password first", "not_set_up"),
            (FORBIDDEN, "pairing code is invalid or expired", "code_expired"),
            (FORBIDDEN, "too many attempts; start pairing again on the laptop", "too_many_attempts"),
            (FORBIDDEN, "wrong household password", "wrong_password"),
            (StatusCode::TOO_MANY_REQUESTS, "too many wrong attempts from this device; try again in a few minutes", "device_blocked"),
            (FORBIDDEN, "only the laptop can do this", "laptop_only"),
            (FORBIDDEN, "request from another website", "cross_site"),
            (StatusCode::UNAUTHORIZED, "unauthorized", "unauthorized"),
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error", "internal"),
            (BAD, "name is required", "name_required"),
            (BAD, "that name is reserved", "name_reserved"),
            (BAD, "that folder does not exist", "no_folder"),
            (BAD, "that file does not exist", "no_file"),
            (BAD, "bad barcode", "bad_barcode"),
            (BAD, "delta must be a non-zero number", "bad_quantity"),
            (BAD, "delta must be a number", "bad_quantity"),
            (BAD, "quantity must be more than zero", "bad_quantity"),
            (BAD, "unknown category", "bad_category"),
            (BAD, "expiry must be a date like 2027-03-31", "bad_date"),
            (BAD, "this item has several batches; change the date of a batch instead", "several_batches"),
            (BAD, "text is required", "text_required"),
            (BAD, "the note is too long", "note_too_long"),
            (BAD, "the assistant remembers too much already; delete some notes first", "notes_full"),
            (NOT_FOUND, "no such item", "not_found"),
            (NOT_FOUND, "no such model", "not_found"),
            (NOT_FOUND, "no file", "not_found"),
            (NOT_FOUND, "model is not installed on the hub", "model_not_on_hub"),
            (BAD, "this is not a Zaklon backup", "not_a_backup"),
            (BAD, "the backup is incomplete", "not_a_backup"),
            (BAD, "the backup's database is damaged", "not_a_backup"),
            (BAD, "the backup's settings are damaged", "not_a_backup"),
            (BAD, "this backup was made by a newer Zaklon; update first", "newer_backup"),
            (BAD, "a copy is already running", "copy_running"),
            (BAD, "nothing selected", "nothing_selected"),
            (BAD, "choose a folder outside the library", "outside_library"),
            (BAD, "} is not installed", "not_installed"),
            (BAD, "} cannot be copied", "cannot_copy"),
            (BAD, "not enough space: ", "drive_full"),
            (BAD, "this drive is formatted as FAT32, which cannot hold files of 4 GB or more; format it as exFAT or NTFS", "fat32"),
            (BAD, "writing to the drive: ", "drive_write"),
            (BAD, "pause the download first", "pause_first"),
            (BAD, "could not delete ", "delete_failed"),
            (BAD, "not enough free disk space", "no_disk_space"),
            (BAD, "checksum mismatch", "checksum"),
            (BAD, "no AI model is installed", "no_model"),
            (BAD, "the AI engine is not installed", "no_ai_engine"),
            (BAD, "the AI engine stopped while loading the model", "ai_memory"),
            (BAD, "the AI engine was stopped", "ai_stopped"),
            (BAD, "the assistant is busy with other questions; try again in a moment", "ai_busy"),
            (BAD, "the question is too long", "question_too_long"),
            (BAD, "ask something first", "question_empty"),
        ];
        let sources = hub_sources();
        for (status, msg, code) in messages {
            assert!(sources.contains(msg), "the hub no longer sends {msg:?}; update this list");
            assert_eq!(error_code(status, msg), code, "{msg}");
        }
        assert_eq!(error_code(NOT_FOUND, "no such note"), "not_found");
        assert_eq!(error_code(BAD, "something new"), "other");
    }

    /// An entry whose message the hub no longer sends is dead weight, and a
    /// trap: it can catch some future message by accident.
    #[test]
    fn every_table_entry_matches_a_message_the_hub_sends() {
        let sources = hub_sources();
        let dead: Vec<&str> = ERROR_CODES.iter().map(|(needle, _)| *needle).filter(|n| !sources.contains(n)).collect();
        assert!(dead.is_empty(), "no message contains {dead:?}");
    }

    /// The app translates every code the hub can send (`CODES` in
    /// ui/src/errors.ts); a missing one would show the English message.
    #[test]
    fn the_app_translates_every_code() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/src/errors.ts");
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        let table = &text[text.find("const CODES").expect("errors.ts has a CODES table")..];
        let table = &table[table.find('{').unwrap() + 1..table.find("};").unwrap()];
        let ui: Vec<&str> = table
            .lines()
            .map(|l| l.split("//").next().unwrap_or_default())
            .flat_map(|l| l.split(','))
            .filter_map(|entry| entry.split_once(':'))
            .map(|(key, _)| key.trim().trim_matches(|c| c == '"' || c == '\''))
            .collect();
        let missing: Vec<&str> = hub_codes().into_iter().filter(|c| !ui.contains(c)).collect();
        assert!(missing.is_empty(), "add these codes to CODES in ui/src/errors.ts: {missing:?}");
    }

    #[test]
    fn user_mistakes_are_400_and_our_failures_500() {
        let user = invalid(anyhow::anyhow!("any wording at all"));
        assert_eq!(user.0, StatusCode::BAD_REQUEST);
        let disk = invalid(anyhow::Error::new(std::io::Error::other("disk gone")));
        assert_eq!(disk.0, StatusCode::INTERNAL_SERVER_ERROR);
        let db = invalid(anyhow::Error::new(zaklon_core::rusqlite::Error::InvalidQuery));
        assert_eq!(db.0, StatusCode::INTERNAL_SERVER_ERROR);
    }
}
