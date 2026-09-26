//! Plain-HTTP site on port 8480 for phones that are not paired yet or use
//! other apps: a page explaining how to install Zaklon, the APK files from
//! `library/apk`, and the offline map files for CoMaps (public OpenStreetMap
//! data), at the paths CoMaps asks for when the hub is set as its map
//! download server: /maps/<series>/<version>/<Region>.mwm. Nothing private
//! is reachable here.

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
        .route("/maps/{*rest}", get(map_file))
        .with_state(state)
}

/// A map piece for CoMaps, with Range support (CoMaps resumes downloads).
async fn map_file(
    axum::extract::State(state): axum::extract::State<Arc<HubState>>,
    axum::extract::Path(rest): axum::extract::Path<String>,
    req: axum::extract::Request,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use tower::ServiceExt;
    let parts: Vec<&str> = rest.split('/').filter(|p| !p.is_empty()).collect();
    // .../<version>/<Region>.mwm  (with or without the series before it)
    let (Some(file), Some(version)) = (parts.last(), parts.len().checked_sub(2).and_then(|i| parts.get(i))) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let safe = file.ends_with(".mwm") && !file.contains("..") && !file.contains('\\') && version.chars().all(|c| c.is_ascii_digit());
    if !safe {
        return StatusCode::NOT_FOUND.into_response();
    }
    let path = state.config().library_dir().join("maps").join(version).join(file);
    if !path.is_file() {
        return StatusCode::NOT_FOUND.into_response();
    }
    match tower_http::services::ServeFile::new(path).oneshot(req).await {
        Ok(r) => r.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
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
