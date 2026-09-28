//! Phone-side client of a Zaklon hub. Keeps the pairing result (hosts, port,
//! certificate fingerprint, device token) in the app's private folder and
//! talks to the hub over TLS pinned to that fingerprint, so the hub's
//! self-signed certificate never has to be trusted by the system.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zaklon_pake::{FinishRequest, Refusal, StartReply, StartRequest};

const LINK_FILE: &str = "hub-link.json";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Before the hub has answered since the app started, and after a try that
/// found no hub (the phone is away from home): how long every address, asked
/// all at once, gets to accept a connection. On the home network the hub
/// answers within milliseconds.
const QUICK_CONNECT: Duration = Duration::from_millis(1500);
/// Ask the network for the hub's new address at most this often.
const REDISCOVER_EVERY: Duration = Duration::from_secs(30);
/// Addresses kept for the hub: the ones that answered most recently.
const MAX_HOSTS: usize = 5;
/// A different Zaklon hub where ours last answered means "our hub was
/// reinstalled" only when it is seen this often, over at least this long,
/// with no answer from our hub in between.
const CHANGED_SEEN: u32 = 3;
const CHANGED_AFTER: Duration = Duration::from_secs(5 * 60);
/// Files the phone keeps its waiting shopping list changes in (see `write_store`).
const STORES: &[(&str, &str)] = &[("outbox", "outbox.json"), ("parked", "outbox-parked.json")];
/// Largest store file accepted from the interface.
const STORE_MAX: usize = 4 * 1024 * 1024;
/// Apps a paired phone can get from the hub: name, where the hub serves it, file.
const APPS: &[(&str, &str, &str)] = &[("comaps", "/api/maps-app", "comaps.apk")];

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
    /// 2: the QR code carries `secret` (version 1 carried the 6-digit code,
    /// which the hub no longer takes on its own).
    pub v: u8,
    pub hosts: Vec<String>,
    pub port: u16,
    /// The hub's certificate fingerprint, pinned from the start.
    pub fp: String,
    /// The QR code's pairing secret, sent with the household password.
    #[serde(default)]
    pub secret: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub install_port: u16,
}

/// The QR code's version this app pairs with.
const QR_VERSION: u8 = 2;

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

/// How the last tries to reach the hub went.
#[derive(Default)]
struct Reach {
    last_ok: Option<Instant>,
    last_fail: Option<Instant>,
    last_discovery: Option<Instant>,
    /// A different Zaklon hub answers where ours last did.
    other_hub: Option<OtherHub>,
}

struct OtherHub {
    hub_id: String,
    first: Instant,
    seen: u32,
}

pub struct ClientState {
    dir: PathBuf,
    /// Apps copied from the hub for installing (the system may clear it).
    apps_dir: PathBuf,
    link: Mutex<Option<HubLink>>,
    /// Base URL of the loopback content proxy, once started.
    content_base: tokio::sync::Mutex<Option<String>>,
    /// One HTTPS client per hub fingerprint, reused so connections stay open.
    http: Mutex<Option<(String, reqwest::Client)>>,
    reach: Mutex<Reach>,
}

