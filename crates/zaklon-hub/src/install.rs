//! Plain-HTTP "install the app" site on port 8480. Deliberately tiny: a page
//! explaining the two steps and the APK files from `library/apk`. Nothing
//! private is reachable here.

use std::sync::Arc;

use axum::{
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use tower_http::services::ServeDir;

use crate::HubState;

pub fn router(state: Arc<HubState>) -> Router {
    let apk_dir = state.config().library_dir().join("apk");
    Router::new()
        .route("/", get(page))
        .route("/get", get(page))
        .nest_service("/apk", ServeDir::new(apk_dir))
        .with_state(state)
}

async fn page(axum::extract::State(state): axum::extract::State<Arc<HubState>>) -> impl IntoResponse {
    let cfg = state.config();
    let apk_dir = cfg.library_dir().join("apk");
    let mut apks: Vec<String> = std::fs::read_dir(&apk_dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.to_lowercase().ends_with(".apk"))
                .collect()
        })
        .unwrap_or_default();
    apks.sort();
    let links = if apks.is_empty() {
        "<p class=\"muted\">No app packages are on this hub yet.</p>".to_string()
    } else {
        apks.iter()
            .map(|n| format!("<a class=\"btn\" href=\"/apk/{}\">Download {}</a>", url_encode(n), html_escape(n)))
            .collect::<Vec<_>>()
            .join("\n")
    };
    Html(format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Install Zaklon</title>
<style>
body{{margin:0;background:#0b0d10;color:#f2f4f7;font-family:system-ui,-apple-system,Segoe UI,Roboto,sans-serif;line-height:1.5}}
main{{max-width:520px;margin:0 auto;padding:32px 20px}}
h1{{font-size:28px;margin:0 0 8px}} .muted{{color:#9aa3ad}}
.btn{{display:block;margin:12px 0;padding:14px 18px;border:1px solid #2a2f36;border-radius:10px;color:#f2f4f7;text-decoration:none;font-weight:600}}
.btn:hover{{border-color:#4ade80}} ol{{padding-left:20px}} li{{margin:8px 0}}
</style></head><body><main>
<h1>Zaklon</h1><p class="muted">{name}</p>
<ol>
<li>Download the app below. Android will ask you to allow installs from this browser once.</li>
<li>Open Zaklon, scan the QR code shown on the laptop, and enter the household password.</li>
</ol>
{links}
<p class="muted">Also available: CoMaps for offline maps, if it has been added to this hub.</p>
</main></body></html>"#,
        name = html_escape(&cfg.hub_name),
    ))
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}
