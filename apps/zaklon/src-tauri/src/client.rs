//! Phone-side client of a Zaklon hub. Keeps the pairing result (hosts, port,
//! certificate fingerprint, device token) in the app's private folder and
//! talks to the hub over TLS pinned to that fingerprint, so the hub's
//! self-signed certificate never has to be trusted by the system.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const LINK_FILE: &str = "hub-link.json";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Everything needed to reach a hub again.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubLink {
    pub hosts: Vec<String>,
    pub port: u16,
    pub fingerprint: String,
    pub hub_id: String,
    pub hub_name: String,
    pub device_id: String,
    pub device_token: String,
    /// Host that answered most recently; tried first next time.
    pub last_host: Option<String>,
}

/// What the interface needs to know about the link, without the token.
#[derive(Debug, Clone, Serialize)]
pub struct LinkSummary {
    pub linked: bool,
    pub hub_id: Option<String>,
    pub hub_name: Option<String>,
    pub device_id: Option<String>,
    pub hosts: Vec<String>,
    pub port: Option<u16>,
    pub last_host: Option<String>,
}

/// The pairing QR payload as produced by the hub (`/api/pair/start`).
/// `name` and `install_port` are shown by the interface, not used here.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct PairPayload {
    pub v: u8,
    pub hosts: Vec<String>,
    pub port: u16,
    pub fp: String,
    pub code: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub install_port: u16,
}

#[derive(Debug, Deserialize)]
struct Paired {
    device_id: String,
    device_token: String,
    hub_id: String,
    hub_name: String,
    fingerprint: String,
}

#[derive(Debug, Serialize)]
pub struct ClientResponse {
    pub status: u16,
    pub body: String,
}

#[derive(Debug, Serialize)]
pub struct DiscoveredHub {
    pub host: String,
    pub port: u16,
    pub fp: String,
    pub id: String,
    pub name: String,
}

pub struct ClientState {
    dir: PathBuf,
    link: Mutex<Option<HubLink>>,
    /// Base URL of the loopback content proxy, once started.
    content_base: tokio::sync::Mutex<Option<String>>,
    /// One HTTPS client per hub fingerprint, reused so connections stay open.
    http: Mutex<Option<(String, reqwest::Client)>>,
}