impl ClientState {
    pub fn load(dir: PathBuf, cache_dir: PathBuf) -> Self {
        let link = std::fs::read(dir.join(LINK_FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<HubLink>(&bytes).ok());
        Self {
            dir,
            apps_dir: cache_dir.join("apps"),
            link: Mutex::new(link),
            content_base: tokio::sync::Mutex::new(None),
            http: Mutex::new(None),
            reach: Mutex::new(Reach::default()),
        }
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
    /// (forgotten or re-paired while this request was in flight). The list
    /// keeps the addresses that answered (or were found) most recently first,
    /// and only a few of them, so old addresses do not pile up.
    fn note_host(&self, token: &str, host: &str, found: &[String]) {
        let guard = self.link.lock().unwrap_or_else(|p| p.into_inner());
        let Some(current) = guard.as_ref() else { return };
        if current.device_token != token {
            return;
        }
        let mut hosts = vec![host.to_string()];
        for h in found.iter().chain(current.hosts.iter()) {
            if !hosts.contains(h) {
                hosts.push(h.clone());
            }
        }
        hosts.truncate(MAX_HOSTS);
        if current.last_host.as_deref() == Some(host) && hosts == current.hosts {
            return;
        }
        let mut updated = current.clone();
        updated.hosts = hosts;
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
    /// (Anyone can answer with those; the pinned TLS connection decides.)
    async fn rediscover(&self, link: &HubLink) -> Vec<String> {
        let Ok(found) = discover().await else { return vec![] };
        found
            .into_iter()
            .filter(|h| h.id == link.hub_id && h.fp.eq_ignore_ascii_case(&link.fingerprint) && h.port == link.port)
            .map(|h| h.host)
            .collect()
    }

    fn reach(&self) -> std::sync::MutexGuard<'_, Reach> {
        self.reach.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Our hub answered.
    fn reached(&self) {
        let mut r = self.reach();
        r.last_ok = Some(Instant::now());
        r.other_hub = None;
    }

    /// A try found no hub.
    fn missed(&self) {
        self.reach().last_fail = Some(Instant::now());
    }

    /// The hub has not answered since the app started, or the last try found
    /// no hub: the phone may be away from home.
    fn seems_away(&self) -> bool {
        let r = self.reach();
        match (r.last_ok, r.last_fail) {
            (None, _) => true,
            (Some(ok), Some(fail)) => ok < fail,
            (Some(_), None) => false,
        }
    }

    /// Ask the network for the hub again, unless that was done a moment ago.
    fn may_rediscover(&self) -> bool {
        let mut r = self.reach();
        if r.last_discovery.is_some_and(|t| t.elapsed() < REDISCOVER_EVERY) {
            return false;
        }
        r.last_discovery = Some(Instant::now());
        true
    }

    /// Record what answers where our hub last did: `Some(id)` when it is a
    /// Zaklon hub with another identity, `None` when it is not. True once
    /// that has been so consistently enough to say our hub was replaced.
    fn note_other_hub(&self, other: Option<String>, now: Instant) -> bool {
        let mut r = self.reach();
        let Some(id) = other else {
            r.other_hub = None;
            return false;
        };
        let o = r.other_hub.get_or_insert_with(|| OtherHub { hub_id: id.clone(), first: now, seen: 0 });
        if o.hub_id != id {
            *o = OtherHub { hub_id: id, first: now, seen: 0 };
        }
        o.seen += 1;
        o.seen >= CHANGED_SEEN && now.saturating_duration_since(o.first) >= CHANGED_AFTER
    }

    fn link(&self) -> Option<HubLink> {
        self.link.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn store(&self, link: Option<HubLink>) -> Result<(), String> {
        let path = self.dir.join(LINK_FILE);
        match &link {
            Some(l) => {
                let json = serde_json::to_vec_pretty(l).map_err(|e| e.to_string())?;
                write_durably(&path, &json).map_err(|e| e.to_string())?;
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
        *self.reach() = Reach::default();
        self.store(None)
    }

    /// Read one of the phone's stores (waiting shopping list changes).
    pub fn read_store(&self, name: &str) -> Result<Option<String>, String> {
        let file = store_file(name)?;
        match std::fs::read_to_string(self.dir.join(file)) {
            Ok(s) => Ok(Some(s)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("could not read the changes saved on this phone: {e}")),
        }
    }

    /// Save one of the phone's stores so it survives the app being killed a
    /// moment later: written to a new file, flushed to the disk, renamed over
    /// the old one and read back. An error means the change was not saved.
    pub fn write_store(&self, name: &str, data: &str) -> Result<(), String> {
        let file = store_file(name)?;
        if data.len() > STORE_MAX {
            return Err("could not save on this phone: too many changes are waiting".into());
        }
        write_durably(&self.dir.join(file), data.as_bytes()).map_err(|e| format!("could not save on this phone: {e}"))
    }

    /// Complete pairing from the QR code: over a connection pinned to the
    /// certificate it names, send its secret and the household password,
    /// trying every host from the QR code until one answers.
    pub async fn pair(&self, payload: PairPayload, password: String, device_name: String) -> Result<LinkSummary, String> {
        if payload.v != QR_VERSION || payload.secret.trim().is_empty() {
            return Err("unsupported pairing code version".into());
        }
        let client = pinned_client(&payload.fp)?;
        let body = serde_json::json!({
            "secret": payload.secret,
            "password": password,
            "device_name": device_name,
            "platform": std::env::consts::OS,
            "nonce": pair_nonce(),
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
                        return self.keep_link(paired, payload.hosts.clone(), payload.port, payload.fp.clone(), host);
                    }
                    // The hub answered but refused: report its message and stop trying other hosts.
                    return Err(refusal(&text, status));
                }
                Err(e) => last_err = format!("{host}: {}", short_err(&e)),
            }
        }
        Err(last_err)
    }

    /// Pair with a hub found on the network ("Find hubs"). A discovery answer
    /// proves nothing (any device on the Wi-Fi can send one), so the
    /// certificate is not taken from it. The phone connects accepting any
    /// certificate, notes the one that brought the hub's answer, and checks
    /// with the 6-digit code (SPAKE2, see the zaklon-pake crate) that the hub
    /// showing that code holds that certificate. Only then does it send the
    /// household password, over a connection pinned to that certificate.
    pub async fn pair_found(
        &self,
        host: String,
        port: u16,
        code: String,
        password: String,
        device_name: String,
    ) -> Result<LinkSummary, String> {
        let code = code.trim();
        if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
            return Err(Refusal::WrongCode.to_string());
        }
        let base = format!("https://{host}:{port}");

        // 1. Any certificate; the hub proves below that the one we got is its own.
        let phone = zaklon_pake::Phone::start(code);
        let open = pairing_client(Arc::new(SeenVerifier { seen: Arc::default() }))?;
        let res = open
            .post(format!("{base}{}", zaklon_pake::START_PATH))
            .json(&StartRequest { msg: zaklon_pake::to_hex(phone.message()), device_name: device_name.clone() })
            .send()
            .await
            .map_err(|e| format!("{host}: {}", short_err(&e)))?;
        // The certificate of the connection this very answer came on.
        let seen = res
            .extensions()
            .get::<reqwest::tls::TlsInfo>()
            .and_then(|info| info.peer_certificate())
            .map(zaklon_pake::fingerprint_of)
            .ok_or("the hub's certificate could not be read")?;
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(refusal(&text, status));
        }
        let reply: StartReply = serde_json::from_str(&text).map_err(|e| format!("bad reply: {e}"))?;

        // 2. Nothing more is sent unless the hub proved that certificate with the code.
        let proof = answer_hub(phone, &reply, &seen)?;

        // 3. The password goes only to that certificate.
        let fingerprint = zaklon_pake::to_hex(&seen);
        let pinned = pairing_client(Arc::new(PinVerifier { fingerprint: fingerprint.clone() }))?;
        let body = FinishRequest {
            session: reply.session,
            proof,
            password,
            device_name,
            platform: Some(std::env::consts::OS.into()),
            nonce: Some(pair_nonce()),
        };
        let mut last_err = String::new();
        // Twice: a second try with the same nonce recovers a lost reply.
        for _ in 0..2 {
            match pinned.post(format!("{base}{}", zaklon_pake::FINISH_PATH)).json(&body).send().await {
                Ok(res) => {
                    let status = res.status();
                    let text = res.text().await.unwrap_or_default();
                    if !status.is_success() {
                        return Err(refusal(&text, status));
                    }
                    let paired: Paired = serde_json::from_str(&text).map_err(|e| format!("bad reply: {e}"))?;
                    return self.keep_link(paired, vec![host.clone()], port, fingerprint, &host);
                }
                Err(e) => last_err = format!("{host}: {}", short_err(&e)),
            }
        }
        Err(last_err)
    }

    /// Keep what a pairing gave, if the hub names the certificate the phone pinned.
    fn keep_link(&self, paired: Paired, hosts: Vec<String>, port: u16, fingerprint: String, host: &str) -> Result<LinkSummary, String> {
        if paired.fingerprint != fingerprint {
            return Err("hub fingerprint changed during pairing".into());
        }
        let link = HubLink {
            hosts,
            port,
            fingerprint,
            hub_id: paired.hub_id,
            hub_name: paired.hub_name,
            device_id: paired.device_id,
            device_token: paired.device_token,
            last_host: Some(host.to_string()),
        };
        self.store(Some(link))?;
        *self.reach() = Reach::default();
        self.reached();
        Ok(self.summary())
    }

    /// Download from the hub into `dest`, resuming a `.part` file left by an
    /// earlier try, checking the SHA-256 the hub sends. The checksum is
    /// computed while data arrives, so there is no long wait at the end.
    /// `progress(done, total, verifying)`: `verifying` is true while the part
    /// that was already on the phone is read back before resuming.
    pub async fn fetch_to_file(&self, path: &str, dest: &std::path::Path, progress: impl Fn(u64, u64, bool) + Send + Sync + 'static) -> Result<(), String> {
        use futures_util::StreamExt;
        use std::io::Write;

        let link = self.link().ok_or("not paired with a hub")?;
        // No total timeout here (a model is over a gigabyte), only a stall timeout.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let tls = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(PinVerifier { fingerprint: link.fingerprint.clone() }))
            .with_no_client_auth();
        let client = reqwest::Client::builder()
            .use_preconfigured_tls(tls)
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(Duration::from_secs(60))
            .no_proxy()
            .build()
            .map_err(|e| e.to_string())?;

        let part = part_path(dest);
        let progress = Arc::new(progress);
        let mut last_err = String::from("hub not reachable");
        for host in Self::ordered_hosts(&link) {
            let have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
            let url = format!("https://{}:{}{}", host, link.port, path);
            let mut req = client.get(&url).header("authorization", format!("Bearer {}", link.device_token));
            if have > 0 {
                req = req.header("range", format!("bytes={have}-"));
            }
            let mut res = match req.send().await {
                Ok(r) => r,
                Err(e) => {
                    last_err = format!("{host}: {}", short_err(&e));
                    continue;
                }
            };
            // 416: the part on the phone is already as long as (or longer than)
            // the file. A 206 that does not start where we asked cannot be
            // appended either. In both cases drop the part and start over once.
            let bad_resume = res.status() == reqwest::StatusCode::PARTIAL_CONTENT
                && !res
                    .headers()
                    .get("content-range")
                    .and_then(|v| v.to_str().ok())
                    .is_some_and(|v| v.trim().starts_with(&format!("bytes {have}-")));
            if res.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE || bad_resume {
                let _ = std::fs::remove_file(&part);
                res = match client.get(&url).header("authorization", format!("Bearer {}", link.device_token)).send().await {
                    Ok(r) => r,
                    Err(e) => {
                        last_err = format!("{host}: {}", short_err(&e));
                        continue;
                    }
                };
            }
            let have = if res.status() == reqwest::StatusCode::PARTIAL_CONTENT { have } else { 0 };
            let status = res.status();
            if !status.is_success() {
                return Err(format!("hub replied {status}"));
            }
            let sha = res.headers().get("x-zaklon-sha256").and_then(|v| v.to_str().ok()).unwrap_or_default().trim().to_string();
            if sha.is_empty() {
                return Err("the hub did not say how to check the file; update the hub".into());
            }
            let resumed = status == reqwest::StatusCode::PARTIAL_CONTENT;
            let body_len = res.content_length().unwrap_or(0);
            let total = if resumed { have + body_len } else { body_len };
            let mut done = if resumed { have } else { 0 };
            // Hash what is already on the phone once, then keep hashing as data arrives.
            let mut hasher = Sha256::new();
            if resumed {
                let (part2, p2) = (part.clone(), progress.clone());
                hasher = tokio::task::spawn_blocking(move || -> std::io::Result<Sha256> {
                    use std::io::Read;
                    let mut h = Sha256::new();
                    let mut f = std::fs::File::open(&part2)?;
                    let mut buf = vec![0u8; 1 << 20];
                    let mut read = 0u64;
                    let mut last = std::time::Instant::now();
                    loop {
                        let n = f.read(&mut buf)?;
                        if n == 0 {
                            break;
                        }
                        h.update(&buf[..n]);
                        read += n as u64;
                        if last.elapsed() >= Duration::from_millis(300) {
                            p2(read, have, true);
                            last = std::time::Instant::now();
                        }
                    }
                    Ok(h)
                })
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())?;
            }
            let mut file = if resumed {
                std::fs::OpenOptions::new().append(true).open(&part).map_err(|e| e.to_string())?
            } else {
                std::fs::File::create(&part).map_err(|e| e.to_string())?
            };
            let mut stream = res.bytes_stream();
            let mut last_report = std::time::Instant::now();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|e| format!("connection lost: {}", short_err(&e)))?;
                file.write_all(&chunk).map_err(|e| e.to_string())?;
                hasher.update(&chunk);
                done += chunk.len() as u64;
                if last_report.elapsed() >= Duration::from_millis(300) {
                    progress(done, total, false);
                    last_report = std::time::Instant::now();
                }
            }
            file.flush().map_err(|e| e.to_string())?;
            drop(file);
            progress(done, total, false);
            if done < total {
                return Err("connection lost; try again to continue".into());
            }
            let actual: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
            if !actual.eq_ignore_ascii_case(&sha) {
                let _ = std::fs::remove_file(&part);
                return Err("checksum mismatch: the copy was damaged and discarded; try again".into());
            }
            std::fs::rename(&part, dest).map_err(|e| e.to_string())?;
            return Ok(());
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
        let mut hosts = Self::ordered_hosts(&link);
        if self.seems_away() {
            // The phone may be away from home, where these addresses stay
            // silent until each one times out: ask them all at once, briefly,
            // and start with the one that answers first (none: no hub here).
            hosts = answering_first(&hosts, link.port, QUICK_CONNECT).await;
        }
        let mut last_err = String::from("no answer from the hub");
        let mut tried: Vec<String> = Vec::new();
        let mut found: Vec<String> = Vec::new();
        // Another server answered where our hub last did.
        let mut other_at_last = false;
        loop {
            for host in &hosts {
                tried.push(host.clone());
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
                        self.reached();
                        self.note_host(&link.device_token, host, &found);
                        return Ok(ClientResponse { status, body: text });
                    }
                    // Another certificate: our hub is not at this address (now).
                    // Nothing was sent, so go on as if it did not answer.
                    Err(e) if is_pin_mismatch(&e) => {
                        other_at_last |= link.last_host.as_deref() == Some(host.as_str());
                        last_err = format!("{host}: no answer from the hub (another server answers there)");
                    }
                    // Only when the hub never got the request is it safe to try
                    // another address; otherwise a "-1" could be applied twice.
                    Err(e) if e.is_connect() => last_err = format!("{host}: {}", short_err(&e)),
                    Err(e) => {
                        self.missed();
                        return Err(format!("{host}: {}", short_err(&e)));
                    }
                }
            }
            if !found.is_empty() || !self.may_rediscover() {
                break;
            }
            found = self.rediscover(&link).await;
            hosts = found.iter().filter(|h| !tried.contains(h)).cloned().collect();
            if hosts.is_empty() {
                break;
            }
        }
        self.missed();
        // Say "reinstalled" only when a Zaklon hub with another identity keeps
        // answering where ours did; anything else is just "not reachable", and
        // nothing on the phone is thrown away because of it.
        if let (true, Some(host)) = (other_at_last, link.last_host.as_deref()) {
            let other = other_hub_at(host, link.port, &link.hub_id).await;
            if self.note_other_hub(other, Instant::now()) {
                return Err(HUB_CHANGED.into());
            }
        }
        Err(last_err)
    }

    /// Copy an app from the hub (over the pinned connection, checking its
    /// SHA-256) and return the address the phone's browser can download it
    /// from: the loopback proxy, which serves only that checked copy.
    pub async fn fetch_app(self: &Arc<Self>, name: &str) -> Result<String, String> {
        let (_, path, file) = APPS.iter().find(|(n, _, _)| *n == name).ok_or("unknown app")?;
        std::fs::create_dir_all(&self.apps_dir).map_err(|e| e.to_string())?;
        let dest = self.apps_dir.join(file);
        let _ = std::fs::remove_file(&dest);
        self.fetch_to_file(path, &dest, |_, _, _| {}).await?;
        let base = self.content_base().await?;
        Ok(format!("{base}/apk/{file}"))
    }
}

