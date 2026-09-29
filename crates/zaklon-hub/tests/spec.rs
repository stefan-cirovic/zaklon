//! The system specification (Settings › About) against a real hub: the
//! laptop and a paired phone read it, every group and value it promises is
//! there, slow values arrive without the answer waiting for them, and
//! nothing secret is in it.
//!
//! Run with: `cargo test -p zaklon-hub --test spec -- --nocapture`

mod common;

use std::time::{Duration, Instant};

use common::*;
use serde_json::{json, Value};

const PASSWORD: &str = "correct horse";

/// "3.46.0", "1.90.0": numbers with dots.
fn is_version(v: &Value) -> bool {
    v.as_str().is_some_and(|s| s.split('.').count() >= 2 && s.split('.').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())))
}

/// Every field name in the answer, however deep.
fn field_names(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(map) => {
            for (k, v) in map {
                out.push(k.clone());
                field_names(v, out);
            }
        }
        Value::Array(list) => list.iter().for_each(|v| field_names(v, out)),
        _ => {}
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_system_specification_is_complete_and_holds_nothing_secret() {
    let hub = run_hub(temp_dir("spec")).await;
    assert_eq!(hub.post("/api/setup", json!({ "password": PASSWORD, "hub_name": "Spec hub", "language": "en" })).await.0, 204);
    let (_, status) = hub.get("/api/status").await;
    let fingerprint = status["fingerprint"].as_str().unwrap().to_string();
    // A backup, so there is a last one.
    let (st, made) = hub.post("/api/backups", json!({})).await;
    assert_eq!(st, 200, "{made}");

    // A phone pairs.
    let (_, start) = hub.post("/api/pair/start", json!({})).await;
    let pairing_secret = start["payload"]["secret"].as_str().unwrap().to_string();
    let phone = phone_client(&fingerprint);
    let r = phone
        .post(format!("{}/api/pair/complete", hub.tls))
        .json(&json!({ "secret": pairing_secret, "password": PASSWORD, "device_name": "Ana's phone" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let paired: Value = r.json().await.unwrap();
    let token = paired["device_token"].as_str().unwrap().to_string();

    // The laptop reads it at once, whatever is still being read.
    let asked = Instant::now();
    let (st, spec) = hub.get("/api/system/spec").await;
    assert_eq!(st, 200, "{spec}");
    assert!(asked.elapsed() < Duration::from_secs(5), "the answer does not wait for slow values");
    for group in ["app", "built_with", "database", "assistant", "library", "maps", "network"] {
        assert!(spec[group].is_object(), "{group}: {spec}");
    }
    assert!(spec["pending"].is_array());

    let app = &spec["app"];
    assert_eq!(app["version"], status["version"]);
    assert!(["release", "development"].contains(&app["mode"].as_str().unwrap()), "{app}");
    assert_eq!(app["build_date"].as_str().unwrap().len(), 10, "YYYY-MM-DD: {app}");
    assert!(app["commit"].as_str().unwrap().chars().all(|c| c.is_ascii_hexdigit()), "{app}");
    assert!(!app["os"].as_str().unwrap().is_empty(), "{app}");
    #[cfg(windows)]
    assert!(app["os"].as_str().unwrap().starts_with("Windows") && app["os_build"].is_string(), "{app}");

    let built = &spec["built_with"];
    assert!(is_version(&built["rust"]), "{built}");
    assert!(is_version(&built["tauri"]) && built["tauri"].as_str().unwrap().starts_with("2."), "{built}");
    assert!(is_version(&built["sqlite"]) && built["sqlite"].as_str().unwrap().starts_with("3."), "{built}");
    // A fresh hub has no engines yet: the versions Add-ons offers.
    assert_eq!(built["llama_cpp"]["installed"], false);
    assert!(built["llama_cpp"]["version"].as_str().unwrap().starts_with('b'), "{built}");
    assert_eq!(built["kiwix_serve"]["installed"], false);
    assert!(is_version(&built["kiwix_serve"]["version"]), "{built}");
    assert!(built["libzim"].is_null(), "no library engine to ask");
    assert_eq!(built["comaps"], spec["maps"]["comaps_app"]);
    assert!(built["comaps"].as_str().unwrap().starts_with("20"), "{built}");

    let db = &spec["database"];
    assert_eq!(db["sqlite"], built["sqlite"]);
    assert!(db["size"].as_u64().unwrap() > 0, "{db}");
    assert_eq!(db["migrations"], json!(["batches_v1"]));
    let last = db["last_backup"].as_str().expect("the backup just made");
    assert!(last.ends_with('Z') && last.contains('T'), "RFC 3339: {last}");

    let ai = &spec["assistant"];
    assert_eq!(ai["engine"], "missing");
    assert_eq!(ai["models"], json!([]));
    assert!(ai["in_use"].is_null());
    assert!(ai["ram_total"].as_u64().unwrap() > 0 && ai["threads"].as_u64().unwrap() >= 1, "{ai}");
    assert!(ai["recommended"].is_null() || ai["recommended"]["id"].is_string(), "{ai}");
    assert!(ai["cpu"].is_string());

    let lib = &spec["library"];
    assert_eq!((lib["packs"].as_u64(), lib["size"].as_u64()), (Some(0), Some(0)), "{lib}");
    assert!(lib["folder"].as_str().unwrap().ends_with("library"), "{lib}");
    assert!(lib["drive_total"].as_u64().unwrap() > 0 && lib["drive"].is_string(), "{lib}");

    let maps = &spec["maps"];
    assert!(maps["world"].is_null(), "no world map: {maps}");
    assert_eq!((maps["comaps_maps"].as_u64(), maps["comaps_app_on_hub"].as_bool()), (Some(0), Some(false)), "{maps}");

    let net = &spec["network"];
    assert_eq!(net["port"], status["port"]);
    assert_eq!(net["phones"], 1);
    assert!(net["addresses"].is_array());
    let short = net["fingerprint"].as_str().unwrap();
    assert!(short.len() == 16 && fingerprint.starts_with(short), "only the start of the fingerprint: {short}");

    // A paired phone reads the same over its pinned connection; nobody else can.
    let r = phone.get(format!("{}/api/system/spec", hub.tls)).bearer_auth(&token).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let from_phone: Value = r.json().await.unwrap();
    assert_eq!(from_phone["app"]["version"], app["version"]);
    assert_eq!(from_phone["network"]["phones"], 1);
    let r = phone.get(format!("{}/api/system/spec", hub.tls)).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401, "no token, no specification");
    let r = phone.get(format!("{}/api/system/spec", hub.tls)).bearer_auth("not-a-token").send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);

    // What was still being read arrives.
    let deadline = Instant::now() + Duration::from_secs(90);
    let settled = loop {
        let (st, s) = hub.get("/api/system/spec").await;
        assert_eq!(st, 200);
        if s["pending"].as_array().unwrap().is_empty() {
            break s;
        }
        assert!(Instant::now() < deadline, "still pending: {}", s["pending"]);
        tokio::time::sleep(Duration::from_millis(250)).await;
    };
    #[cfg(windows)]
    {
        let profiles = settled["network"]["profiles"].as_array().expect("how Windows files the networks");
        assert!(profiles.iter().all(|p| ["Private", "Public", "Domain"].contains(&p["category"].as_str().unwrap())), "{profiles:?}");
    }

    // Nothing secret, from the laptop or the phone.
    let db_file = hub.root.join("household").join("household.db");
    let hash = zaklon_core::db::setting_in(&db_file, "household_password_hash").unwrap().expect("the password's hash");
    let backup_key: Value = serde_json::from_str(&zaklon_core::db::setting_in(&db_file, "backup_key").unwrap().expect("the backup key")).unwrap();
    let tls_key = std::fs::read_to_string(hub.root.join("household").join("tls").join("hub-key.pem")).unwrap();
    let token_hash = sha256_hex(token.as_bytes());
    let mut secrets: Vec<(String, &str)> = vec![
        (PASSWORD.into(), "the household password"),
        (hash.clone(), "the password's hash"),
        (token.clone(), "the phone's token"),
        (token_hash, "the token's hash"),
        (pairing_secret, "the pairing secret"),
        (backup_key["recipient"].as_str().unwrap().into(), "the backup key's recipient"),
        (backup_key["locked"].as_str().unwrap().into(), "the locked backup key"),
        (fingerprint.clone(), "the whole fingerprint"),
    ];
    // The hash's salt and digest ($argon2id$v=19$m=...,t=...,p=...$salt$digest).
    secrets.extend(hash.split('$').filter(|p| p.len() >= 16).map(|p| (p.to_string(), "part of the password's hash")));
    // The private key, line by line.
    secrets.extend(tls_key.lines().filter(|l| !l.starts_with("-----") && l.len() >= 16).map(|l| (l.to_string(), "the TLS key")));
    for (answer, who) in [(&settled, "laptop"), (&from_phone, "phone")] {
        let text = answer.to_string();
        for (secret, what) in &secrets {
            assert!(!text.contains(secret.as_str()), "{what} is in what the {who} sees");
        }
        assert!(!text.contains("PRIVATE KEY"), "a key is in what the {who} sees");
        let mut names = Vec::new();
        field_names(answer, &mut names);
        for name in names {
            assert!(!["password", "token", "secret", "hash", "key"].iter().any(|w| name.contains(w)), "a field named {name:?}");
        }
    }
}
