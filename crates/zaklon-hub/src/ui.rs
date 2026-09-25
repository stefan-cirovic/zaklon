//! Serves the built React interface embedded in the binary (release) or read
//! from `ui/dist` on disk (debug), with an SPA fallback to index.html.

use axum::{
    body::Body,
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../ui/dist"]
struct Assets;

pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match Assets::get(path).or_else(|| Assets::get("index.html")) {
        Some(file) => {
            let served = if Assets::get(path).is_some() { path } else { "index.html" };
            let mime = mime_guess::from_path(served).first_or_octet_stream();
            let cache = if served == "index.html" { "no-cache" } else { "public, max-age=31536000, immutable" };
            (
                [(header::CONTENT_TYPE, mime.as_ref().to_string()), (header::CACHE_CONTROL, cache.to_string())],
                Body::from(file.data.into_owned()),
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}