/// `dest` with ".part" added: where a download waits until it is checked.
pub fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".part");
    dest.with_file_name(name)
}

fn store_file(name: &str) -> Result<&'static str, String> {
    STORES.iter().find(|(n, _)| *n == name).map(|(_, f)| *f).ok_or_else(|| "unknown store".to_string())
}

/// Write a file so that it is on the disk when this returns: a temporary
/// file flushed to the disk, renamed over the old one, and read back.
pub fn write_durably(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = path.with_extension("tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    // On Linux (Android) the rename is on the disk only once the folder is.
    #[cfg(unix)]
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    if std::fs::read(path)? != bytes {
        return Err(std::io::Error::other("the saved copy does not match"));
    }
    Ok(())
}

/// Ask every address at once which accepts a connection. The first to accept
/// comes first, followed by the others (tried only if it turns out not to be
/// our hub); no address waits for another. Empty when none answers within
/// `wait` (nothing is sent then, so the phone falls back to its copy quickly).
async fn answering_first(hosts: &[String], port: u16, wait: Duration) -> Vec<String> {
    use futures_util::stream::{FuturesUnordered, StreamExt};
    let mut pending: FuturesUnordered<_> = hosts
        .iter()
        .cloned()
        .map(|h| async move {
            let ok = matches!(tokio::time::timeout(wait, tokio::net::TcpStream::connect((h.as_str(), port))).await, Ok(Ok(_)));
            (h, ok)
        })
        .collect();
    while let Some((host, ok)) = pending.next().await {
        if ok {
            let mut out = vec![host.clone()];
            out.extend(hosts.iter().filter(|h| **h != host).cloned());
            return out;
        }
    }
    Vec::new()
}

/// Is a Zaklon hub with another identity than `hub_id` answering at `host`?
/// Reads only its public status, without our token, over a connection that
/// accepts any certificate but checks that the status names that same one.
async fn other_hub_at(host: &str, port: u16, hub_id: &str) -> Option<String> {
    let seen = Arc::new(Mutex::new(None::<String>));
    let _ = rustls::crypto::ring::default_provider().install_default();
    let tls = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(SeenVerifier { seen: seen.clone() }))
        .with_no_client_auth();
    let client = reqwest::Client::builder()
        .use_preconfigured_tls(tls)
        .connect_timeout(QUICK_CONNECT)
        .timeout(Duration::from_secs(4))
        .no_proxy()
        .build()
        .ok()?;
    let res = client.get(format!("https://{host}:{port}/api/status")).send().await.ok()?;
    if !res.status().is_success() {
        return None;
    }
    let v: serde_json::Value = res.json().await.ok()?;
    let id = v.get("hub_id")?.as_str()?;
    let fp = v.get("fingerprint")?.as_str()?;
    let presented = seen.lock().unwrap_or_else(|p| p.into_inner()).clone()?;
    let hex = |s: &str| s.chars().filter(char::is_ascii_hexdigit).collect::<String>().to_ascii_lowercase();
    (!id.is_empty() && id != hub_id && hex(fp) == presented).then(|| id.to_string())
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

/// Accepts any certificate (the server must still hold its key) and remembers
/// its fingerprint. Used to read the public status of whatever answers at an
/// address (see `other_hub_at`), and for the first step of pairing with a hub
/// found on the network, where the hub then proves the certificate is its
/// own (see `pair_found`).
#[derive(Debug)]
struct SeenVerifier {
    seen: Arc<Mutex<Option<String>>>,
}

impl ServerCertVerifier for SeenVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let actual: String = Sha256::digest(end_entity.as_ref()).iter().map(|b| format!("{b:02x}")).collect();
        *self.seen.lock().unwrap_or_else(|p| p.into_inner()) = Some(actual);
        Ok(ServerCertVerified::assertion())
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

/// A client for one step of pairing with a hub found on the network. It
/// reports the certificate of the connection each answer came on, and
/// follows no redirects: the answer must come from the address that was asked.
fn pairing_client(verifier: Arc<dyn ServerCertVerifier>) -> Result<reqwest::Client, String> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let tls = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    reqwest::Client::builder()
        .use_preconfigured_tls(tls)
        .tls_info(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .no_proxy()
        .build()
        .map_err(|e| e.to_string())
}

/// The hub's answer to the phone's first pairing message, checked against
/// `seen`, the certificate of the connection it came on. Returns the phone's
/// proof for the last step; an error means nothing more may be sent.
fn answer_hub(phone: zaklon_pake::Phone, reply: &StartReply, seen: &zaklon_pake::Fingerprint) -> Result<String, String> {
    let hex = |s: &str| zaklon_pake::from_hex(s).ok_or_else(|| Refusal::BadMessage.to_string());
    let proof = phone
        .check(&hex(&reply.msg)?, &hex(&reply.check)?, &hex(&reply.proof)?, seen)
        .map_err(|r| r.to_string())?;
    Ok(zaklon_pake::to_hex(&proof))
}

/// Random value a pairing request is sent with; repeating the request with
/// it returns the same result (for a reply that got lost).
fn pair_nonce() -> String {
    use rand::RngCore;
    let mut b = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut b);
    zaklon_pake::to_hex(&b)
}

