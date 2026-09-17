use axum::{
    body::Body,
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "playground/dist/"]
struct PlaygroundAssets;

pub async fn static_asset(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let asset_path = if path.is_empty() { "index.html" } else { path };

    if let Some(asset) = PlaygroundAssets::get(asset_path) {
        return asset_response(asset_path, asset.data.into_owned());
    }

    // SPA fallback. API routes are registered before this fallback, so unknown
    // frontend routes resolve to index.html without masking /v1 or /health.
    if let Some(index) = PlaygroundAssets::get("index.html") {
        return asset_response("index.html", index.data.into_owned());
    }

    (StatusCode::NOT_FOUND, "Playground assets are not embedded").into_response()
}

fn asset_response(path: &str, bytes: Vec<u8>) -> Response {
    let content_type = match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("ico") => "image/x-icon",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, if path == "index.html" { "no-cache" } else { "public, max-age=31536000, immutable" })
        .body(Body::from(bytes))
        .expect("valid static response")
}
