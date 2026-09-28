//! Pairing a phone with the hub: by the pairing QR code, or with the
//! 6-digit code on a phone that found the hub with "Find hubs".

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    extract::{ConnectInfo, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use zaklon_core::dates::now_rfc3339;
use zaklon_core::db::Device;
use zaklon_core::pairing;

use super::auth::token_hash;
use super::error::{bad, forbidden, not_found, ApiError};
use super::household::reserved_device_name;
use super::{blocking, Local};
use crate::{HubState, PairingFailure, PairingSession, PakeRun};

const PAIRING_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_PAIRING_ATTEMPTS: u8 = 3;
/// How long a phone may repeat a pairing request after the reply was lost.
const PAIR_REPLAY_WINDOW: Duration = Duration::from_secs(120);
/// How long the hub waits for the phone's proof in a code check from "Find
/// hubs". The phone sends it right after the hub's answer.
const PAKE_RUN_TTL: Duration = Duration::from_secs(60);

/// What the laptop's own listener answers for the phones' pairing requests.
pub(super) async fn network_only() -> ApiError {
    not_found("no such endpoint here; phones pair over the network")
}

// ---- pairing ----------------------------------------------------------------
//
// "Add a phone" on the laptop opens one code at a time, for a few minutes,
// with three attempts. It shows two things:
// - The pairing QR code, with the hub's addresses, its certificate
//   fingerprint and a 128-bit secret. A phone that scans it pins the
//   certificate and pairs with the secret and the household password
//   (`/api/pair/complete`). The secret cannot be guessed.
// - The 6-digit code, typed on a phone that found the hub with "Find hubs".
//   It is accepted only through the code check (SPAKE2, below), where each
//   attempt is one guess at it, and never on its own: otherwise a device on
//   the Wi-Fi could try codes until one is accepted, then answer "Find hubs"
//   in the hub's place with it.
// Every request that fails takes one of the open code's attempts, and one
// address may take only two of the three, so a single device that is not the
// phone being paired cannot use them all up. Each failure also counts against
// its address (see `PairingFailures`); a blocked address is refused before it
// takes an attempt.

/// Most of a code's attempts one address may take.
const MAX_ATTEMPTS_PER_ADDRESS: u8 = 2;
const CODE_EXPIRED: &str = "pairing code is invalid or expired";
const TOO_MANY_ATTEMPTS: &str = "too many attempts; start pairing again on the laptop";

#[derive(Serialize)]
pub(super) struct PairStart {
    code: String,
    expires_in_secs: u64,
    /// What goes into the QR code.
    payload: PairPayload,
}

/// The pairing QR code's content.
#[derive(Serialize)]
struct PairPayload {
    /// 2: the QR code carries `secret`, not the 6-digit code.
    v: u8,
    hosts: Vec<String>,
    port: u16,
    fp: String,
    secret: String,
    name: String,
    install_port: u16,
}

pub(super) async fn pair_start(State(state): State<Arc<HubState>>, _: Local) -> Result<Json<PairStart>, ApiError> {
    if !state.db.is_set_up()? {
        return Err(bad("set a household password first"));
    }
    let code = pairing::pairing_code();
    let secret = pairing::pairing_secret();
    state.pairing_failures.lock().unwrap_or_else(|p| p.into_inner()).reset_total();
    {
        let mut sessions = state.pairing.lock().unwrap_or_else(|p| p.into_inner());
        // Only the newest code is valid: showing a new QR cancels the old one.
        sessions.clear();
        sessions.insert(
            code.clone(),
            PairingSession {
                expires_at: Instant::now() + PAIRING_TTL,
                failed_attempts: 0,
                attempts_by_ip: HashMap::new(),
                secret: secret.clone(),
                runs: Default::default(),
            },
        );
    }
    let cfg = state.config();
    Ok(Json(PairStart {
        code,
        expires_in_secs: PAIRING_TTL.as_secs(),
        payload: PairPayload {
            v: 2,
            hosts: state.lan_addresses().iter().map(|a| a.to_string()).collect(),
            port: cfg.port,
            fp: state.identity.fingerprint.clone(),
            secret,
            name: cfg.hub_name,
            install_port: cfg.install_port,
        },
    }))
}

#[derive(Deserialize)]
pub(super) struct PairComplete {
    /// The secret from the pairing QR code.
    #[serde(default)]
    secret: String,
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

/// Takes one of the code's attempts for `ip`. Returns the reason when there
/// is none to take.
fn take_attempt(sessions: &mut HashMap<String, PairingSession>, code: &str, ip: IpAddr) -> Option<&'static str> {
    let Some(session) = sessions.get_mut(code) else {
        return Some(CODE_EXPIRED);
    };
    if session.failed_attempts >= MAX_PAIRING_ATTEMPTS {
        sessions.remove(code);
        return Some(TOO_MANY_ATTEMPTS);
    }
    let taken = session.attempts_by_ip.entry(ip).or_insert(0);
    if *taken >= MAX_ATTEMPTS_PER_ADDRESS {
        return Some(TOO_MANY_ATTEMPTS);
    }
    *taken += 1;
    session.failed_attempts += 1;
    None
}

/// The open code, if any (only the newest code is open).
fn open_code(sessions: &mut HashMap<String, PairingSession>, now: Instant) -> Option<String> {
    sessions.retain(|_, s| s.expires_at > now);
    sessions.keys().next().cloned()
}

/// For a pairing by QR code: takes one of the open code's attempts and
/// checks the QR code's secret. Returns the open code, or the reason for
/// refusing.
///
/// Every call takes an attempt, whatever it sends. A wrong secret gets the
/// same answer as no open code at all, so a caller without the QR code learns
/// nothing: not whether a code is open, and nothing about the code or the
/// password (which is checked only after the secret, see `check_password`).
fn qr_check(state: &HubState, secret: &str, ip: IpAddr, now: Instant) -> Result<String, &'static str> {
    let mut sessions = state.pairing.lock().unwrap_or_else(|p| p.into_inner());
    let Some(code) = open_code(&mut sessions, now) else {
        return Err(CODE_EXPIRED);
    };
    let right = sessions.get(&code).is_some_and(|s| zaklon_pake::secrets_match(s.secret.as_bytes(), secret.trim().as_bytes()));
    match take_attempt(&mut sessions, &code, ip) {
        None if right => Ok(code),
        Some(reason) if right => Err(reason),
        _ => Err(CODE_EXPIRED),
    }
}

