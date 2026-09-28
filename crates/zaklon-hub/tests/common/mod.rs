//! What the hub's end-to-end tests share: a real hub in a fresh data folder
//! on free ports, and "phones" that talk to it over TLS.

// Each test program uses its own share of these.
#![allow(dead_code)]

use std::net::{IpAddr, TcpListener};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// A port that is free for TCP and UDP. Windows reserves ranges of ports
/// (Hyper-V, WSL) that a plain "port 0" pick can land in for the other
/// protocol, so both are tried before the port is used. Loopback only: the
/// hub under test listens on 127.0.0.1 (ZAKLON_LOOPBACK_ONLY), so nothing
/// here makes Windows ask about its firewall.
pub fn free_port() -> u16 {
    for _ in 0..50 {
        let port = TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port();
        let tcp_ok = TcpListener::bind(("127.0.0.1", port)).is_ok();
        let udp_ok = std::net::UdpSocket::bind(("127.0.0.1", port)).is_ok();
        if tcp_ok && udp_ok {
            return port;
        }
    }
    panic!("no free port found");
}

pub fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("zaklon-e2e-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Accepts the certificate with this fingerprint, or any certificate when
/// there is none. Either way the server must hold the certificate's key.
#[derive(Debug)]
struct Pin(Option<String>);

impl ServerCertVerifier for Pin {
    fn verify_server_cert(
        &self,
        cert: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        match &self.0 {
            Some(fp) if sha256_hex(cert.as_ref()) != *fp => Err(rustls::Error::General("fingerprint mismatch".into())),
            _ => Ok(ServerCertVerified::assertion()),
        }
    }
    fn verify_tls12_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(m, c, d, &rustls::crypto::ring::default_provider().signature_verification_algorithms)
    }
    fn verify_tls13_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(m, c, d, &rustls::crypto::ring::default_provider().signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        rustls::crypto::ring::default_provider().signature_verification_algorithms.supported_schemes()
    }
}

fn tls_client(pin: Pin, from: Option<IpAddr>) -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let tls = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(pin))
        .with_no_client_auth();
    reqwest::Client::builder().use_preconfigured_tls(tls).tls_info(true).no_proxy().local_address(from).build().unwrap()
}

/// A "phone": TLS client that only accepts the hub certificate with this fingerprint.
pub fn phone_client(fingerprint: &str) -> reqwest::Client {
    tls_client(Pin(Some(fingerprint.to_string())), None)
}

/// A phone that found a hub on the network and does not know its
/// certificate yet: it accepts any, and each answer says which one it came
/// with (see `seen_certificate`).
pub fn finder_client() -> reqwest::Client {
    tls_client(Pin(None), None)
}

/// Another device on the network: the hub sees it at `127.0.0.<n>` (every
/// 127.x.x.x address is this computer).
pub fn other_device(n: u8) -> IpAddr {
    IpAddr::from([127, 0, 0, n])
}

/// `finder_client`, connecting from the address `from`.
pub fn finder_client_from(from: IpAddr) -> reqwest::Client {
    tls_client(Pin(None), Some(from))
}

/// `phone_client`, connecting from the address `from`.
pub fn phone_client_from(fingerprint: &str, from: IpAddr) -> reqwest::Client {
    tls_client(Pin(Some(fingerprint.to_string())), Some(from))
}

/// The certificate the answer came with.
pub fn seen_certificate(res: &reqwest::Response) -> [u8; 32] {
    let info = res.extensions().get::<reqwest::tls::TlsInfo>().expect("TLS details of the answer");
    zaklon_pake::fingerprint_of(info.peer_certificate().expect("a certificate"))
}

pub struct Hub {
    pub local: String,
    pub tls: String,
    pub install: String,
    pub beacon_port: u16,
    pub root: PathBuf,
    pub http: reqwest::Client,
}

impl Hub {
    pub async fn get(&self, path: &str) -> (u16, Value) {
        let r = self.http.get(format!("{}{path}", self.local)).send().await.unwrap();
        let st = r.status().as_u16();
        let text = r.text().await.unwrap();
        (st, serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }
    pub async fn send(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> (u16, Value) {
        let mut req = self.http.request(method, format!("{}{path}", self.local));
        if let Some(b) = body {
            req = req.json(&b);
        }
        let r = req.send().await.unwrap();
        let st = r.status().as_u16();
        let text = r.text().await.unwrap();
        (st, serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }
    pub async fn post(&self, path: &str, body: Value) -> (u16, Value) {
        self.send(reqwest::Method::POST, path, Some(body)).await
    }
}

pub async fn wait_for(url: &str) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if reqwest::get(url).await.is_ok() {
            return;
        }
        assert!(Instant::now() < deadline, "hub did not start: {url}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Start a real hub on the data folder `root`, on free loopback ports, and
/// wait until it answers.
pub async fn run_hub(root: PathBuf) -> Hub {
    let (tls, local, install, beacon) = (free_port(), free_port(), free_port(), free_port());
    std::env::set_var("ZAKLON_TLS_PORT", tls.to_string());
    std::env::set_var("ZAKLON_LOCAL_PORT", local.to_string());
    std::env::set_var("ZAKLON_INSTALL_PORT", install.to_string());
    std::env::set_var("ZAKLON_BEACON_PORT", beacon.to_string());
    std::env::set_var("ZAKLON_IGNORE_BATTERY", "1");
    std::env::set_var("ZAKLON_LOOPBACK_ONLY", "1");

    let hub = zaklon_hub::Hub::open(&root).unwrap();
    tokio::spawn(async move { hub.run().await.unwrap() });
    let h = Hub {
        local: format!("http://127.0.0.1:{local}"),
        tls: format!("https://127.0.0.1:{tls}"),
        install: format!("http://127.0.0.1:{install}"),
        beacon_port: beacon,
        root,
        http: reqwest::Client::builder().no_proxy().build().unwrap(),
    };
    wait_for(&format!("{}/api/status", h.local)).await;
    h
}
