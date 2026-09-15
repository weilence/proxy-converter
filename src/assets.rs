//! Release-only admin UI assets. `frontend/dist/` is built by the frontend
//! toolchain and embedded into the binary at compile time; debug builds do
//! not serve the UI at all (see `admin::debug_page`).

use axum::{
    extract::Path,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "frontend/dist/"]
struct Dist;

/// The admin single-page app entry point.
pub async fn page() -> Response {
    serve("index.html")
}

/// Hashed Vite output under `assets/`.
pub async fn asset(Path(path): Path<String>) -> Response {
    if path.contains("..") {
        return StatusCode::NOT_FOUND.into_response();
    }
    serve(&format!("assets/{path}"))
}

fn serve(path: &str) -> Response {
    let Some(file) = Dist::get(path) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let mime = mime_guess::from_path(path).first_or_octet_stream();
    // The entry point must revalidate; hashed assets never change.
    let cache_control = if path == "index.html" {
        "no-cache"
    } else {
        "public, max-age=31536000, immutable"
    };

    (
        [
            (header::CONTENT_TYPE, mime.as_ref().to_owned()),
            (header::CACHE_CONTROL, cache_control.to_owned()),
        ],
        file.data,
    )
        .into_response()
}
