//! HTTP and WebSocket routes (`OpenAPI` contract).
//!
//! Every route here has an entry in `contracts/openapi.yaml`; a test keeps the two in step.

mod admin;
mod auth;
mod browse;
mod captures;
mod comments;
mod error;
mod invites;
mod links;
mod notices;
mod preview;
mod projects;
mod sharing;
mod socket;
mod tokens;
mod web;

use axum::extract::DefaultBodyLimit;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use board_engine::{Engine, ServiceState, SessionState};
use board_model::{Actor, Role, VERSION};
use serde::Serialize;

pub use auth::{CLIENT_HEADER, Caller, LocalDoor, Remote, SESSION_COOKIE};
pub use error::ApiError;
pub use preview::{PreviewOrigin, preview_router};

/// Routes that exist, as `(method, path)`. Checked against the contract in tests.
pub const ROUTES: &[(&str, &str)] = &[
    ("get", "/api/health"),
    ("get", "/api/version"),
    ("get", "/api/system"),
    ("get", "/api/ws"),
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
    ("post", "/api/auth/setup"),
    ("get", "/api/invites/{token}"),
    ("post", "/api/invites/{token}/accept"),
    ("get", "/api/admin/invites"),
    ("post", "/api/admin/invites"),
    ("delete", "/api/admin/invites/{id}"),
    ("post", "/api/admin/users/{id}/sign-in-link"),
    ("post", "/api/me/sign-in-link"),
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
    ("get", "/api/me/cli-tokens"),
    ("post", "/api/me/cli-tokens"),
    ("delete", "/api/me/cli-tokens/{id}"),
    ("post", "/api/auth/device"),
    ("post", "/api/auth/device/token"),
    ("get", "/api/auth/device/{code}"),
    ("post", "/api/auth/device/{code}/approve"),
    ("post", "/api/auth/device/{code}/deny"),
    ("get", "/api/links/{token}"),
    ("post", "/api/links/{token}/enter"),
    ("get", "/api/projects/{key}/share-links"),
    ("post", "/api/projects/{key}/share-links"),
    ("post", "/api/projects/{key}/share-links/{id}/revoke"),
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

/// The remote door of a local board: what a tunnel or reverse proxy points at. Everyone signs
/// in here, previews are served sandboxed under the same address, and while sharing is off it
/// answers nothing.
pub fn remote_app(engine: Engine, web_dir: Option<std::path::PathBuf>) -> Router {
    let origin = PreviewOrigin {
        frame_ancestors: vec!["'self'".to_owned()],
        sandbox: true,
    };
    routes()
        .fallback(move |uri: axum::http::Uri| web::serve(web_dir.clone(), uri))
        .with_state(engine.clone())
        .merge(preview::preview_routes(engine.clone(), &origin))
        .layer(axum::middleware::from_fn(
            move |mut request: axum::extract::Request, next: axum::middleware::Next| {
                let open = engine.remote_open();
                async move {
                    if !open {
                        return ApiError::new(
                            axum::http::StatusCode::SERVICE_UNAVAILABLE,
                            "sharing_off",
                            "This board is not shared right now",
                        )
                        .into_response();
                    }
                    request.extensions_mut().insert(Remote);
                    next.run(request).await
                }
            },
        ))
}

fn routes() -> Router<Engine> {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/version", get(version))
        .route("/api/system", get(system))
        .route("/api/ws", get(socket::socket))
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
        .route("/api/me/cli-tokens", get(tokens::list).post(tokens::create))
        .route("/api/me/cli-tokens/{id}", delete(tokens::revoke))
        .route("/api/auth/device", post(tokens::start))
        .route("/api/auth/device/token", post(tokens::poll))
        .route("/api/auth/device/{code}", get(tokens::request))
        .route("/api/auth/device/{code}/approve", post(tokens::approve))
        .route("/api/auth/device/{code}/deny", post(tokens::deny))
        .route("/api/links/{token}", get(links::shared))
        .route("/api/links/{token}/enter", post(links::enter))
        .route(
            "/api/projects/{key}/share-links",
            get(links::list).post(links::create),
        )
        .route(
            "/api/projects/{key}/share-links/{id}/revoke",
            post(links::revoke),
        )
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
        .merge(invites::routes())
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
    sign_in: Option<SignInInfo>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SignInInfo {
    /// `None`: no provider; people come in through invite links.
    provider: Option<String>,
    /// Register this at the provider as the redirect URI.
    callback_url: Option<String>,
    /// Whether the provider's discovery document loads.
    #[serde(flatten)]
    state: Service,
    has_admin: bool,
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

/// Answers before sign-in too (the sign-in page and `board login` need `localMode`); only
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
        tool_url: system.tool_url,
        preview_url: system.preview_url,
        local_mode: local,
        services: Services {
            workspace: service(system.workspace),
            render: service(system.render),
        },
        sign_in: system.sign_in.map(|sign_in| SignInInfo {
            provider: sign_in.provider,
            callback_url: sign_in.callback_url,
            state: service(sign_in.state),
            has_admin: sign_in.has_admin,
        }),
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
    /// The one project a guest's share link opens.
    #[serde(skip_serializing_if = "Option::is_none")]
    project: Option<String>,
    local_mode: bool,
}

/// Who is calling. Unlike other routes this answers for people still waiting for approval, so
/// the tool can show them the waiting page.
async fn me(
    State(engine): State<Engine>,
    LocalDoor(local_mode): LocalDoor,
    headers: HeaderMap,
) -> Result<Json<Me>, ApiError> {
    let state = if local_mode {
        SessionState::Active(engine.local_actor())
    } else if let Some(token) = auth::bearer(&headers) {
        let engine = engine.clone();
        let actor = projects::blocking(move || engine.token_actor(&token))
            .await?
            .ok_or_else(ApiError::unauthorized)?;
        SessionState::Active(actor)
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
                project: None,
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
    let (kind, id, email, project) = match actor {
        Actor::Local => ("local", None, None, None),
        // Members who came in through an invite have no address.
        Actor::User { id, email, .. } => (
            "user",
            Some(id),
            Some(email).filter(|e| !e.is_empty()),
            None,
        ),
        Actor::Guest { id, project, .. } => ("guest", Some(id), None, Some(project)),
    };
    Ok(Json(Me {
        kind,
        id,
        name,
        email,
        avatar_url: None,
        role,
        project,
        local_mode,
    }))
}

async fn not_found() -> ApiError {
    ApiError::not_found("no such route")
}