/// Checks the household password for an attempt already taken on `code`.
/// Returns the reason on failure; on success the code is used up. The check
/// (Argon2, slow on purpose) runs on a blocking thread without holding the
/// pairing lock; the attempt was taken before it started, so parallel
/// guesses still count toward the limits.
async fn check_password(state: &Arc<HubState>, code: &str, password: &str) -> Result<Option<&'static str>, ApiError> {
    let hash = state.db.get_setting("household_password_hash")?.unwrap_or_default();
    let password = password.to_string();
    let ok = blocking(move || pairing::verify_password(&password, &hash)).await?;

    let mut sessions = state.pairing.lock().unwrap_or_else(|p| p.into_inner());
    sessions.retain(|_, s| s.expires_at > Instant::now());
    // The code may have been used, replaced, canceled or burned meanwhile.
    let Some(session) = sessions.get(code) else {
        return Ok(Some(CODE_EXPIRED));
    };
    if ok {
        sessions.remove(code);
        Ok(None)
    } else if session.failed_attempts >= MAX_PAIRING_ATTEMPTS {
        sessions.remove(code);
        Ok(Some(TOO_MANY_ATTEMPTS))
    } else {
        Ok(Some("wrong household password"))
    }
}

/// The phone's nonce, when it is one the phone may repeat its request with.
fn pair_nonce(nonce: Option<&str>) -> Option<String> {
    nonce.map(str::trim).filter(|n| n.len() >= 16 && n.len() <= 128).map(str::to_string)
}

/// A repeat of a pairing that already succeeded (the phone never got the
/// reply): the same nonce, and the same `repeat` (see `recent_pairs`).
fn repeated_pair(state: &HubState, nonce: Option<&str>, repeat: &str) -> Option<Paired> {
    let n = nonce?;
    let mut recent = state.recent_pairs.lock().unwrap_or_else(|p| p.into_inner());
    recent.retain(|_, (at, _, _)| at.elapsed() < PAIR_REPLAY_WINDOW);
    recent.get(n).filter(|(_, r, _)| r == repeat).map(|(_, _, paired)| paired.clone())
}