/// The hub's reason for refusing, or its status when it gave none.
fn refusal(text: &str, status: reqwest::StatusCode) -> String {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
        .unwrap_or_else(|| format!("hub replied {status}"))
}

/// Said when a Zaklon hub with a different identity keeps answering where
/// the phone's hub used to (the laptop was reinstalled, or replaced).
pub const HUB_CHANGED: &str = "the hub was reinstalled or replaced; pair this phone again";

/// The server presented another certificate than the pinned one.
fn is_pin_mismatch(e: &reqwest::Error) -> bool {
    let mut source: Option<&dyn std::error::Error> = Some(e);
    while let Some(err) = source {
        if err.to_string().contains("does not match the pairing code") {
            return true;
        }
        source = err.source();
    }
    false
}

fn short_err(e: &reqwest::Error) -> String {
    if is_pin_mismatch(e) {
        "the certificate does not match the hub's fingerprint".into()
    } else if e.is_connect() {
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
        if let Some((_, _, file)) = APPS.iter().find(|(_, _, f)| rest == format!("/apk/{f}")) {
            return self.serve_app(file).await;
        }
        let path_only = rest.split('?').next().unwrap_or(rest).to_ascii_lowercase();
        let escapes = path_only.contains("..") || path_only.contains("%2e") || path_only.contains('\\') || path_only.contains("%5c");
        if !(rest.starts_with("/kiwix/") || rest.starts_with("/kiwix-lat/")) || escapes {
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
            // Security headers from the hub (the sandbox CSP for articles) must
            // reach the web view too.
            let passthrough: Vec<(header::HeaderName, header::HeaderValue)> =
                [header::CONTENT_SECURITY_POLICY, header::X_CONTENT_TYPE_OPTIONS]
                    .into_iter()
                    .filter_map(|name| {
                        let value = res.headers().get(name.as_str())?.to_str().ok()?;
                        Some((name, header::HeaderValue::from_str(value).ok()?))
                    })
                    .collect();
            return match res.bytes().await {
                Ok(body) => {
                    let mut out = (status, [(header::CONTENT_TYPE, ctype), (header::CACHE_CONTROL, cache)], body).into_response();
                    out.headers_mut().extend(passthrough);
                    out
                }
                Err(_) => StatusCode::BAD_GATEWAY.into_response(),
            };
        }
        (StatusCode::BAD_GATEWAY, "hub not reachable").into_response()
    }

    /// An app copied from the hub and checked (see `fetch_app`), for the
    /// phone's browser to download and hand to the system installer.
    async fn serve_app(&self, file: &str) -> axum::response::Response {
        use axum::http::{header, StatusCode};
        use axum::response::IntoResponse;
        use tokio::io::AsyncReadExt;
        let Ok(f) = tokio::fs::File::open(self.apps_dir.join(file)).await else {
            return StatusCode::NOT_FOUND.into_response();
        };
        let len = f.metadata().await.map(|m| m.len()).unwrap_or(0);
        let chunks = futures_util::stream::unfold(Some(f), |state| async move {
            let mut f = state?;
            let mut buf = vec![0u8; 256 * 1024];
            match f.read(&mut buf).await {
                Ok(0) => None,
                Ok(n) => {
                    buf.truncate(n);
                    Some((Ok::<_, std::io::Error>(axum::body::Bytes::from(buf)), Some(f)))
                }
                Err(e) => Some((Err(e), None)),
            }
        });
        (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "application/vnd.android.package-archive".to_string()),
                (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{file}\"")),
                (header::CONTENT_LENGTH, len.to_string()),
                (header::CACHE_CONTROL, "no-store".to_string()),
            ],
            axum::body::Body::from_stream(chunks),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("zaklon-client-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn linked(dir: &Path, hosts: &[&str]) -> ClientState {
        let state = ClientState::load(dir.to_path_buf(), dir.join("cache"));
        state
            .store(Some(HubLink {
                hosts: hosts.iter().map(|h| h.to_string()).collect(),
                port: 8484,
                fingerprint: "ab".into(),
                hub_id: "hub-1".into(),
                hub_name: "Kuća".into(),
                device_id: "d".into(),
                device_token: "t".into(),
                last_host: Some(hosts[0].to_string()),
            }))
            .unwrap();
        state
    }

    #[test]
    fn keeps_the_addresses_that_answered_last_and_only_a_few() {
        let dir = temp("hosts");
        let state = linked(&dir, &["10.0.0.1", "10.0.0.2", "10.0.0.3", "10.0.0.4", "10.0.0.5", "10.0.0.6", "10.0.0.7"]);
        // The hub was found at a new address.
        state.note_host("t", "10.0.0.9", &["10.0.0.9".to_string()]);
        let l = state.link().unwrap();
        assert_eq!(l.last_host.as_deref(), Some("10.0.0.9"));
        assert_eq!(l.hosts, ["10.0.0.9", "10.0.0.1", "10.0.0.2", "10.0.0.3", "10.0.0.4"]);
        // An answer at an older address moves it to the front.
        state.note_host("t", "10.0.0.3", &[]);
        assert_eq!(state.link().unwrap().hosts, ["10.0.0.3", "10.0.0.9", "10.0.0.1", "10.0.0.2", "10.0.0.4"]);
        // A reply for a link that was forgotten meanwhile changes nothing.
        state.note_host("other-token", "10.0.0.8", &[]);
        assert_eq!(state.link().unwrap().last_host.as_deref(), Some("10.0.0.3"));
        // The link survives a restart of the app.
        let again = ClientState::load(dir.clone(), dir.join("cache"));
        assert_eq!(again.link().unwrap().hosts[0], "10.0.0.3");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn another_hub_counts_only_when_seen_consistently() {
        let dir = temp("other");
        let state = linked(&dir, &["10.0.0.1"]);
        let t0 = Instant::now();
        let at = |mins: u64| t0 + Duration::from_secs(mins * 60);
        // One sighting, or three within a minute, is not enough.
        assert!(!state.note_other_hub(Some("hub-2".into()), at(0)));
        assert!(!state.note_other_hub(Some("hub-2".into()), at(1)));
        assert!(!state.note_other_hub(Some("hub-2".into()), at(1)));
        // Seen again after five minutes: our hub was replaced.
        assert!(state.note_other_hub(Some("hub-2".into()), at(6)));
        // Our hub answering again forgets all of it.
        state.reached();
        assert!(!state.note_other_hub(Some("hub-2".into()), at(7)));
        // Something that is not a Zaklon hub also resets it.
        assert!(!state.note_other_hub(Some("hub-2".into()), at(8)));
        assert!(!state.note_other_hub(None, at(9)));
        assert!(!state.note_other_hub(Some("hub-2".into()), at(20)));
        // A different hub each time is not "consistently".
        assert!(!state.note_other_hub(Some("hub-3".into()), at(30)));
        assert!(!state.note_other_hub(Some("hub-2".into()), at(40)));
        assert!(!state.note_other_hub(Some("hub-3".into()), at(50)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_try_makes_the_next_ones_quick() {
        let dir = temp("away");
        let state = linked(&dir, &["10.0.0.1"]);
        // Until the hub first answers, the phone may be anywhere.
        assert!(state.seems_away());
        state.reached();
        assert!(!state.seems_away());
        state.missed();
        assert!(state.seems_away());
        state.reached();
        assert!(!state.seems_away());
        // Rediscovery at most every REDISCOVER_EVERY.
        assert!(state.may_rediscover());
        assert!(!state.may_rediscover());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn answering_finds_the_address_that_accepts() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let started = std::time::Instant::now();
        // 127.0.0.2 refuses (nothing listens there on this port); 127.0.0.1 accepts.
        let hosts = ["127.0.0.2".to_string(), "127.0.0.1".to_string()];
        let found = answering_first(&hosts, port, Duration::from_secs(3)).await;
        assert_eq!(found, ["127.0.0.1", "127.0.0.2"], "the one that answers first, then the rest");
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(listener);
        // Nothing listens: nothing to try, and no long wait.
        let started = std::time::Instant::now();
        assert!(answering_first(&hosts, port, Duration::from_millis(300)).await.is_empty());
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn stores_are_written_durably_and_only_known_ones() {
        let dir = temp("stores");
        let state = ClientState::load(dir.clone(), dir.join("cache"));
        assert_eq!(state.read_store("outbox").unwrap(), None);
        state.write_store("outbox", "[1]").unwrap();
        state.write_store("outbox", "[1,2]").unwrap();
        assert_eq!(state.read_store("outbox").unwrap().as_deref(), Some("[1,2]"));
        assert!(!dir.join("outbox.tmp").exists(), "no temporary file is left");
        assert!(state.write_store("../hub-link", "x").is_err());
        assert!(state.read_store("hub-link").is_err());
        assert!(state.write_store("parked", &"x".repeat(STORE_MAX + 1)).is_err());
        assert_eq!(part_path(Path::new("/m/model.gguf")), Path::new("/m/model.gguf.part"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What a hub (or whatever answers in its place) with `code` and the
    /// certificate `own` replies to the phone's first message.
    fn hub_reply(code: &str, phone: &zaklon_pake::Phone, own: &zaklon_pake::Fingerprint) -> StartReply {
        let a = zaklon_pake::hub_answer(code, phone.message(), own).unwrap();
        let hex = zaklon_pake::to_hex;
        StartReply { session: "run".into(), msg: hex(&a.msg), check: hex(&a.check), proof: hex(&a.proof) }
    }

    #[test]
    fn a_hub_found_on_the_network_must_prove_the_certificate_the_phone_saw() {
        let (hub, relay) = ([1u8; 32], [2u8; 32]);
        // The hub showing the same code, on the connection the phone got: go on.
        let phone = zaklon_pake::Phone::start("123456");
        let reply = hub_reply("123456", &phone, &hub);
        assert_eq!(answer_hub(phone, &reply, &hub).unwrap().len(), 64);
        // A mistyped code.
        let phone = zaklon_pake::Phone::start("123457");
        let reply = hub_reply("123456", &phone, &hub);
        assert_eq!(answer_hub(phone, &reply, &hub).unwrap_err(), "wrong pairing code");
        // The right code, passed on by a device that answered with its own
        // certificate: the hub's proof is for another one.
        let phone = zaklon_pake::Phone::start("123456");
        let reply = hub_reply("123456", &phone, &hub);
        assert!(answer_hub(phone, &reply, &relay).unwrap_err().contains("in place of the hub"));
        // A device that knows neither the code nor the hub's key.
        let phone = zaklon_pake::Phone::start("123456");
        let reply = hub_reply("654321", &phone, &relay);
        assert_eq!(answer_hub(phone, &reply, &relay).unwrap_err(), "wrong pairing code");
        // Not even hex.
        let phone = zaklon_pake::Phone::start("123456");
        let mut reply = hub_reply("123456", &phone, &hub);
        reply.proof = "zz".into();
        assert_eq!(answer_hub(phone, &reply, &hub).unwrap_err(), "bad pairing message");
        // The app has words for both refusals (they arrive as plain text).
        let errors = include_str!("../../../../ui/src/errors.ts");
        assert!(errors.contains("/wrong pairing code/"));
        assert!(errors.contains("/answered in place of the hub/"));
        assert!(Refusal::NotTheHub.to_string().contains("answered in place of the hub"));
    }

    /// The whole "Find hubs" pairing with a real hub on this computer.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pairs_with_a_hub_found_on_the_network() {
        use serde_json::{json, Value};

        let root = temp("found-hub");
        let free = || std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port();
        let (tls, local) = (free(), free());
        let config = json!({ "hub_id": "found-hub", "port": tls, "local_port": local, "install_port": free(), "beacon_port": free(), "auto_update_check": false });
        std::fs::create_dir_all(root.join("household")).unwrap();
        std::fs::write(root.join("household/hub.json"), config.to_string()).unwrap();
        // Listen on 127.0.0.1 only: no DNS-SD, and no firewall question.
        std::env::set_var("ZAKLON_LOOPBACK_ONLY", "1");
        let hub = zaklon_hub::Hub::open(&root).unwrap();
        tokio::spawn(async move {
            let _ = hub.run().await;
        });
        let laptop = reqwest::Client::builder().no_proxy().build().unwrap();
        let base = format!("http://127.0.0.1:{local}");
        let deadline = Instant::now() + Duration::from_secs(20);
        while laptop.get(format!("{base}/api/status")).send().await.is_err() {
            assert!(Instant::now() < deadline, "the hub did not start");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let status: Value = laptop.get(format!("{base}/api/status")).send().await.unwrap().json().await.unwrap();
        let fingerprint = status["fingerprint"].as_str().unwrap().to_string();
        let r = laptop.post(format!("{base}/api/setup")).json(&json!({ "password": "correct horse" })).send().await.unwrap();
        assert_eq!(r.status().as_u16(), 204);
        let pair: Value = laptop.post(format!("{base}/api/pair/start")).send().await.unwrap().json().await.unwrap();
        let code = pair["code"].as_str().unwrap().to_string();
        let wrong = if code == "000000" { "111111" } else { "000000" };

        let dir = temp("found-phone");
        let phone = ClientState::load(dir.clone(), dir.join("cache"));
        async fn try_pair(phone: &ClientState, port: u16, code: &str, password: &str) -> Result<LinkSummary, String> {
            phone.pair_found("127.0.0.1".into(), port, code.into(), password.into(), "Ana's phone".into()).await
        }
        // A wrong code stops before the password is sent.
        assert_eq!(try_pair(&phone, tls, wrong, "correct horse").await.unwrap_err(), "wrong pairing code");
        assert!(!phone.summary().linked);
        // The right code with a wrong password.
        assert_eq!(try_pair(&phone, tls, &code, "not the password").await.unwrap_err(), "wrong household password");
        // One phone may use only two of a code's three attempts.
        assert!(try_pair(&phone, tls, &code, "correct horse").await.unwrap_err().contains("too many attempts"));
        // A new code on the laptop, both right: paired and pinned.
        let pair: Value = laptop.post(format!("{base}/api/pair/start")).send().await.unwrap().json().await.unwrap();
        let code = pair["code"].as_str().unwrap().to_string();
        let link = try_pair(&phone, tls, &code, "correct horse").await.unwrap();
        assert!(link.linked);
        assert_eq!(link.hosts, ["127.0.0.1"]);
        assert_eq!(phone.link().unwrap().fingerprint, fingerprint);
        let me = phone.request("GET".into(), "/api/me".into(), None).await.unwrap();
        assert_eq!(me.status, 200, "{}", me.body);
        assert!(me.body.contains("Ana's phone"));
        // The code is used up.
        assert!(try_pair(&phone, tls, &code, "correct horse").await.unwrap_err().contains("invalid or expired"));

        // Pairing by QR code: the QR code's secret and the household password,
        // over a connection pinned to the certificate the QR code names.
        phone.forget().await.unwrap();
        let pair: Value = laptop.post(format!("{base}/api/pair/start")).send().await.unwrap().json().await.unwrap();
        let mut payload: PairPayload = serde_json::from_value(pair["payload"].clone()).unwrap();
        assert_eq!((payload.v, payload.secret.len(), payload.fp.as_str()), (QR_VERSION, 32, fingerprint.as_str()));
        // This hub listens on 127.0.0.1 only.
        payload.hosts = vec!["127.0.0.1".into()];
        // The QR code of an older hub (the 6-digit code in it) is not taken.
        let older = PairPayload { v: 1, secret: String::new(), ..payload.clone() };
        assert!(phone.pair(older, "correct horse".into(), "Ana's phone".into()).await.unwrap_err().contains("version"));
        // The 6-digit code in place of the secret is refused like no code at all.
        let guessed = PairPayload { secret: pair["code"].as_str().unwrap().into(), ..payload.clone() };
        assert!(phone.pair(guessed, "correct horse".into(), "Ana's phone".into()).await.unwrap_err().contains("invalid or expired"));
        let link = phone.pair(payload, "correct horse".into(), "Ana's phone".into()).await.unwrap();
        assert!(link.linked);
        assert_eq!(phone.link().unwrap().fingerprint, fingerprint);
        let me = phone.request("GET".into(), "/api/me".into(), None).await.unwrap();
        assert_eq!(me.status, 200, "{}", me.body);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
