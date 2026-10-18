//! HTTP and WebSocket routes (`OpenAPI` contract).
//!
//! Every route here has an entry in `contracts/openapi.yaml`; a test keeps the two in step.

mod error;
mod preview;
mod projects;

use axum::extract::DefaultBodyLimit;
use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::routing::{get, post};
use axum::{Json, Router};
use board_engine::Engine;
use board_model::{Actor, Role, VERSION};
use serde::Serialize;

pub use error::ApiError;
pub use preview::{PreviewOrigin, preview_router};

/// Routes that exist, as `(method, path)`. Checked against the contract in tests.
pub const ROUTES: &[(&str, &str)] = &[
    ("get", "/api/health"),
    ("get", "/api/version"),
    ("get", "/api/auth/me"),
    ("get", "/api/projects"),
    ("post", "/api/projects"),
    ("get", "/api/projects/{key}"),
    ("get", "/api/projects/{key}/checkout"),
    ("post", "/api/projects/{key}/push"),
    ("get", "/api/projects/{key}/versions"),
    ("get", "/api/projects/{key}/diff"),
    ("get", "/api/projects/{key}/preview"),
];

/// Largest request body: imports and pushes carry whole files (base64 in JSON).
const MAX_BODY: usize = 512 * 1024 * 1024;

/// The board API router.
pub fn router(engine: Engine) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/version", get(version))
        .route("/api/auth/me", get(me))
        .route("/api/projects", get(projects::list).post(projects::create))
        .route("/api/projects/{key}", get(projects::get))
        .route("/api/projects/{key}/checkout", get(projects::checkout))
        .route("/api/projects/{key}/push", post(projects::push))
        .route("/api/projects/{key}/versions", get(projects::versions))
        .route("/api/projects/{key}/diff", get(projects::diff))
        .route("/api/projects/{key}/preview", get(preview::status))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .fallback(not_found)
        .with_state(engine)
}

#[derive(Serialize)]
struct Health {
    ok: bool,
}

async fn health() -> Json<Health> {
    Json(Health { ok: true })
}

#[derive(Serialize)]
struct VersionInfo {
    version: &'static str,
    latest: Option<String>,
}

async fn version(State(engine): State<Engine>) -> Result<Json<VersionInfo>, ApiError> {
    Ok(Json(VersionInfo {
        version: VERSION,
        latest: engine.latest_version()?,
    }))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Me {
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    email: Option<String>,
    role: Role,
    local_mode: bool,
}

async fn me(State(engine): State<Engine>, Caller(actor): Caller) -> Json<Me> {
    let role = actor.role();
    let name = actor.name().to_owned();
    let (kind, id, email) = match actor {
        Actor::Local => ("local", None, None),
        Actor::User { id, email, .. } => ("user", Some(id), Some(email)),
        Actor::Guest { id, .. } => ("guest", Some(id), None),
    };
    Json(Me {
        kind,
        id,
        name,
        email,
        role,
        local_mode: engine.local_mode(),
    })
}

async fn not_found() -> ApiError {
    ApiError::not_found("no such route")
}

/// Who is calling. Only local mode exists so far; sign-in arrives in P1.
struct Caller(Actor);

impl FromRequestParts<Engine> for Caller {
    type Rejection = ApiError;

    fn from_request_parts(
        _parts: &mut Parts,
        engine: &Engine,
    ) -> impl Future<Output = Result<Self, ApiError>> + Send {
        std::future::ready(if engine.local_mode() {
            Ok(Caller(Actor::Local))
        } else {
            Err(ApiError::unauthorized())
        })
    }
}
