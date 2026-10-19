//! HTTP and WebSocket routes (`OpenAPI` contract).
//!
//! Every route here has an entry in `contracts/openapi.yaml`; a test keeps the two in step.

mod admin;
mod auth;
mod browse;
mod captures;
mod comments;
mod error;
mod preview;
mod projects;
mod web;

use axum::extract::DefaultBodyLimit;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use board_engine::{Engine, ServiceState, SessionState};
use board_model::{Actor, Role, VERSION};
use serde::Serialize;

pub use auth::{CLIENT_HEADER, Caller, SESSION_COOKIE};
pub use error::ApiError;
pub use preview::{PreviewOrigin, preview_router};

/// Routes that exist, as `(method, path)`. Checked against the contract in tests.
pub const ROUTES: &[(&str, &str)] = &[
    ("get", "/api/health"),
    ("get", "/api/version"),
    ("get", "/api/system"),
    ("get", "/api/auth/me"),
    ("get", "/api/auth/oidc/start"),
    ("get", "/api/auth/oidc/callback"),
    ("post", "/api/auth/logout"),
    ("get", "/api/admin/allow-rules"),
    ("post", "/api/admin/allow-rules"),
    ("delete", "/api/admin/allow-rules/{id}"),
    ("get", "/api/admin/users"),
    ("patch", "/api/admin/users/{id}"),
    ("post", "/api/admin/users/{id}/approve"),
    ("post", "/api/admin/users/{id}/reject"),
    ("get", "/api/projects"),
    ("post", "/api/projects"),
    ("get", "/api/projects/{key}"),
    ("delete", "/api/projects/{key}"),
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
        .route("/api/auth/oidc/start", get(auth::start))
        .route("/api/auth/oidc/callback", get(auth::callback))
        .route("/api/auth/logout", post(auth::logout))
        .route(
            "/api/admin/allow-rules",
            get(admin::allow_rules).post(admin::add_allow_rule),
        )
        .route(
            "/api/admin/allow-rules/{id}",
            delete(admin::remove_allow_rule),
        )
        .route("/api/admin/users", get(admin::users))
        .route("/api/admin/users/{id}", patch(admin::change_user))
        .route("/api/admin/users/{id}/approve", post(admin::approve))
        .route("/api/admin/users/{id}/reject", post(admin::reject))
        .route("/api/projects", get(projects::list).post(projects::create))
        .route(
            "/api/projects/{key}",
            get(projects::get).delete(projects::delete),
        )
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
    /// `local`, `user`, `guest`, or `pending` (signed in, waiting for an admin).
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    avatar_url: Option<String>,
    role: Role,
    local_mode: bool,
}

/// Who is calling. Unlike other routes this answers for people still waiting for approval, so
/// the tool can show them the waiting page.
async fn me(State(engine): State<Engine>, headers: HeaderMap) -> Result<Json<Me>, ApiError> {
    let local_mode = engine.local_mode();
    let state = if local_mode {
        SessionState::Active(Actor::Local)
    } else {
        let token = auth::cookie(&headers, SESSION_COOKIE).ok_or_else(ApiError::unauthorized)?;
        let engine = engine.clone();
        projects::blocking(move || engine.session(&token))
            .await?
            .ok_or_else(ApiError::unauthorized)?
    };
    let actor = match state {
        SessionState::Active(actor) => actor,
        SessionState::Waiting(user) => {
            return Ok(Json(Me {
                kind: "pending",
                id: Some(user.id),
                name: user.name,
                email: Some(user.email),
                avatar_url: user.avatar_url,
                role: user.role,
                local_mode,
            }));
        }
        SessionState::Refused(refusal) => {
            return Err(ApiError::new(
                axum::http::StatusCode::FORBIDDEN,
                refusal.code(),
                "You do not have access to this board",
            ));
        }
    };
    let role = actor.role();
    let name = actor.name().to_owned();
    let (kind, id, email) = match actor {
        Actor::Local => ("local", None, None),
        Actor::User { id, email, .. } => ("user", Some(id), Some(email)),
        Actor::Guest { id, .. } => ("guest", Some(id), None),
    };
    Ok(Json(Me {
        kind,
        id,
        name,
        email,
        avatar_url: None,
        role,
        local_mode,
    }))
}

async fn not_found() -> ApiError {
    ApiError::not_found("no such route")
}