impl ClientState {
    pub fn load(dir: PathBuf) -> Self {
        let link = std::fs::read(dir.join(LINK_FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<HubLink>(&bytes).ok());
        Self { dir, link: Mutex::new(link), content_base: tokio::sync::Mutex::new(None), http: Mutex::new(None) }
    }

    fn client_for(&self, fingerprint: &str) -> Result<reqwest::Client, String> {
        let mut cache = self.http.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((fp, c)) = cache.as_ref() {
            if fp == fingerprint {
                return Ok(c.clone());
            }
        }
        let c = pinned_client(fingerprint)?;
        *cache = Some((fingerprint.to_string(), c.clone()));
        Ok(c)
    }

    /// Remember a host that answered, unless the link changed meanwhile
    /// (forgotten or re-paired while this request was in flight).
    fn note_host(&self, token: &str, host: &str, hosts: Option<Vec<String>>) {
        let guard = self.link.lock().unwrap_or_else(|p| p.into_inner());
        let Some(current) = guard.as_ref() else { return };
        if current.device_token != token {
            return;
        }
        let mut updated = current.clone();
        if let Some(h) = hosts {
            updated.hosts = h;
        }
        if updated.last_host.as_deref() == Some(host) && updated.hosts == current.hosts {
            return;
        }
        updated.last_host = Some(host.to_string());
        drop(guard);
        let _ = self.store(Some(updated));
    }

    /// Hosts to try: the last one that answered first.
    fn ordered_hosts(link: &HubLink) -> Vec<String> {
        let mut hosts = link.hosts.clone();
        if let Some(last) = &link.last_host {
            hosts.retain(|h| h != last);
            hosts.insert(0, last.clone());
        }
        hosts
    }

    /// The hub may have a new address (router gave the laptop another IP).
    /// Ask the network; accept only the hub with our id and certificate.
    async fn rediscover(&self, link: &HubLink) -> Option<Vec<String>> {
        let found = discover().await.ok()?;
        let fresh: Vec<String> = found
            .into_iter()
            .filter(|h| h.id == link.hub_id && h.fp.eq_ignore_ascii_case(&link.fingerprint) && h.port == link.port)
            .map(|h| h.host)
            .collect();
        if fresh.is_empty() {
            return None;
        }
        let mut hosts = fresh;
        for h in &link.hosts {
            if !hosts.contains(h) {
                hosts.push(h.clone());
            }
        }
        Some(hosts)
    }

    fn link(&self) -> Option<HubLink> {
        self.link.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn store(&self, link: Option<HubLink>) -> Result<(), String> {
        let path = self.dir.join(LINK_FILE);
        match &link {
            Some(l) => {
                std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
                let json = serde_json::to_vec_pretty(l).map_err(|e| e.to_string())?;
                let tmp = path.with_extension("tmp");
                std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
                std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
            }
            None => {
                let _ = std::fs::remove_file(&path);
            }
        }
        *self.link.lock().unwrap_or_else(|p| p.into_inner()) = link;
        Ok(())
    }

    pub fn summary(&self) -> LinkSummary {
        match self.link() {
            Some(l) => LinkSummary {
                linked: true,
                hub_id: Some(l.hub_id),
                hub_name: Some(l.hub_name),
                device_id: Some(l.device_id),
                hosts: l.hosts,
                port: Some(l.port),
                last_host: l.last_host,
            },
            None => LinkSummary {
                linked: false,
                hub_id: None,
                hub_name: None,
                device_id: None,
                hosts: vec![],
                port: None,
                last_host: None,
            },
        }
    }

    /// Unlink this phone. The hub is asked to revoke the token first (best
    /// effort: works only if it is reachable), then the local link is removed.
    pub async fn forget(&self) -> Result<(), String> {
        if let Some(link) = self.link() {
            let path = format!("/api/devices/{}", link.device_id);
            let _ = tokio::time::timeout(Duration::from_secs(5), self.request("DELETE".into(), path, None)).await;
        }
        *self.http.lock().unwrap_or_else(|p| p.into_inner()) = None;
        self.store(None)
    }

    /// Complete pairing: try every host from the QR until one answers.
    pub async fn pair(&self, payload: PairPayload, password: String, device_name: String) -> Result<LinkSummary, String> {
        if payload.v != 1 {
            return Err("unsupported pairing code version".into());
        }
        let client = pinned_client(&payload.fp)?;
        let nonce: String = {
            use rand::RngCore;
            let mut b = [0u8; 16];
            rand::thread_rng().fill_bytes(&mut b);
            b.iter().map(|x| format!("{x:02x}")).collect()
        };
        let body = serde_json::json!({
            "code": payload.code,
            "password": password,
            "device_name": device_name,
            "platform": std::env::consts::OS,
            "nonce": nonce,
        });
        let mut last_err = String::from("no hosts in pairing code");
        // Each host twice: a second try with the same nonce recovers a lost reply.
        let attempts: Vec<&String> = payload.hosts.iter().flat_map(|h| [h, h]).collect();
        for host in attempts {
            let url = format!("https://{}:{}/api/pair/complete", host, payload.port);
            match client.post(&url).json(&body).send().await {
                Ok(res) => {
                    let status = res.status();
                    let text = res.text().await.unwrap_or_default();
                    if status.is_success() {
                        let paired: Paired = serde_json::from_str(&text).map_err(|e| format!("bad reply: {e}"))?;
                        if paired.fingerprint != payload.fp {
                            return Err("hub fingerprint changed during pairing".into());
                        }
                        let link = HubLink {
                            hosts: payload.hosts.clone(),
                            port: payload.port,
                            fingerprint: payload.fp.clone(),
                            hub_id: paired.hub_id,
                            hub_name: paired.hub_name,
                            device_id: paired.device_id,
                            device_token: paired.device_token,
                            last_host: Some(host.clone()),
                        };
                        self.store(Some(link))?;
                        return Ok(self.summary());
                    }
                    // The hub answered but refused: report its message and stop trying other hosts.
                    let msg = serde_json::from_str::<serde_json::Value>(&text)
                        .ok()
                        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
                        .unwrap_or_else(|| format!("hub replied {status}"));
                    return Err(msg);
                }
                Err(e) => last_err = format!("{host}: {}", short_err(&e)),
            }
        }
        Err(last_err)
    }

    /// Send an authenticated request to the hub, trying the last good host first.
    pub async fn request(&self, method: String, path: String, body: Option<String>) -> Result<ClientResponse, String> {
        let Some(link) = self.link() else {
            return Err("not paired with a hub".into());
        };
        let client = self.client_for(&link.fingerprint)?;
        let method = reqwest::Method::from_bytes(method.as_bytes()).map_err(|e| e.to_string())?;
        let mut last_err = String::from("hub has no known addresses");
        let mut rediscovered = false;
        let mut hosts = Self::ordered_hosts(&link);
        loop {
            for host in &hosts {
                let url = format!("https://{}:{}{}", host, link.port, path);
                let mut req = client
                    .request(method.clone(), &url)
                    .header("authorization", format!("Bearer {}", link.device_token));
                if let Some(b) = &body {
                    req = req.header("content-type", "application/json").body(b.clone());
                }
                match req.send().await {
                    Ok(res) => {
                        let status = res.status().as_u16();
                        let text = res.text().await.unwrap_or_default();
                        let new_hosts = rediscovered.then(|| hosts.clone());
                        self.note_host(&link.device_token, host, new_hosts);
                        return Ok(ClientResponse { status, body: text });
                    }
                    // Only when the hub never got the request is it safe to try
                    // another address; otherwise a "-1" could be applied twice.
                    Err(e) if e.is_connect() => last_err = format!("{host}: {}", short_err(&e)),
                    Err(e) => return Err(format!("{host}: {}", short_err(&e))),
                }
            }
            if rediscovered {
                return Err(last_err);
            }
            rediscovered = true;
            match self.rediscover(&link).await {
                Some(h) => hosts = h,
                None => return Err(last_err),
            }
        }
    }
}

/// Ask the local network for hubs (UDP beacon) and collect replies for about a second.
pub async fn discover() -> Result<Vec<DiscoveredHub>, String> {
    use tokio::net::UdpSocket;
    let socket = UdpSocket::bind(("0.0.0.0", 0)).await.map_err(|e| e.to_string())?;
    socket.set_broadcast(true).map_err(|e| e.to_string())?;
    // Padded: the hub only answers requests at least as long as its reply.
    let mut request = b"ZAKLON?".to_vec();
    request.resize(512, 0);
    let _ = socket.send_to(&request, ("255.255.255.255", 8485)).await;
    let mut found: Vec<DiscoveredHub> = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_millis(1500);
    let mut buf = [0u8; 1024];
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, socket.recv_from(&mut buf)).await {
            Ok(Ok((n, peer))) => {
                if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&buf[..n]) {
                    let get = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string();
                    let port = v.get("port").and_then(|x| x.as_u64()).unwrap_or(8484) as u16;
                    let host = peer.ip().to_string();
                    if !found.iter().any(|f| f.host == host) {
                        found.push(DiscoveredHub { host, port, fp: get("fp"), id: get("id"), name: get("name") });
                    }
                }
            }
            _ => break,
        }
    }
    Ok(found)
}

