//! Pairing with a hub found on the network ("Find hubs"), against a real hub.
//! The phone checks with the pairing code that it talks to the hub whose
//! certificate it sees (SPAKE2, see the zaklon-pake crate) before it sends
//! the household password. The right code pairs; a wrong code costs one of
//! the code's attempts; a device that answers in place of the hub and passes
//! the messages on with its own certificate is caught before the password
//! is sent.
//!
//! Run with: `cargo test -p zaklon-hub --test pake -- --nocapture`

mod common;

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::*;
use serde_json::{json, Value};
use zaklon_pake::{FinishRequest, Phone, Refusal, StartReply, StartRequest};

// ---- the phone's three steps ----------------------------------------------------

/// Step 1 at `base`: the hub's answer and the certificate it came with, or
/// the refusal (status and body).
async fn start(base: &str, phone: &Phone) -> Result<(StartReply, [u8; 32]), (u16, Value)> {
    let res = finder_client()
        .post(format!("{base}{}", zaklon_pake::START_PATH))
        .json(&StartRequest { msg: zaklon_pake::to_hex(phone.message()) })
        .send()
        .await
        .unwrap();
    let seen = seen_certificate(&res);
    let status = res.status().as_u16();
    let body: Value = res.json().await.unwrap();
    if status != 200 {
        return Err((status, body));
    }
    Ok((serde_json::from_value(body).unwrap(), seen))
}

/// Step 2, on the phone: the hub's answer checked against the certificate it
/// came with. Returns the phone's proof.
fn check(phone: Phone, reply: &StartReply, seen: &[u8; 32]) -> Result<String, Refusal> {
    let hex = |s: &str| zaklon_pake::from_hex(s).unwrap();
    phone.check(&hex(&reply.msg), &hex(&reply.check), &hex(&reply.proof), seen).map(|p| zaklon_pake::to_hex(&p))
}

/// Step 3, over a connection pinned to `fingerprint`.
async fn finish(base: &str, fingerprint: &str, session: &str, proof: &str, password: &str, nonce: Option<&str>) -> (u16, Value) {
    let body = FinishRequest {
        session: session.into(),
        proof: proof.into(),
        password: password.into(),
        device_name: "Found phone".into(),
        platform: None,
        nonce: nonce.map(String::from),
    };
    let res = phone_client(fingerprint).post(format!("{base}{}", zaklon_pake::FINISH_PATH)).json(&body).send().await.unwrap();
    (res.status().as_u16(), res.json().await.unwrap())
}

/// All three steps, the way the phone app takes them (`pair_found` in
/// client.rs): it stops at the first refusal and says why.
async fn pair_like_the_phone(base: &str, code: &str, password: &str) -> Result<Value, String> {
    let error = |body: &Value| body["error"].as_str().unwrap_or_default().to_string();
    let phone = Phone::start(code);
    let (reply, seen) = start(base, &phone).await.map_err(|(_, body)| error(&body))?;
    let proof = check(phone, &reply, &seen).map_err(|r| r.to_string())?;
    match finish(base, &zaklon_pake::to_hex(&seen), &reply.session, &proof, password, None).await {
        (200, paired) => Ok(paired),
        (_, body) => Err(error(&body)),
    }
}

async fn new_code(hub: &Hub) -> String {
    let (st, pair) = hub.post("/api/pair/start", json!({})).await;
    assert_eq!(st, 200, "{pair}");
    pair["code"].as_str().unwrap().to_string()
}

/// The hub's error code in a refusal.
fn code_of(refusal: (u16, Value)) -> (u16, String) {
    (refusal.0, refusal.1["code"].as_str().unwrap_or_default().to_string())
}

// ---- a device in between --------------------------------------------------------

/// A device on the Wi-Fi that answers "Find hubs" in place of the hub, with
/// its own certificate, and passes every request on to the real hub (over a
/// connection pinned to the hub, as a paired phone would). It keeps what
/// passed through it.
struct Relay {
    url: String,
    fingerprint: String,
    passed: Arc<Mutex<Vec<String>>>,
}

