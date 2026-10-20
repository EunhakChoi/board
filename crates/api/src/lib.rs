//! HTTP and WebSocket routes (`OpenAPI` contract).
//!
//! Every route here has an entry in `contracts/openapi.yaml`; a test keeps the two in step.

mod auth;
mod branches;
mod browse;
mod captures;
mod comments;
mod error;
mod members;
mod notices;
mod preview;
mod projects;
mod sharing;
mod socket;
mod web;

use axum::extract::DefaultBodyLimit;
use axum::extract::State;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use charet_engine::{Engine, ServiceState};
use charet_model::{Role, VERSION};
use serde::Serialize;

pub use auth::{CLIENT_HEADER, Caller, LocalDoor, Remote, SESSION_COOKIE, VISITOR_COOKIE};
pub use error::ApiError;
pub use preview::{PreviewOrigin, preview_router};

/// Routes that exist, as `(method, path)`. Checked against the contract in tests.
pub const ROUTES: &[(&str, &str)] = &[
    ("get", "/api/health"),
    ("get", "/api/version"),
    ("get", "/api/system"),
    ("get", "/api/ws"),
    ("get", "/api/auth/me"),
    ("post", "/api/auth/token"),
    ("post", "/api/auth/logout"),
    ("get", "/api/admin/users"),
    ("post", "/api/admin/users"),
    ("patch", "/api/admin/users/{id}"),
    ("delete", "/api/admin/users/{id}"),
    ("post", "/api/admin/users/{id}/token"),
    ("get", "/api/admin/access"),
    ("put", "/api/admin/access"),
    ("get", "/api/admin/sharing"),
    ("put", "/api/admin/sharing"),
    ("delete", "/api/admin/sharing"),
    ("get", "/api/me/notifications"),
    ("post", "/api/me/notifications/read"),
    ("get", "/api/admin/webhooks"),
    ("post", "/api/admin/webhooks"),
    ("delete", "/api/admin/webhooks/{id}"),
    ("post", "/api/admin/webhooks/{id}/rotate"),
    ("post", "/api/admin/webhooks/{id}/test"),
    ("get", "/api/projects"),
    ("post", "/api/projects"),
    ("get", "/api/projects/{key}"),
    ("delete", "/api/projects/{key}"),
    ("patch", "/api/projects/{key}"),
    ("get", "/api/projects/{key}/members"),
    ("put", "/api/projects/{key}/members/{user}"),
    ("delete", "/api/projects/{key}/members/{user}"),
    ("get", "/api/projects/{key}/checkout"),
    ("post", "/api/projects/{key}/push"),
    ("get", "/api/projects/{key}/versions"),
    ("post", "/api/projects/{key}/versions/{number}/restore"),
    ("get", "/api/projects/{key}/branches"),
    ("post", "/api/projects/{key}/branches"),
    ("get", "/api/projects/{key}/branches/{number}"),
    ("post", "/api/projects/{key}/branches/{number}/push"),
    (
        "post",
        "/api/projects/{key}/branches/{number}/request-review",
    ),
    ("post", "/api/projects/{key}/branches/{number}/merge"),
    ("post", "/api/projects/{key}/branches/{number}/approve"),
    ("post", "/api/projects/{key}/branches/{number}/archive"),
    ("post", "/api/projects/{key}/branches/{number}/reopen"),
    ("post", "/api/projects/{key}/branches/{number}/duplicate"),
    ("post", "/api/projects/{key}/branches/{number}/catch-up"),
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

/// The charet API router; anything else is a JSON 404.
pub fn router(engine: Engine) -> Router {
    routes().fallback(not_found).with_state(engine)
}

/// The API plus the built web tool from `web_dir` (`None` = a page saying it is not built).
/// When previews share the tool's address, they are served here too, sandboxed.
pub fn app(engine: Engine, web_dir: Option<std::path::PathBuf>) -> Router {
    let shared = engine.previews_on_tool();
    let app = routes()
        .fallback(move |uri: axum::http::Uri| web::serve(web_dir.clone(), uri))
        .with_state(engine.clone());
    if !shared {
        return app;
    }
    let origin = PreviewOrigin {
        frame_ancestors: vec!["'self'".to_owned()],
        sandbox: true,
    };
    app.merge(preview::preview_routes(engine, &origin))
}

/// The remote door of a local charet: what a tunnel or reverse proxy points at. Nobody is the
/// owner here (members bring their token), and previews are served sandboxed under the same
/// address.
pub fn remote_app(engine: Engine, web_dir: Option<std::path::PathBuf>) -> Router {
    let origin = PreviewOrigin {
        frame_ancestors: vec!["'self'".to_owned()],
        sandbox: true,
    };
    routes()
        .fallback(move |uri: axum::http::Uri| web::serve(web_dir.clone(), uri))
        .with_state(engine.clone())
        .merge(preview::preview_routes(engine, &origin))
        .layer(axum::middleware::from_fn(
            |mut request: axum::extract::Request, next: axum::middleware::Next| async move {
                request.extensions_mut().insert(Remote);
                next.run(request).await
            },
        ))
}

fn routes() -> Router<Engine> {
    Router::new()
        .merge(branches::routes())
        .route("/api/health", get(health))
        .route("/api/version", get(version))
        .route("/api/system", get(system))
        .route("/api/ws", get(socket::socket))
        .route("/api/auth/me", get(auth::me))
        .route("/api/auth/token", post(auth::sign_in))
        .route("/api/auth/logout", post(auth::logout))
        .route("/api/projects", get(projects::list).post(projects::create))
        .route(
            "/api/projects/{key}",
            get(projects::get)
                .delete(projects::delete)
                .patch(projects::update),
        )
        .route("/api/projects/{key}/members", get(projects::members))
        .route(
            "/api/projects/{key}/members/{user}",
            put(projects::add_member).delete(projects::remove_member),
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
        .merge(notices::routes())
        .merge(members::routes())
        .merge(sharing::routes())
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

/// Answers without a token too (the token page and `charet login` need `localMode`); only
/// admins see why a service fails.
async fn system(
    State(engine): State<Engine>,
    LocalDoor(local): LocalDoor,
    caller: Option<Caller>,
) -> Result<Json<SystemInfo>, ApiError> {
    let admin = caller.is_some_and(|Caller(actor)| actor.role() == Role::Admin);
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
        tool_url: system.tool_url.clone(),
        // Through the remote door of a local charet, previews come from the shared address.
        preview_url: if !local && system.local_mode {
            system.tool_url.clone()
        } else {
            system.preview_url
        },
        local_mode: local,
        services: Services {
            workspace: service(system.workspace),
            render: service(system.render),
        },
    }))
}

async fn not_found() -> ApiError {
    ApiError::not_found("no such route")
}