// ---- pinned TLS -------------------------------------------------------------

#[derive(Debug)]
struct PinVerifier {
    fingerprint: String,
}

impl ServerCertVerifier for PinVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let actual: String = Sha256::digest(end_entity.as_ref()).iter().map(|b| format!("{b:02x}")).collect();
        if actual.eq_ignore_ascii_case(&self.fingerprint) {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General("hub certificate does not match the pairing code".into()))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &rustls::crypto::ring::default_provider().signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &rustls::crypto::ring::default_provider().signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        rustls::crypto::ring::default_provider().signature_verification_algorithms.supported_schemes()
    }
}

fn pinned_client(fingerprint: &str) -> Result<reqwest::Client, String> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let tls = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinVerifier { fingerprint: fingerprint.to_string() }))
        .with_no_client_auth();
    reqwest::Client::builder()
        .use_preconfigured_tls(tls)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .pool_idle_timeout(Duration::from_secs(30))
        .no_proxy()
        .build()
        .map_err(|e| e.to_string())
}

fn short_err(e: &reqwest::Error) -> String {
    if e.is_connect() {
        "no answer".into()
    } else if e.is_timeout() {
        "timed out".into()
    } else {
        let s = e.to_string();
        s.split(": ").last().unwrap_or(&s).to_string()
    }
}