async fn relay_to(hub: &str, hub_fingerprint: &str) -> Relay {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let own = zaklon_core::tls::load_or_generate(&temp_dir("relay-tls"), "Zaklon on HOME-PC").unwrap();
    let passed = Arc::new(Mutex::new(Vec::new()));
    let (client, hub, log) = (phone_client(hub_fingerprint), hub.to_string(), passed.clone());
    let app = axum::Router::new().fallback(move |uri: axum::http::Uri, body: String| {
        let (client, hub, log) = (client.clone(), hub.clone(), log.clone());
        async move {
            log.lock().unwrap().push(body.clone());
            let res = client
                .post(format!("{hub}{}", uri.path()))
                .header("content-type", "application/json")
                .body(body)
                .send()
                .await
                .unwrap();
            let status = axum::http::StatusCode::from_u16(res.status().as_u16()).unwrap();
            (status, [(axum::http::header::CONTENT_TYPE, "application/json")], res.text().await.unwrap())
        }
    });
    let port = free_port();
    let tls = axum_server::tls_rustls::RustlsConfig::from_pem(own.cert_pem.into_bytes(), own.key_pem.into_bytes())
        .await
        .unwrap();
    tokio::spawn(axum_server::bind_rustls(SocketAddr::from(([127, 0, 0, 1], port)), tls).serve(app.into_make_service()));
    let deadline = Instant::now() + Duration::from_secs(10);
    while tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_err() {
        assert!(Instant::now() < deadline, "the relay did not start");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Relay { url: format!("https://127.0.0.1:{port}"), fingerprint: own.fingerprint, passed }
}

// ---- the test -------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pairing_with_a_hub_found_on_the_network() {
    let hub = run_hub(temp_dir("pake-hub")).await;
    let fingerprint = hub.get("/api/status").await.1["fingerprint"].as_str().unwrap().to_string();
    let (st, _) = hub.post("/api/setup", json!({ "password": "correct horse" })).await;
    assert_eq!(st, 204);

    // 1. Before the laptop shows a code there is nothing to check against.
    let refused = start(&hub.tls, &Phone::start("123456")).await.unwrap_err();
    assert_eq!(code_of(refused), (403, "code_expired".into()));

    // 2. The right code: the hub proves its certificate, the phone proves the
    //    code, and the household password pairs the phone.
    let code = new_code(&hub).await;
    let phone = Phone::start(&code);
    let (reply, seen) = start(&hub.tls, &phone).await.unwrap();
    assert_eq!(zaklon_pake::to_hex(&seen), fingerprint, "the phone reached the hub itself");
    let proof = check(phone, &reply, &seen).expect("the hub proves its certificate");
    let nonce = Some("0123456789abcdef-pake");
    let (st, paired) = finish(&hub.tls, &fingerprint, &reply.session, &proof, "correct horse", nonce).await;
    assert_eq!(st, 200, "{paired}");
    assert_eq!(paired["fingerprint"], fingerprint.as_str());
    // The reply "got lost": the same request gets the same device, not a second one.
    let (st, again) = finish(&hub.tls, &fingerprint, &reply.session, &proof, "correct horse", nonce).await;
    assert_eq!((st, &again["device_id"]), (200, &paired["device_id"]));
    // Without that nonce it is a new answer to a check that was answered already.
    let refused = finish(&hub.tls, &fingerprint, &reply.session, &proof, "correct horse", None).await;
    assert_eq!(code_of(refused), (403, "code_expired".into()));
    let token = paired["device_token"].as_str().unwrap();
    let r = phone_client(&fingerprint).get(format!("{}/api/devices", hub.tls)).bearer_auth(token).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let devices: Value = r.json().await.unwrap();
    assert_eq!(devices.as_array().unwrap().len(), 1);
    assert_eq!(devices[0]["name"], "Found phone");
    // The code is used up.
    let err = pair_like_the_phone(&hub.tls, &code, "correct horse").await.unwrap_err();
    assert!(err.contains("invalid or expired"), "{err}");

    // 3. A wrong code: the phone notices before it sends anything more, and
    //    each try takes one of the code's three attempts.
    let code = new_code(&hub).await;
    let wrong = if code == "000000" { "111111" } else { "000000" };
    for i in 0..3 {
        let phone = Phone::start(wrong);
        let (reply, seen) = start(&hub.tls, &phone).await.unwrap();
        assert_eq!(check(phone, &reply, &seen), Err(Refusal::WrongCode), "try {i}");
    }
    let refused = start(&hub.tls, &Phone::start(&code)).await.unwrap_err();
    assert_eq!(code_of(refused), (403, "too_many_attempts".into()), "the code is burned");

    // 4. Without the phone's proof of the code the hub never looks at the
    //    password, and each check is answered once.
    let code = new_code(&hub).await;
    let phone = Phone::start(&code);
    let (reply, seen) = start(&hub.tls, &phone).await.unwrap();
    let refused = finish(&hub.tls, &fingerprint, &reply.session, &"00".repeat(32), "correct horse", None).await;
    assert_eq!(code_of(refused), (403, "wrong_code".into()));
    let proof = check(phone, &reply, &seen).unwrap();
    let refused = finish(&hub.tls, &fingerprint, &reply.session, &proof, "correct horse", None).await;
    assert_eq!(code_of(refused), (403, "code_expired".into()), "a check is answered once");
    // A wrong password takes an attempt too (the second of this code).
    let err = pair_like_the_phone(&hub.tls, &code, "wrong password").await.unwrap_err();
    assert_eq!(err, "wrong household password");

    // 5. A device in between. The right code goes through it, so the phone
    //    and the hub agree on the key, but the hub proves its own
    //    certificate, not the one the phone got from the relay: the phone
    //    stops, and the password never passes the relay.
    let code = new_code(&hub).await;
    let relay = relay_to(&hub.tls, &fingerprint).await;
    assert_ne!(relay.fingerprint, fingerprint);
    let phone = Phone::start(&code);
    let (reply, seen) = start(&relay.url, &phone).await.unwrap();
    assert_eq!(zaklon_pake::to_hex(&seen), relay.fingerprint, "the phone reached the relay");
    assert_eq!(check(phone, &reply, &seen), Err(Refusal::NotTheHub));
    let err = pair_like_the_phone(&relay.url, &code, "correct horse").await.unwrap_err();
    assert!(err.contains("in place of the hub"), "{err}");
    let passed = relay.passed.lock().unwrap().clone();
    assert_eq!(passed.len(), 2, "only the first steps went through the relay");
    assert!(passed.iter().all(|body| !body.contains("password") && !body.contains("correct horse")));
    // The relay saw a whole check go by, but without the key it cannot answer it.
    let refused = finish(&relay.url, &relay.fingerprint, &reply.session, &"00".repeat(32), "guess", None).await;
    assert_eq!(code_of(refused), (403, "wrong_code".into()));
    // The same code still pairs with the hub itself (its third and last attempt).
    let paired = pair_like_the_phone(&hub.tls, &code, "correct horse").await.unwrap();
    assert_eq!(paired["fingerprint"], fingerprint.as_str());

    // 6. Every check counts as a failure of its address until it pairs; too
    //    many and that address waits, even with a new code on the laptop.
    let mut blocked = false;
    for _ in 0..=zaklon_hub::PairingFailures::MAX_PER_IP {
        new_code(&hub).await;
        match start(&hub.tls, &Phone::start("000000")).await {
            Ok(_) => {}
            Err(refused) => {
                assert_eq!(code_of(refused), (429, "device_blocked".into()));
                blocked = true;
                break;
            }
        }
    }
    assert!(blocked, "too many failed checks block the address");
}
