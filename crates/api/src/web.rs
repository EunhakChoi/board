//! Serves the built web tool (`apps/board-web/dist`) next to the API, with the single-page app
//! fallback: unknown paths outside `/api` get `index.html`.

use std::path::{Component, Path, PathBuf};

use axum::http::{StatusCode, Uri, header};
use axum::response::{Html, IntoResponse, Response};

use crate::ApiError;

const NOT_BUILT: &str = "<!doctype html><title>board</title><p>The board web tool is not built. \
Run <code>pnpm --filter board-web build</code> or set <code>BOARD_WEB_DIR</code>.</p>";

pub async fn serve(dir: Option<PathBuf>, uri: Uri) -> Response {
    let path = uri.path();
    if path == "/api" || path.starts_with("/api/") {
        return ApiError::not_found("no such route").into_response();
    }
    let Some(dir) = dir else {
        return Html(NOT_BUILT).into_response();
    };
    let relative = path.trim_start_matches('/');
    let file = safe_join(&dir, relative)
        .filter(|f| f.is_file())
        .unwrap_or_else(|| dir.join("index.html"));
    let Ok(bytes) = tokio::fs::read(&file).await else {
        return Html(NOT_BUILT).into_response();
    };
    // Hashed assets never change; the page itself is always revalidated.
    let cache = if relative.starts_with("assets/") && file != dir.join("index.html") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type(&file)),
            (header::CACHE_CONTROL, cache),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        bytes,
    )
        .into_response()
}

/// `dir/relative` when `relative` stays inside `dir`.
fn safe_join(dir: &Path, relative: &str) -> Option<PathBuf> {
    let candidate = Path::new(relative);
    if relative.is_empty()
        || candidate
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return None;
    }
    Some(dir.join(candidate))
}

fn content_type(file: &Path) -> &'static str {
    match file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
    {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_only_inside() {
        let dir = Path::new("/srv/web");
        assert_eq!(safe_join(dir, "assets/a.js"), Some(dir.join("assets/a.js")));
        assert_eq!(safe_join(dir, "../etc/passwd"), None);
        assert_eq!(safe_join(dir, "/etc/passwd"), None);
        assert_eq!(safe_join(dir, ""), None);
    }
}