fn remember_pair(state: &HubState, nonce: Option<String>, repeat: String, paired: &Paired) {
    if let Some(n) = nonce {
        state.recent_pairs.lock().unwrap_or_else(|p| p.into_inner()).insert(n, (Instant::now(), repeat, paired.clone()));
    }
}

/// A device that failed too often waits; others can still pair.
async fn refuse_if_blocked(state: &HubState, ip: IpAddr, now: Instant) -> Result<(), ApiError> {
    if state.pairing_failures.lock().unwrap_or_else(|p| p.into_inner()).is_blocked(ip, now) {
        tokio::time::sleep(Duration::from_millis(400)).await;
        return Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "too many wrong attempts from this device; try again in a few minutes".into(),
        ));
    }
    Ok(())
}

/// Counts one failed pairing attempt from `ip`.
fn record_failure(state: &HubState, ip: IpAddr, now: Instant) {
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
}

/// A failed pairing attempt, answered slowly to slow down guessing.
async fn refuse_slowly(msg: &str) -> ApiError {
    tokio::time::sleep(Duration::from_millis(400)).await;
    forbidden(msg)
}

/// The name a phone pairs under: trimmed, at most 60 characters.
fn device_name_from(name: &str) -> String {
    name.trim().chars().take(60).collect()
}

/// Adds a phone that passed every check, and returns what it keeps.
fn add_device(state: &HubState, device_name: &str, platform: String) -> Result<Paired, ApiError> {
    let token = pairing::random_token(32);
    let mut name = device_name_from(device_name);
    if name.is_empty() || reserved_device_name(&name) {
        name = "Phone".to_string();
    }
    let device = Device {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        platform,
        created_at: now_rfc3339(),
        last_seen: Some(now_rfc3339()),
    };
    state.db.insert_device(&device, &token_hash(&token))?;
    let cfg = state.config();
    tracing::info!(device = %device.name, "device paired");
    Ok(Paired {
        device_id: device.id,
        device_token: token,
        hub_id: cfg.hub_id,
        hub_name: cfg.hub_name,
        fingerprint: state.identity.fingerprint.clone(),
    })
}

/// Pairing by QR code: the QR code's secret and the household password.
pub(super) async fn pair_complete(
    State(state): State<Arc<HubState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<PairComplete>,
) -> Result<Json<Paired>, ApiError> {
    let now = Instant::now();
    let ip = peer.ip();
    let nonce = pair_nonce(body.nonce.as_deref());
    let repeat = format!("qr:{}", body.secret.trim());
    if let Some(paired) = repeated_pair(&state, nonce.as_deref(), &repeat) {
        return Ok(Json(paired));
    }
    refuse_if_blocked(&state, ip, now).await?;

    let code = match qr_check(&state, &body.secret, ip, now) {
        Ok(code) => code,
        Err(msg) => {
            record_failure(&state, ip, now);
            return Err(refuse_slowly(msg).await);
        }
    };
    if let Some(msg) = check_password(&state, &code, &body.password).await? {
        record_failure(&state, ip, now);
        return Err(refuse_slowly(msg).await);
    }
    let paired = add_device(&state, &body.device_name, body.platform)?;
    remember_pair(&state, nonce, repeat, &paired);
    Ok(Json(paired))
}

// ---- pairing from "Find hubs" -------------------------------------------------
//
// A hub found on the network is only a suggestion: anyone on the Wi-Fi can
// answer "Find hubs". So before the phone sends the household password, it
// checks with the pairing code that it talks to the hub whose certificate it
// sees (SPAKE2; the protocol is in the zaklon-pake crate). The hub cannot
// tell whether the phone typed the right code, only the phone can, so each
// check is one guess at the code: it takes one of the code's attempts, and it
// counts as a failed attempt from its address until the phone pairs with it.
// A check that is refused (no open code, no attempts left for it, or no name
// for the phone) guesses nothing and takes nothing.

