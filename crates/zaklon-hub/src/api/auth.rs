//! Who is asking: the laptop itself, a paired device with its token, or
//! anyone allowed to read library pages.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::{ConnectInfo, FromRequestParts, OptionalFromRequestParts},
    http::{request::Parts, StatusCode},
};
use sha2::{Digest, Sha256};
use zaklon_core::dates::now_rfc3339;
use zaklon_core::db::Device;

use super::error::{forbidden, unauthorized, ApiError};
use super::Listener;
use crate::HubState;

pub enum Caller {
    Local,
    Device(Device),
}

impl Caller {
    /// Who did it, as shown in the history.
    pub(super) fn actor(&self) -> String {
        match self {
            Caller::Local => "laptop".into(),
            Caller::Device(d) => d.name.clone(),
        }
    }

    /// Whose saved conversations these are: the laptop's, or this phone's.
    pub(super) fn owner(&self) -> String {
        match self {
            Caller::Local => zaklon_core::conversations::LAPTOP.into(),
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

pub(super) fn token_hash(token: &str) -> String {
    let d = Sha256::digest(token.as_bytes());
    d.iter().map(|b| format!("{b:02x}")).collect()
}
