//! Previews: the API route that starts and reports a commit's build, and the preview origin that
//! serves built files (`docs/design/store-runtime.md` §4, `security.md`).
//!
//! Previews are served by a separate listener (another host or port), or by the tool itself under
//! [`PREVIEW_PREFIX`], sandboxed, when both share one address. A request carries a token signed
//! for one (project, commit) in its path, so relative URLs (assets, lazy chunks) keep it and no
//! cookie is needed.

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use board_engine::{Build, Engine, PREVIEW_PREFIX, Preview};
use serde::{Deserialize, Serialize};

use crate::projects::blocking;
use crate::{ApiError, Caller};

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
    /// Serve every page sandboxed (an opaque origin): needed when previews share the tool's
    /// address, so project code can neither reach the tool's pages nor use its session.
    pub sandbox: bool,
}

/// What a sandboxed preview may still do: run scripts, submit forms, open dialogs and popups.
const SANDBOX: &str = "sandbox allow-scripts allow-forms allow-popups allow-modals allow-downloads allow-pointer-lock";

#[derive(Clone)]
struct PreviewState {
    engine: Engine,
    csp: HeaderValue,
}

/// Router of the preview origin. Serves nothing but built files.
pub fn preview_router(engine: Engine, origin: &PreviewOrigin) -> Router {
    preview_routes(engine, origin).fallback(|| async { plain(StatusCode::NOT_FOUND, "not found") })
}

/// The preview routes (under [`PREVIEW_PREFIX`]) without a fallback, to merge into the tool's
/// router when both share one address.
pub fn preview_routes(engine: Engine, origin: &PreviewOrigin) -> Router {
    let mut csp = format!("frame-ancestors {}", origin.frame_ancestors.join(" "));
    if origin.sandbox {
        csp = format!("{SANDBOX}; {csp}");
    }
    let state = PreviewState {
        engine,
        csp: HeaderValue::from_str(&csp)
            .unwrap_or_else(|_| HeaderValue::from_static("frame-ancestors 'none'")),
    };
    Router::new()
        .route(
            &format!("{PREVIEW_PREFIX}/{{token}}/{{key}}/{{commit}}/"),
            get(file_root),
        )
        .route(
            &format!("{PREVIEW_PREFIX}/{{token}}/{{key}}/{{commit}}/{{*path}}"),
            get(file),
        )
        .with_state(state)
}

async fn file_root(
    state: State<PreviewState>,
    Path((token, key, commit)): Path<(String, String, String)>,
) -> Response {
    serve(state, token, key, commit, String::new()).await
}

async fn file(
    state: State<PreviewState>,
    Path((token, key, commit, path)): Path<(String, String, String, String)>,
) -> Response {
    serve(state, token, key, commit, path).await
}

async fn serve(
    State(state): State<PreviewState>,
    token: String,
    key: String,
    commit: String,
    path: String,
) -> Response {
    if state.engine.preview_access(&key, &commit, &token).is_none() {
        return plain(
            StatusCode::FORBIDDEN,
            "this preview link expired; reopen it from the tool",
        );
    }
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
    // A sandboxed page has an opaque origin, so its module scripts and styles are CORS requests.
    // The token in the path is the access; anyone holding the URL may read the file anyway.
    out.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    // Tokens travel in URLs; never leak them to other sites.
    out.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
}

fn plain(status: StatusCode, message: &'static str) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        message,
    )
        .into_response()
}