pub(super) async fn pake_start(
    State(state): State<Arc<HubState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<zaklon_pake::StartRequest>,
) -> Result<Json<zaklon_pake::StartReply>, ApiError> {
    let now = Instant::now();
    let ip = peer.ip();
    refuse_if_blocked(&state, ip, now).await?;
    let phone_msg = zaklon_pake::from_hex(&body.msg).ok_or_else(|| bad("bad pairing message"))?;
    // The phone says who it is before it may take an attempt; it pairs
    // under that name, and the log says who checked the code.
    let device_name = device_name_from(&body.device_name);
    if device_name.is_empty() {
        return Err(bad("bad pairing message"));
    }
    let own = zaklon_pake::fingerprint_from_hex(&state.identity.fingerprint)
        .ok_or_else(|| ApiError::from(anyhow::anyhow!("the hub's certificate fingerprint is not 64 hex digits")))?;
    let code = {
        let mut sessions = state.pairing.lock().unwrap_or_else(|p| p.into_inner());
        let Some(code) = open_code(&mut sessions, now) else {
            return Err(forbidden(CODE_EXPIRED));
        };
        if let Some(reason) = take_attempt(&mut sessions, &code, ip) {
            return Err(forbidden(reason));
        }
        code
    };
    record_failure(&state, ip, now);
    tracing::info!(%ip, device = %device_name, "a phone checks the pairing code");
    let answer = zaklon_pake::hub_answer(&code, &phone_msg, &own).map_err(|_| bad("bad pairing message"))?;
    let session = pairing::random_token(16);
    {
        let mut sessions = state.pairing.lock().unwrap_or_else(|p| p.into_inner());
        // Replaced or canceled meanwhile (too many failures cancel every code).
        let Some(open) = sessions.get_mut(&code) else {
            return Err(forbidden(CODE_EXPIRED));
        };
        open.runs.retain(|_, run| now.saturating_duration_since(run.started) < PAKE_RUN_TTL);
        open.runs.insert(session.clone(), PakeRun { started: now, ip, device_name, expect: answer.expect });
    }
    Ok(Json(zaklon_pake::StartReply {
        session,
        msg: zaklon_pake::to_hex(&answer.msg),
        check: zaklon_pake::to_hex(&answer.check),
        proof: zaklon_pake::to_hex(&answer.proof),
    }))
}

pub(super) async fn pake_finish(
    State(state): State<Arc<HubState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<zaklon_pake::FinishRequest>,
) -> Result<Json<Paired>, ApiError> {
    let now = Instant::now();
    let ip = peer.ip();
    let nonce = pair_nonce(body.nonce.as_deref());
    let repeat = format!("run:{}", body.session);
    if let Some(paired) = repeated_pair(&state, nonce.as_deref(), &repeat) {
        return Ok(Json(paired));
    }
    refuse_if_blocked(&state, ip, now).await?;

    // A check is answered once, whatever the answer. Its failure was already
    // counted when it started.
    let run = {
        let mut sessions = state.pairing.lock().unwrap_or_else(|p| p.into_inner());
        sessions.retain(|_, s| s.expires_at > now);
        sessions.iter_mut().find_map(|(code, s)| s.runs.remove(&body.session).map(|run| (code.clone(), run)))
    };
    let Some((code, run)) = run.filter(|(_, run)| now.saturating_duration_since(run.started) < PAKE_RUN_TTL) else {
        return Err(refuse_slowly(CODE_EXPIRED).await);
    };
    // Only a phone with the same key (the same code) can make this proof, and
    // only then is the password looked at: a guess at the code never tells
    // anything about the password.
    if !zaklon_pake::from_hex(&body.proof).is_some_and(|p| zaklon_pake::proof_matches(&run.expect, &p)) {
        return Err(refuse_slowly("wrong pairing code").await);
    }
    if let Some(msg) = check_password(&state, &code, &body.password).await? {
        return Err(refuse_slowly(msg).await);
    }
    state.pairing_failures.lock().unwrap_or_else(|p| p.into_inner()).forgive(run.ip);
    let paired = add_device(&state, &run.device_name, body.platform.unwrap_or_else(default_platform))?;
    remember_pair(&state, nonce, repeat, &paired);
    Ok(Json(paired))
}
