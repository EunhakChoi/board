//! Previews: the API route that starts and reports a commit's build, and the preview origin that
//! serves built files (`docs/design/store-runtime.md` §4, `security.md`).
//!
//! The preview origin is a separate listener (host or port) with no sign-in cookies. A request
//! carries a token signed for one (project, commit), either as `?t=` or, after the first page,
//! as a cookie scoped to that commit's path.

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use board_engine::{Build, Engine, Preview};
use serde::{Deserialize, Serialize};

use crate::projects::blocking;
use crate::{ApiError, Caller};

const COOKIE: &str = "board_preview";

#[derive(Deserialize)]
pub struct PreviewQuery {
    rev: Option<String>,
    #[serde(default)]
    retry: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewOut {
    commit: String,
    #[serde(flatten)]
    build: Build,
    #[serde(skip_serializing_if = "Option::is_none")]
    base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<String>,
}

impl From<Preview> for PreviewOut {
    fn from(preview: Preview) -> Self {
        Self {
            commit: preview.commit,
            build: preview.build,
            base_url: preview.base_url,
            token: preview.token,
        }
    }
}

/// `GET /api/projects/{key}/preview?rev=&retry=`: starts the build if needed; poll until ready.
pub async fn status(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(key): Path<String>,
    Query(query): Query<PreviewQuery>,
) -> Result<Json<PreviewOut>, ApiError> {
    let rev = query.rev.unwrap_or_else(|| "main".to_owned());
    let preview = blocking(move || engine.preview(&actor, &key, &rev, query.retry)).await?;
    Ok(Json(preview.into()))
}

/// Settings of the preview listener.
#[derive(Debug, Clone)]
pub struct PreviewOrigin {
    /// Origins allowed to frame previews (the tool), for `Content-Security-Policy: frame-ancestors`.
    pub frame_ancestors: Vec<String>,
}

#[derive(Clone)]
struct PreviewState {
    engine: Engine,
    csp: HeaderValue,
}

/// Router of the preview origin. Serves nothing but built files.
pub fn preview_router(engine: Engine, origin: &PreviewOrigin) -> Router {
    let csp = format!("frame-ancestors {}", origin.frame_ancestors.join(" "));
    let state = PreviewState {
        engine,
        csp: HeaderValue::from_str(&csp)
            .unwrap_or_else(|_| HeaderValue::from_static("frame-ancestors 'none'")),
    };
    Router::new()
        .route("/{key}/{commit}/", get(file_root))
        .route("/{key}/{commit}/{*path}", get(file))
        .fallback(|| async { plain(StatusCode::NOT_FOUND, "not found") })
        .with_state(state)
}

#[derive(Deserialize)]
struct TokenQuery {
    t: Option<String>,
}

async fn file_root(
    state: State<PreviewState>,
    Path((key, commit)): Path<(String, String)>,
    query: Query<TokenQuery>,
    headers: HeaderMap,
) -> Response {
    serve(state, key, commit, String::new(), query, headers).await
}

async fn file(
    state: State<PreviewState>,
    Path((key, commit, path)): Path<(String, String, String)>,
    query: Query<TokenQuery>,
    headers: HeaderMap,
) -> Response {
    serve(state, key, commit, path, query, headers).await
}

async fn serve(
    State(state): State<PreviewState>,
    key: String,
    commit: String,
    path: String,
    Query(query): Query<TokenQuery>,
    headers: HeaderMap,
) -> Response {
    let from_query = query.t.as_deref();
    let token = from_query.or_else(|| cookie(&headers, COOKIE));
    let Some(left) = token.and_then(|t| state.engine.preview_access(&key, &commit, t)) else {
        return plain(
            StatusCode::FORBIDDEN,
            "this preview link expired; reopen it from board",
        );
    };
    // Set the cookie only when the token came in the URL, so later requests (assets, the mock
    // service worker) need no token.
    let set_cookie = from_query.map(|t| {
        format!(
            "{COOKIE}={t}; Path=/{key}/{commit}/; Max-Age={left}; HttpOnly; Secure; SameSite=None; Partitioned"
        )
    });
    let engine = state.engine.clone();
    let result = tokio::task::spawn_blocking(move || {
        let path = if path.is_empty() {
            "index.html".to_owned()
        } else {
            path
        };
        engine.preview_file(&key, &commit, &path)
    })
    .await;
    let file = match result {
        Ok(Ok(Some(file))) => file,
        Ok(Ok(None)) => return plain(StatusCode::NOT_FOUND, "not found"),
        Ok(Err(error)) => return ApiError::from(error).into_response(),
        Err(_) => return plain(StatusCode::INTERNAL_SERVER_ERROR, "internal error"),
    };
    let mut response = file.body.into_response();
    let out = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(&file.content_type) {
        out.insert(header::CONTENT_TYPE, value);
    }
    if let Ok(value) = HeaderValue::from_str(&file.cache_control) {
        out.insert(header::CACHE_CONTROL, value);
    }
    out.insert(header::CONTENT_SECURITY_POLICY, state.csp.clone());
    out.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    // Tokens travel in URLs; never leak them to other sites.
    out.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    if let Some(value) = set_cookie.and_then(|c| HeaderValue::from_str(&c).ok()) {
        out.insert(header::SET_COOKIE, value);
    }
    response
}

fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v)
}

fn plain(status: StatusCode, message: &'static str) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        message,
    )
        .into_response()
}
