//! HTTP and WebSocket routes (`OpenAPI` contract).
//!
//! Every route here has an entry in `contracts/openapi.yaml`; a test keeps the two in step.

mod browse;
mod captures;
mod comments;
mod error;
mod preview;
mod projects;
mod web;

use axum::extract::DefaultBodyLimit;
use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::routing::{get, post};
use axum::{Json, Router};
use board_engine::{Engine, ServiceState};
use board_model::{Actor, Role, VERSION};
use serde::Serialize;

pub use error::ApiError;
pub use preview::{PreviewOrigin, preview_router};

/// Routes that exist, as `(method, path)`. Checked against the contract in tests.
pub const ROUTES: &[(&str, &str)] = &[
    ("get", "/api/health"),
    ("get", "/api/version"),
    ("get", "/api/system"),
    ("get", "/api/auth/me"),
    ("get", "/api/projects"),
    ("post", "/api/projects"),
    ("get", "/api/projects/{key}"),
    ("get", "/api/projects/{key}/checkout"),
    ("post", "/api/projects/{key}/push"),
    ("get", "/api/projects/{key}/versions"),
    ("get", "/api/projects/{key}/diff"),
    ("get", "/api/projects/{key}/preview"),
    ("post", "/api/projects/{key}/check"),
    ("get", "/api/projects/{key}/captures"),
    ("post", "/api/projects/{key}/captures"),
    (
        "get",
        "/api/projects/{key}/captures/{kind}/{target}/{theme}/image",
    ),
    (
        "get",
        "/api/projects/{key}/captures/{kind}/{target}/{theme}/elements",
    ),
    ("get", "/api/projects/{key}/sheet"),
    ("get", "/api/projects/{key}/comments"),
    ("post", "/api/projects/{key}/comments"),
    ("get", "/api/projects/{key}/comments/{id}"),
    ("post", "/api/projects/{key}/comments/{id}/replies"),
    ("post", "/api/projects/{key}/comments/{id}/resolve"),
    ("post", "/api/projects/{key}/comments/{id}/reopen"),
    ("get", "/api/projects/{key}/comments/{id}/snapshot"),
    ("get", "/api/projects/{key}/manifest"),
    ("get", "/api/projects/{key}/files"),
    ("get", "/api/projects/{key}/file"),
];

/// Largest request body: imports and pushes carry whole files (base64 in JSON).
const MAX_BODY: usize = 512 * 1024 * 1024;

/// The board API router; anything else is a JSON 404.
pub fn router(engine: Engine) -> Router {
    routes().fallback(not_found).with_state(engine)
}

/// The API plus the built web tool from `web_dir` (`None` = a page saying it is not built).
pub fn app(engine: Engine, web_dir: Option<std::path::PathBuf>) -> Router {
    routes()
        .fallback(move |uri: axum::http::Uri| web::serve(web_dir.clone(), uri))
        .with_state(engine)
}

fn routes() -> Router<Engine> {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/version", get(version))
        .route("/api/system", get(system))
        .route("/api/auth/me", get(me))
        .route("/api/projects", get(projects::list).post(projects::create))
        .route("/api/projects/{key}", get(projects::get))
        .route("/api/projects/{key}/checkout", get(projects::checkout))
        .route("/api/projects/{key}/push", post(projects::push))
        .route("/api/projects/{key}/versions", get(projects::versions))
        .route("/api/projects/{key}/diff", get(projects::diff))
        .route("/api/projects/{key}/preview", get(preview::status))
        .route("/api/projects/{key}/manifest", get(browse::manifest))
        .route("/api/projects/{key}/files", get(browse::files))
        .route("/api/projects/{key}/file", get(browse::file))
        .route("/api/projects/{key}/check", post(captures::check))
        .route("/api/projects/{key}/sheet", get(captures::sheet))
        .route(
            "/api/projects/{key}/captures",
            get(captures::list).post(captures::capture),
        )
        .route(
            "/api/projects/{key}/captures/{kind}/{target}/{theme}/image",
            get(captures::image),
        )
        .route(
            "/api/projects/{key}/captures/{kind}/{target}/{theme}/elements",
            get(captures::elements),
        )
        .route(
            "/api/projects/{key}/comments",
            get(comments::list).post(comments::add),
        )
        .route("/api/projects/{key}/comments/{id}", get(comments::get))
        .route(
            "/api/projects/{key}/comments/{id}/replies",
            post(comments::reply),
        )
        .route(
            "/api/projects/{key}/comments/{id}/resolve",
            post(comments::resolve),
        )
        .route(
            "/api/projects/{key}/comments/{id}/reopen",
            post(comments::reopen),
        )
        .route(
            "/api/projects/{key}/comments/{id}/snapshot",
            get(comments::snapshot),
        )
        .layer(DefaultBodyLimit::max(MAX_BODY))
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
struct SystemInfo {
    version: &'static str,
    latest: Option<String>,
    tool_url: Option<String>,
    preview_url: Option<String>,
    local_mode: bool,
    services: Services,
}

#[derive(Serialize)]
struct Services {
    workspace: Service,
    render: Service,
}

#[derive(Serialize)]
struct Service {
    ok: bool,
    /// Why it fails; admins only (it can name internal addresses).
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

async fn system(
    State(engine): State<Engine>,
    Caller(actor): Caller,
) -> Result<Json<SystemInfo>, ApiError> {
    let admin = actor.role() == Role::Admin;
    let system = projects::blocking(move || engine.system().map_err(Into::into)).await?;
    let service = |state: ServiceState| match state {
        ServiceState::Ok => Service {
            ok: true,
            error: None,
        },
        ServiceState::Failing(error) => Service {
            ok: false,
            error: admin.then_some(error),
        },
    };
    Ok(Json(SystemInfo {
        version: VERSION,
        latest: system.latest,
        tool_url: system.tool_url,
        preview_url: system.preview_url,
        local_mode: system.local_mode,
        services: Services {
            workspace: service(system.workspace),
            render: service(system.render),
        },
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
