//! Coming in without an identity provider: the setup link, invite links for new members and
//! sign-in links for another device.

use axum::extract::{Path, State};
use axum::http::header::SET_COOKIE;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use board_engine::{Engine, InviteKind, NewInviteLink, SESSION_SECS};
use board_model::{Actor, Invite, Role};
use serde::{Deserialize, Serialize};

use crate::auth::session_cookie;
use crate::projects::blocking;
use crate::{ApiError, Caller};

pub fn routes() -> Router<Engine> {
    Router::new()
        .route("/api/auth/setup", post(setup))
        .route("/api/invites/{token}", get(invite))
        .route("/api/invites/{token}/accept", post(accept))
        .route("/api/admin/invites", get(list).post(create))
        .route("/api/admin/invites/{id}", delete(revoke))
        .route("/api/admin/users/{id}/sign-in-link", post(member_link))
        .route("/api/me/sign-in-link", post(own_link))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetupInput {
    token: String,
    name: String,
}

/// `POST /api/auth/setup`: the setup link without a provider makes the first admin.
async fn setup(
    State(engine): State<Engine>,
    Json(input): Json<SetupInput>,
) -> Result<Response, ApiError> {
    let secure = secure(&engine);
    let session = blocking(move || engine.finish_setup(&input.token, &input.name)).await?;
    Ok(with_session(
        StatusCode::NO_CONTENT.into_response(),
        &session,
        secure,
    ))
}

#[derive(Serialize)]
pub struct InviteView {
    /// `join` (a new member picks a name) or `sign-in` (an existing member's account).
    kind: &'static str,
    role: Role,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

/// `GET /api/invites/{token}`: what the link does. Needs no sign-in.
async fn invite(
    State(engine): State<Engine>,
    Path(token): Path<String>,
) -> Result<Json<InviteView>, ApiError> {
    let info = blocking(move || engine.invite(&token)).await?;
    Ok(Json(InviteView {
        kind: match info.kind {
            InviteKind::Join => "join",
            InviteKind::SignIn => "sign-in",
        },
        role: info.role,
        name: info.name,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptInput {
    #[serde(default)]
    name: String,
}

/// `POST /api/invites/{token}/accept`: uses the link and sets the session cookie.
async fn accept(
    State(engine): State<Engine>,
    Path(token): Path<String>,
    Json(input): Json<AcceptInput>,
) -> Result<Response, ApiError> {
    let secure = secure(&engine);
    let session = blocking(move || engine.accept_invite(&token, &input.name)).await?;
    Ok(with_session(
        StatusCode::NO_CONTENT.into_response(),
        &session,
        secure,
    ))
}

#[derive(Serialize)]
pub struct Invites {
    invites: Vec<Invite>,
}

async fn list(
    State(engine): State<Engine>,
    Caller(actor): Caller,
) -> Result<Json<Invites>, ApiError> {
    let invites = blocking(move || engine.invites(&actor)).await?;
    Ok(Json(Invites { invites }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewInviteInput {
    role: Role,
    #[serde(default)]
    label: String,
}

#[derive(Serialize)]
pub struct Created {
    invite: Invite,
    /// The address to hand out (`<tool>/i/<token>`); shown once, only its hash is kept.
    url: String,
}

async fn create(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Json(input): Json<NewInviteInput>,
) -> Result<(StatusCode, Json<Created>), ApiError> {
    let base = base(&engine);
    let made = blocking(move || engine.create_invite(&actor, input.role, &input.label)).await?;
    Ok((StatusCode::CREATED, Json(created(&base, made))))
}

async fn revoke(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    blocking(move || engine.revoke_invite(&actor, &id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// An admin's sign-in link for a member (they lost their session, or use a new computer).
async fn member_link(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<Created>), ApiError> {
    let base = base(&engine);
    let made = blocking(move || engine.create_sign_in_link(&actor, &id)).await?;
    Ok((StatusCode::CREATED, Json(created(&base, made))))
}

/// A member's link for their own other device (15 minutes).
async fn own_link(
    State(engine): State<Engine>,
    Caller(actor): Caller,
) -> Result<(StatusCode, Json<Created>), ApiError> {
    let Actor::User { id, .. } = &actor else {
        return Err(ApiError::from(board_engine::EngineError::Forbidden));
    };
    let id = id.clone();
    let base = base(&engine);
    let made = blocking(move || engine.create_sign_in_link(&actor, &id)).await?;
    Ok((StatusCode::CREATED, Json(created(&base, made))))
}

fn created(base: &str, made: NewInviteLink) -> Created {
    Created {
        url: format!("{base}/i/{}", made.token),
        invite: made.invite,
    }
}

fn base(engine: &Engine) -> String {
    engine.tool_url().unwrap_or_default()
}

fn secure(engine: &Engine) -> bool {
    engine.tool_url().is_some_and(|u| u.starts_with("https://"))
}

fn with_session(mut response: Response, session: &str, secure: bool) -> Response {
    if let Ok(value) = HeaderValue::from_str(&session_cookie(session, secure, SESSION_SECS)) {
        response.headers_mut().insert(SET_COOKIE, value);
    }
    response
}
