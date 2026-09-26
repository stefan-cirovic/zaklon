//! End-to-end test of a real hub: it opens a fresh data folder, listens on
//! free ports, and is driven over HTTP exactly like the desktop window
//! (loopback) and a paired phone (TLS pinned to the hub's certificate) would.
//!
//! Run with: `cargo test -p zaklon-hub --test e2e -- --nocapture`

use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

// ---- helpers ------------------------------------------------------------------

fn free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port()
}

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("zaklon-e2e-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug)]
struct Pin(String);

impl ServerCertVerifier for Pin {
    fn verify_server_cert(
        &self,
        cert: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if sha256_hex(cert.as_ref()) == self.0 {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General("fingerprint mismatch".into()))
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

/// A "phone": TLS client that only accepts the hub certificate with this fingerprint.
fn phone_client(fingerprint: &str) -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let tls = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Pin(fingerprint.to_string())))
        .with_no_client_auth();
    reqwest::Client::builder().use_preconfigured_tls(tls).no_proxy().build().unwrap()
}

struct Hub {
    local: String,
    tls: String,
    install: String,
    beacon_port: u16,
    root: PathBuf,
    http: reqwest::Client,
}

impl Hub {
    async fn get(&self, path: &str) -> (u16, Value) {
        let r = self.http.get(format!("{}{path}", self.local)).send().await.unwrap();
        let st = r.status().as_u16();
        let text = r.text().await.unwrap();
        (st, serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }
    async fn send(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> (u16, Value) {
        let mut req = self.http.request(method, format!("{}{path}", self.local));
        if let Some(b) = body {
            req = req.json(&b);
        }
        let r = req.send().await.unwrap();
        let st = r.status().as_u16();
        let text = r.text().await.unwrap();
        (st, serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }
    async fn post(&self, path: &str, body: Value) -> (u16, Value) {
        self.send(reqwest::Method::POST, path, Some(body)).await
    }
}

async fn wait_for(url: &str) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if reqwest::get(url).await.is_ok() {
            return;
        }
        assert!(Instant::now() < deadline, "hub did not start: {url}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Serve `dir` over HTTP (with Range support) on a free port; returns the base URL.
async fn file_server(dir: &Path) -> String {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = axum::Router::new().fallback_service(tower_http::services::ServeDir::new(dir));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

async fn wait_pack(hub: &Hub, id: &str, until: &[&str]) -> Value {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let (_, cat) = hub.get("/api/catalog").await;
        let pack = cat["packs"].as_array().unwrap().iter().find(|p| p["id"] == id).unwrap().clone();
        let status = pack["state"]["status"].as_str().unwrap().to_string();
        if until.contains(&status.as_str()) {
            return pack;
        }
        assert!(Instant::now() < deadline, "pack {id} stuck in {status}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn start_hub() -> Hub {
    let root = temp_dir("hub");
    let (tls, local, install, beacon) = (free_port(), free_port(), free_port(), free_port());
    std::env::set_var("ZAKLON_TLS_PORT", tls.to_string());
    std::env::set_var("ZAKLON_LOCAL_PORT", local.to_string());
    std::env::set_var("ZAKLON_INSTALL_PORT", install.to_string());
    std::env::set_var("ZAKLON_BEACON_PORT", beacon.to_string());
    std::env::set_var("ZAKLON_IGNORE_BATTERY", "1");

    // A small test catalog served by a local file server.
    let files = temp_dir("files");
    let payload: Vec<u8> = (0..3_000_000u32).map(|i| (i.wrapping_mul(2654435761) >> 24) as u8).collect();
    std::fs::write(files.join("tiny_test_2026-01.zim"), &payload).unwrap();
    let server = file_server(&files).await;
    let catalog = json!({
        "version": 1,
        "generated": "2999-01-01",
        "packs": [{
            "id": "test-pack",
            "title": { "en": "Test pack", "sr": "Test paket" },
            "category": "knowledge",
            "version": "2026-01",
            "size": payload.len(),
            "files": [{
                "path": "zim/tiny_test_2026-01.zim",
                "urls": [format!("{server}/tiny_test_2026-01.zim")],
                "sha256": sha256_hex(&payload),
                "size": payload.len()
            }]
        }, {
            "id": "broken-pack",
            "title": { "en": "Broken pack" },
            "category": "knowledge",
            "version": "1",
            "size": payload.len(),
            "files": [{
                "path": "zim/broken.zim",
                "urls": [format!("{server}/tiny_test_2026-01.zim")],
                "sha256": "0".repeat(64),
                "size": payload.len()
            }]
        }]
    });
    std::fs::create_dir_all(root.join("catalog")).unwrap();
    std::fs::write(root.join("catalog/catalog.json"), catalog.to_string()).unwrap();

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

// ---- the test -------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn full_hub_flow() {
    let hub = start_hub().await;

    // 1. Fresh hub: not set up; the laptop sees details.
    let (st, status) = hub.get("/api/status").await;
    assert_eq!(st, 200);
    assert_eq!(status["set_up"], false);
    assert!(status["devices"].is_number(), "laptop sees details");
    let fingerprint = status["fingerprint"].as_str().unwrap().to_string();
    assert_eq!(fingerprint.len(), 64);

    // 2. A phone that does not know the fingerprint yet can read only the public status.
    let phone = phone_client(&fingerprint);
    let (st, public) = {
        let r = phone.get(format!("{}/api/status", hub.tls)).send().await.unwrap();
        (r.status().as_u16(), r.json::<Value>().await.unwrap())
    };
    assert_eq!(st, 200);
    assert!(public.get("devices").is_none(), "no details for strangers");
    assert!(public.get("root").is_none());

    // 3. A client pinned to a different fingerprint must refuse the hub.
    let impostor = phone_client(&"ab".repeat(32));
    assert!(impostor.get(format!("{}/api/status", hub.tls)).send().await.is_err(), "pinning rejects other certs");

    // 4. Setup: short password refused, then accepted, then not repeatable.
    let (st, _) = hub.post("/api/setup", json!({ "password": "short" })).await;
    assert_eq!(st, 400);
    let (st, _) = hub.post("/api/setup", json!({ "password": "correct horse", "hub_name": "E2E hub", "language": "sr" })).await;
    assert_eq!(st, 204);
    let (st, _) = hub.post("/api/setup", json!({ "password": "another one" })).await;
    assert_eq!(st, 400);
    assert_eq!(hub.get("/api/status").await.1["hub_name"], "E2E hub");

    // 5. Other websites in a browser on the laptop cannot drive the hub.
    let evil = hub
        .http
        .post(format!("{}/api/pair/start", hub.local))
        .header("origin", "http://evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(evil.status().as_u16(), 403, "cross-site request refused");
    let rebind = hub
        .http
        .get(format!("{}/api/devices", hub.local))
        .header("host", "evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(rebind.status().as_u16(), 403, "DNS rebinding refused");

    // 6. Nothing private is reachable over the network without a token.
    let r = phone.get(format!("{}/api/devices", hub.tls)).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);
    let r = phone.get(format!("{}/api/devices", hub.tls)).bearer_auth("not-a-token").send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);

    // 7. Pairing: wrong password counts attempts; the third wrong one burns the code.
    let (_, pair) = hub.post("/api/pair/start", json!({})).await;
    let code = pair["code"].as_str().unwrap().to_string();
    assert_eq!(pair["payload"]["fp"], fingerprint.as_str());
    for i in 0..3 {
        let r = phone
            .post(format!("{}/api/pair/complete", hub.tls))
            .json(&json!({ "code": code, "password": "wrong password", "device_name": "x" }))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status().as_u16(), 403, "attempt {i}");
    }
    let r = phone
        .post(format!("{}/api/pair/complete", hub.tls))
        .json(&json!({ "code": code, "password": "correct horse", "device_name": "x" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 403, "burned code stays invalid");

    // 8. Pairing with a fresh code and the right password. Starting a new code
    //    cancels the previous one.
    let (_, old) = hub.post("/api/pair/start", json!({})).await;
    let (_, pair) = hub.post("/api/pair/start", json!({})).await;
    let r = phone
        .post(format!("{}/api/pair/complete", hub.tls))
        .json(&json!({ "code": old["code"], "password": "correct horse", "device_name": "x" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 403, "an older code is no longer valid");
    let request = json!({ "code": pair["code"], "password": "correct horse", "device_name": "Ana's phone", "nonce": "0123456789abcdef-e2e" });
    let r = phone.post(format!("{}/api/pair/complete", hub.tls)).json(&request).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let paired: Value = r.json().await.unwrap();
    // The reply "got lost": the phone repeats the request and gets the same device, not a second one.
    let again: Value = phone.post(format!("{}/api/pair/complete", hub.tls)).json(&request).send().await.unwrap().json().await.unwrap();
    assert_eq!(again["device_id"], paired["device_id"]);
    let token = paired["device_token"].as_str().unwrap().to_string();
    assert_eq!(paired["fingerprint"], fingerprint.as_str());

    let as_phone = |method: reqwest::Method, path: &str| {
        phone.request(method, format!("{}{path}", hub.tls)).bearer_auth(&token)
    };

    let devices: Value = as_phone(reqwest::Method::GET, "/api/devices").send().await.unwrap().json().await.unwrap();
    assert_eq!(devices.as_array().unwrap().len(), 1);
    assert_eq!(devices[0]["name"], "Ana's phone");

    // 9. A phone cannot do laptop-only things.
    let r = as_phone(reqwest::Method::POST, "/api/pair/start").send().await.unwrap();
    assert_eq!(r.status().as_u16(), 403);
    let r = as_phone(reqwest::Method::POST, "/api/password").json(&json!({ "new_password": "hijacked!!" })).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 403);

    // 10. Supplies from the phone, with history naming the phone.
    let r = as_phone(reqwest::Method::POST, "/api/items")
        .json(&json!({ "name": "Brašno", "quantity": 2, "unit": "kg", "category": "food", "place": "pantry", "min_quantity": 3, "barcode": "8600000000017" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 201);
    let item: Value = r.json().await.unwrap();
    let id = item["id"].as_str().unwrap().to_string();
    let adjusted: Value = as_phone(reqwest::Method::POST, &format!("/api/items/{id}/adjust"))
        .json(&json!({ "delta": -0.5 }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(adjusted["quantity"], 1.5);
    let r = as_phone(reqwest::Method::POST, "/api/items").json(&json!({ "name": "x", "category": "weapons" })).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 400, "unknown category refused");
    let r = as_phone(reqwest::Method::POST, &format!("/api/items/{id}/adjust")).json(&json!({ "delta": 0 })).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 400, "zero delta refused");

    let (_, summary) = hub.get("/api/supplies/summary").await;
    assert_eq!(summary["running_low"][0]["name"], "Brašno");
    let (_, shopping) = hub.get("/api/shopping").await;
    assert_eq!(shopping[0]["source"], "running_low");
    let (_, code) = hub.get("/api/barcodes/8600000000017").await;
    assert_eq!(code["item"]["name"], "Brašno");
    let (_, history) = hub.get("/api/history?limit=5").await;
    assert_eq!(history[0]["action"], "consume");
    assert_eq!(history[0]["actor"], "Ana's phone");

    // 11. Add-ons: download, verify and install a pack.
    let (st, _) = hub.post("/api/packs/test-pack/download", json!({})).await;
    assert_eq!(st, 202);
    let pack = wait_pack(&hub, "test-pack", &["installed", "failed"]).await;
    assert_eq!(pack["state"]["status"], "installed", "{pack}");
    let installed = hub.root.join("library/zim/tiny_test_2026-01.zim");
    assert_eq!(std::fs::metadata(&installed).unwrap().len(), 3_000_000);

    // 12. A pack whose checksum does not match is rejected and nothing is kept.
    hub.post("/api/packs/broken-pack/download", json!({})).await;
    let pack = wait_pack(&hub, "broken-pack", &["installed", "failed"]).await;
    assert_eq!(pack["state"]["status"], "failed");
    assert!(pack["state"]["error"].as_str().unwrap().contains("checksum"));
    assert!(!hub.root.join("library/zim/broken.zim").exists());

    // 13. Export to a "USB stick", remove, import back.
    let usb = temp_dir("usb");
    let (st, _) = hub.post("/api/packs/test-pack/export", json!({ "dir": usb.display().to_string() })).await;
    assert_eq!(st, 200);
    assert!(usb.join("zaklon-packs/tiny_test_2026-01.zim").is_file());
    let (st, _) = hub.send(reqwest::Method::DELETE, "/api/packs/test-pack", None).await;
    assert_eq!(st, 204);
    assert!(!installed.exists());
    let (_, imported) = hub.post("/api/packs/import", json!({ "dir": usb.display().to_string() })).await;
    assert_eq!(imported["imported"], json!(["test-pack"]));
    assert!(installed.exists());

    // 14. Resume: a partial download continues from where it stopped.
    hub.send(reqwest::Method::DELETE, "/api/packs/test-pack", None).await;
    let payload = std::fs::read(usb.join("zaklon-packs/tiny_test_2026-01.zim")).unwrap();
    std::fs::write(hub.root.join("library/zim/tiny_test_2026-01.zim.part"), &payload[..1_234_567]).unwrap();
    hub.post("/api/packs/test-pack/download", json!({})).await;
    let pack = wait_pack(&hub, "test-pack", &["installed", "failed"]).await;
    assert_eq!(pack["state"]["status"], "installed", "{pack}");
    assert_eq!(sha256_hex(&std::fs::read(&installed).unwrap()), sha256_hex(&payload));

    // 15. Library pages may never run scripts (sandbox), even opened directly.
    let r = hub.http.get(format!("{}/kiwix/content/x/y", hub.local)).send().await.unwrap();
    if r.status().is_success() {
        assert_eq!(r.headers()["content-security-policy"], "sandbox allow-popups");
    }
    // A cross-site navigation is never trusted as the laptop.
    let r = hub.http.get(format!("{}/api/devices", hub.local)).header("sec-fetch-site", "cross-site").send().await.unwrap();
    assert_eq!(r.status().as_u16(), 403);

    // 15b. Without the library engine add-on the library says so.
    let (_, lib) = hub.get("/api/library").await;
    assert_eq!(lib["engine"], "missing");
    let (_, results) = hub.get("/api/library/search?q=voda").await;
    assert_eq!(results, json!([]));

    // 16. The install page is public, the APK folder serves only files that exist.
    let page = reqwest::get(format!("{}/get", hub.install)).await.unwrap();
    assert_eq!(page.status().as_u16(), 200);
    assert!(page.text().await.unwrap().contains("Install Zaklon"));
    let apk = reqwest::get(format!("{}/apk/zaklon.apk", hub.install)).await.unwrap();
    assert_eq!(apk.status().as_u16(), 404);
    let sneaky = reqwest::get(format!("{}/apk/..%2F..%2Fhousehold%2Fhub.json", hub.install)).await.unwrap();
    assert_ne!(sneaky.status().as_u16(), 200, "no path traversal out of the APK folder");

    // 17. Discovery beacon answers with the same fingerprint.
    let sock = tokio::net::UdpSocket::bind(("127.0.0.1", 0)).await.unwrap();
    // A short request gets no answer (no amplification)...
    sock.send_to(b"ZAKLON?", SocketAddr::from(([127, 0, 0, 1], hub.beacon_port))).await.unwrap();
    let mut probe = [0u8; 512];
    assert!(tokio::time::timeout(Duration::from_millis(500), sock.recv_from(&mut probe)).await.is_err(), "short request ignored");
    // ...a padded one does.
    let mut request = b"ZAKLON?".to_vec();
    request.resize(zaklon_hub::discovery::BEACON_MIN_REQUEST, 0);
    sock.send_to(&request, SocketAddr::from(([127, 0, 0, 1], hub.beacon_port))).await.unwrap();
    let mut buf = [0u8; 512];
    let (n, _) = tokio::time::timeout(Duration::from_secs(3), sock.recv_from(&mut buf)).await.expect("beacon reply").unwrap();
    let beacon: Value = serde_json::from_slice(&buf[..n]).unwrap();
    assert_eq!(beacon["fp"], fingerprint.as_str());

    // 18. Removing the device revokes its token at once.
    let device_id = paired["device_id"].as_str().unwrap();
    let (st, _) = hub.send(reqwest::Method::DELETE, &format!("/api/devices/{device_id}"), None).await;
    assert_eq!(st, 204);
    let r = as_phone(reqwest::Method::GET, "/api/items").send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);

    // 19. The password can be changed from the laptop; the old one no longer pairs.
    let (st, _) = hub.post("/api/password", json!({ "new_password": "new household pw" })).await;
    assert_eq!(st, 204);
    let (_, pair) = hub.post("/api/pair/start", json!({})).await;
    let r = phone
        .post(format!("{}/api/pair/complete", hub.tls))
        .json(&json!({ "code": pair["code"], "password": "correct horse", "device_name": "x" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 403);

    // 20. Env port overrides were not written to disk.
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(hub.root.join("household/hub.json")).unwrap()).unwrap();
    assert_eq!(saved["port"], 8484);
    assert_eq!(saved["local_port"], 8481);
}
