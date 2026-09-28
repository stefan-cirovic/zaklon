//! Saved assistant conversations against a real hub, driven like the laptop
//! window (loopback) and two paired phones (TLS with their tokens): each
//! device sees and changes only its own conversations, a copy can be sent to
//! another device of the household, and a removed phone's conversations go.
//! The test hub has no AI engine, so every answer fails at once; that is
//! enough to save conversations and their turns.
//!
//! Run with: `cargo test -p zaklon-hub --test conversations -- --nocapture`

mod common;

use std::time::{Duration, Instant};

use common::*;
use reqwest::Method;
use serde_json::{json, Value};

const PASSWORD: &str = "correct horse";

/// The laptop (no token, loopback) or a phone (TLS, its token).
struct Dev {
    http: reqwest::Client,
    base: String,
    token: Option<String>,
    id: String,
}

impl Dev {
    async fn call(&self, method: Method, path: &str, body: Option<Value>) -> (u16, Value) {
        let mut req = self.http.request(method, format!("{}{path}", self.base));
        if let Some(t) = &self.token {
            req = req.bearer_auth(t);
        }
        if let Some(b) = body {
            req = req.json(&b);
        }
        let r = req.send().await.unwrap();
        let st = r.status().as_u16();
        let text = r.text().await.unwrap();
        (st, serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }
    async fn get(&self, path: &str) -> (u16, Value) {
        self.call(Method::GET, path, None).await
    }
    async fn post(&self, path: &str, body: Value) -> (u16, Value) {
        self.call(Method::POST, path, Some(body)).await
    }
    /// Ask in a new conversation; returns the conversation's id.
    async fn ask_new(&self, question: &str) -> String {
        let (st, r) = self.post("/api/assistant/ask", json!({ "question": question, "language": "en", "new_conversation": true })).await;
        assert_eq!(st, 200, "{r}");
        assert_eq!(r["turn"]["status"], "pending");
        assert_eq!(r["turn"]["answer_id"], r["id"]);
        r["conversation"]["id"].as_str().unwrap().to_string()
    }
    /// The conversation once none of its answers is still being written.
    async fn answered(&self, id: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let (st, c) = self.get(&format!("/api/conversations/{id}")).await;
            assert_eq!(st, 200, "{c}");
            if c["turns"].as_array().unwrap().iter().all(|t| t["status"] != "pending") {
                return c;
            }
            assert!(Instant::now() < deadline, "answers did not finish: {c}");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    async fn titles(&self) -> Vec<String> {
        let (st, list) = self.get("/api/conversations").await;
        assert_eq!(st, 200, "{list}");
        list.as_array().unwrap().iter().map(|c| c["title"].as_str().unwrap().to_string()).collect()
    }
}

async fn pair(hub: &Hub, fingerprint: &str, name: &str) -> Dev {
    let (_, start) = hub.post("/api/pair/start", json!({})).await;
    let http = phone_client(fingerprint);
    let r = http
        .post(format!("{}/api/pair/complete", hub.tls))
        .json(&json!({ "secret": start["payload"]["secret"], "password": PASSWORD, "device_name": name }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let paired: Value = r.json().await.unwrap();
    Dev {
        http,
        base: hub.tls.clone(),
        token: Some(paired["device_token"].as_str().unwrap().to_string()),
        id: paired["device_id"].as_str().unwrap().to_string(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn each_device_keeps_its_own_conversations_and_can_send_a_copy() {
    let hub = run_hub(temp_dir("conversations-Đorđe")).await;
    assert_eq!(hub.post("/api/setup", json!({ "password": PASSWORD, "hub_name": "E2E hub", "language": "en" })).await.0, 204);
    let fingerprint = hub.get("/api/status").await.1["fingerprint"].as_str().unwrap().to_string();
    let laptop = Dev { http: hub.http.clone(), base: hub.local.clone(), token: None, id: "laptop".into() };
    let ana = pair(&hub, &fingerprint, "Ana's phone").await;
    let marko = pair(&hub, &fingerprint, "Marko's phone").await;

    // 1. The laptop asks in a new conversation, titled from the question.
    //    Without an AI engine the answer fails, and the failure is saved.
    let water = laptop.ask_new("How do I purify water without a filter?").await;
    let c = laptop.answered(&water).await;
    assert_eq!(c["title"], "How do I purify water without a filter?");
    assert_eq!(c["turns"][0]["status"], "failed");
    let error = c["turns"][0]["error"].as_str().unwrap();
    assert!(error.contains("not installed") || error.contains("no AI model"), "{error}");
    // A second question in the same conversation.
    let (st, r) = laptop.post("/api/assistant/ask", json!({ "question": "And with a pot?", "conversation": water })).await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(r["conversation"]["id"], water.as_str());
    assert_eq!(laptop.answered(&water).await["turns"].as_array().unwrap().len(), 2);
    // A question without either field is answered but not saved (older apps).
    let (st, r) = laptop.post("/api/assistant/ask", json!({ "question": "Not saved" })).await;
    assert_eq!(st, 200);
    assert!(r.get("conversation").is_none());
    assert_eq!(laptop.titles().await, ["How do I purify water without a filter?"]);

    // 2. A phone sees none of it, and can change none of it.
    assert!(ana.titles().await.is_empty());
    let path = format!("/api/conversations/{water}");
    let turn = c["turns"][0]["id"].as_str().unwrap().to_string();
    for (method, path, body) in [
        (Method::GET, path.clone(), None),
        (Method::PATCH, path.clone(), Some(json!({ "title": "Mine now" }))),
        (Method::DELETE, path.clone(), None),
        (Method::POST, format!("{path}/send"), Some(json!({ "to": "laptop" }))),
        (Method::PATCH, format!("{path}/turns/{turn}"), Some(json!({ "outcome": "done" }))),
    ] {
        let (st, body) = ana.call(method.clone(), &path, body).await;
        assert_eq!((st, body["code"].as_str()), (404, Some("not_found")), "{method} {path}");
    }
    let (st, _) = ana.post("/api/assistant/ask", json!({ "question": "Sneaking in", "conversation": water })).await;
    assert_eq!(st, 404, "a phone cannot ask in the laptop's conversation");
    let c = laptop.answered(&water).await;
    assert_eq!(c["title"], "How do I purify water without a filter?");
    assert_eq!(c["turns"].as_array().unwrap().len(), 2, "nothing was added");
    // Without a token nothing is shown at all.
    let r = ana.http.get(format!("{}/api/conversations", hub.tls)).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);

    // 3. The phone's own conversation is its own: not the laptop's, not the other phone's.
    let beans = ana.ask_new("How long do beans keep?").await;
    ana.answered(&beans).await;
    assert_eq!(ana.titles().await, ["How long do beans keep?"]);
    assert_eq!(laptop.titles().await, ["How do I purify water without a filter?"]);
    assert!(marko.titles().await.is_empty());
    assert_eq!(laptop.get(&format!("/api/conversations/{beans}")).await.0, 404, "the laptop does not see a phone's either");

    // 4. The laptop sends a copy to Ana's phone: a new conversation there, from the laptop.
    assert_eq!(laptop.post(&format!("{path}/send"), json!({ "to": ana.id })).await.0, 204);
    let (_, list) = ana.get("/api/conversations").await;
    assert_eq!(list[0]["title"], "How do I purify water without a filter?");
    assert_eq!(list[0]["from_owner"], "laptop");
    assert_eq!(list[0]["from_name"], "laptop");
    assert_eq!(list[0]["turns"], 2);
    let copy = list[0]["id"].as_str().unwrap().to_string();
    assert_ne!(copy, water);
    let c = ana.answered(&copy).await;
    assert_eq!(c["turns"][1]["question"], "And with a pot?");
    // Only to a device of the household, and not to itself.
    for to in ["not-a-device", "laptop", ""] {
        let (st, _) = laptop.post(&format!("{path}/send"), json!({ "to": to })).await;
        assert_eq!(st, 404, "sent to {to:?}");
    }

    // 5. Ana sends her own to Marko's phone and to the laptop, named as Ana's phone.
    assert_eq!(ana.post(&format!("/api/conversations/{beans}/send"), json!({ "to": marko.id })).await.0, 204);
    assert_eq!(ana.post(&format!("/api/conversations/{beans}/send"), json!({ "to": "laptop" })).await.0, 204);
    let (_, list) = marko.get("/api/conversations").await;
    assert_eq!((list[0]["title"].as_str(), list[0]["from_name"].as_str(), list[0]["from_owner"].as_str()), (Some("How long do beans keep?"), Some("Ana's phone"), Some(ana.id.as_str())));
    let (_, list) = laptop.get("/api/conversations").await;
    assert_eq!(list[0]["from_name"], "Ana's phone");
    assert_eq!(list.as_array().unwrap().len(), 2);

    // 6. Ana renames and deletes her copy; the laptop's original stays as it was.
    let (st, renamed) = ana.call(Method::PATCH, &format!("/api/conversations/{copy}"), Some(json!({ "title": "  Water  " }))).await;
    assert_eq!((st, renamed["title"].as_str()), (200, Some("Water")));
    let (st, body) = ana.call(Method::PATCH, &format!("/api/conversations/{copy}"), Some(json!({ "title": " " }))).await;
    assert_eq!((st, body["code"].as_str()), (400, Some("name_required")));
    assert_eq!(ana.call(Method::DELETE, &format!("/api/conversations/{copy}"), None).await.0, 204);
    assert_eq!(ana.get(&format!("/api/conversations/{copy}")).await.0, 404);
    assert_eq!(laptop.answered(&water).await["title"], "How do I purify water without a filter?");

    // 7. Search looks in titles and questions, with or without diacritics.
    let tea = laptop.ask_new("Koji čaj pomaže za grlo?").await;
    laptop.answered(&tea).await;
    let (_, found) = laptop.get("/api/conversations?q=caj%20GRLO").await;
    assert_eq!(found.as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect::<Vec<_>>(), [tea.as_str()]);
    let (_, found) = laptop.get("/api/conversations?q=pot").await;
    assert_eq!(found[0]["id"], water.as_str(), "a later question counts too");

    // 8. The laptop removes Marko's phone: its conversations go with it.
    assert_eq!(hub.send(Method::DELETE, &format!("/api/devices/{}", marko.id), None).await.0, 204);
    let db = zaklon_core::rusqlite::Connection::open(hub.root.join("household/household.db")).unwrap();
    let left: i64 = db.query_row("SELECT COUNT(*) FROM conversations WHERE owner = ?1", [&marko.id], |r| r.get(0)).unwrap();
    assert_eq!(left, 0);
    let orphans: i64 = db
        .query_row("SELECT COUNT(*) FROM conversation_turns WHERE conversation_id NOT IN (SELECT id FROM conversations)", [], |r| r.get(0))
        .unwrap();
    assert_eq!(orphans, 0);
    let kept: i64 = db.query_row("SELECT COUNT(*) FROM conversations WHERE owner = ?1", [&ana.id], |r| r.get(0)).unwrap();
    assert_eq!(kept, 1, "the other phone keeps its own");
}