// ---- loopback content proxy -------------------------------------------------
//
// Library articles are HTML pages with images and styles, shown in a frame.
// The frame cannot add the device token or pin the hub certificate, so the app
// runs a tiny read-only proxy on 127.0.0.1 that does both. It only forwards
// GET /<secret>/kiwix/... ; the random secret keeps other apps on the phone
// from using it.

impl ClientState {
    /// Start the proxy on first use and return its base URL.
    pub async fn content_base(self: &Arc<Self>) -> Result<String, String> {
        let mut base = self.content_base.lock().await;
        if let Some(b) = base.as_ref() {
            return Ok(b.clone());
        }
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let secret: String = {
            use rand::RngCore;
            let mut b = [0u8; 16];
            rand::thread_rng().fill_bytes(&mut b);
            b.iter().map(|x| format!("{x:02x}")).collect()
        };
        let me = self.clone();
        let prefix = format!("/{secret}");
        let app = axum::Router::new().fallback(move |uri: axum::http::Uri| {
            let me = me.clone();
            let prefix = prefix.clone();
            async move { me.proxy(uri, &prefix).await }
        });
        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                tracing::warn!("content proxy stopped: {e}");
            }
        });
        let b = format!("http://127.0.0.1:{port}/{secret}");
        *base = Some(b.clone());
        Ok(b)
    }

    async fn proxy(&self, uri: axum::http::Uri, prefix: &str) -> axum::response::Response {
        use axum::http::{header, StatusCode};
        use axum::response::IntoResponse;
        let full = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
        let Some(rest) = full.strip_prefix(prefix) else { return StatusCode::NOT_FOUND.into_response() };
        let path_only = rest.split('?').next().unwrap_or(rest).to_ascii_lowercase();
        let escapes = path_only.contains("..") || path_only.contains("%2e") || path_only.contains('\\') || path_only.contains("%5c");
        if !rest.starts_with("/kiwix/") || escapes {
            return StatusCode::NOT_FOUND.into_response();
        }
        let Some(link) = self.link() else {
            return (StatusCode::SERVICE_UNAVAILABLE, "not paired").into_response();
        };
        let Ok(client) = self.client_for(&link.fingerprint) else { return StatusCode::BAD_GATEWAY.into_response() };
        let hosts = Self::ordered_hosts(&link);
        for host in hosts {
            let url = format!("https://{}:{}{}", host, link.port, rest);
            let sent = client
                .get(&url)
                .header("authorization", format!("Bearer {}", link.device_token))
                .send()
                .await;
            let Ok(res) = sent else { continue };
            let status = StatusCode::from_u16(res.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let header_str = |name: &str, dflt: &str| {
                res.headers().get(name).and_then(|v| v.to_str().ok()).unwrap_or(dflt).to_string()
            };
            let ctype = header_str("content-type", "application/octet-stream");
            let cache = header_str("cache-control", "no-cache");
            return match res.bytes().await {
                Ok(body) => (status, [(header::CONTENT_TYPE, ctype), (header::CACHE_CONTROL, cache)], body).into_response(),
                Err(_) => StatusCode::BAD_GATEWAY.into_response(),
            };
        }
        (StatusCode::BAD_GATEWAY, "hub not reachable").into_response()
    }
}
