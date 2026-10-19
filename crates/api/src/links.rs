//! Share link routes: admins create and revoke links; people without an account enter through
//! one under a name.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::header::SET_COOKIE;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use board_engine::{Engine, SESSION_SECS};
use board_model::{Role, ShareLink};
use serde::{Deserialize, Serialize};

use crate::auth::LocalDoor;
use crate::auth::session_cookie;
use crate::projects::blocking;
use crate::{ApiError, Caller};

#[derive(Serialize)]
pub struct Links {
    links: Vec<ShareLink>,
}

pub async fn list(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(key): Path<String>,
) -> Result<Json<Links>, ApiError> {
    let links = blocking(move || engine.share_links(&actor, &key)).await?;
    Ok(Json(Links { links }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewLinkInput {
    role: Role,
    #[serde(default)]
    label: String,
}

#[derive(Serialize)]
pub struct Created {
    link: ShareLink,
    /// Shown once; only its hash is kept.
    token: String,
    /// The address to hand out (`<tool>/l/<token>`).
    url: String,
}

pub async fn create(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(key): Path<String>,
    Json(input): Json<NewLinkInput>,
) -> Result<(StatusCode, Json<Created>), ApiError> {
    let base = engine.tool_url().unwrap_or_default();
    let created =
        blocking(move || engine.create_share_link(&actor, &key, input.role, &input.label)).await?;
    Ok((
        StatusCode::CREATED,
        Json(Created {
            url: format!("{base}/l/{}", created.token),
            link: created.link,
            token: created.token,
        }),
    ))
}

pub async fn revoke(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path((key, id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    blocking(move || engine.revoke_share_link(&actor, &key, &id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shared {
    project: String,
    project_title: String,
    role: Role,
}

/// `GET /api/links/{token}`: what the link opens. Needs no sign-in.
pub async fn shared(
    State(engine): State<Engine>,
    Path(token): Path<String>,
) -> Result<Json<Shared>, ApiError> {
    let shared = blocking(move || engine.shared_link(&token)).await?;
    Ok(Json(Shared {
        project: shared.project,
        project_title: shared.project_title,
        role: shared.role,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnterInput {
    name: String,
}

#[derive(Serialize)]
pub struct Entered {
    project: String,
}

/// `POST /api/links/{token}/enter`: lets someone in under `name` (sets the session cookie).
pub async fn enter(
    State(engine): State<Engine>,
    LocalDoor(local): LocalDoor,
    Path(token): Path<String>,
    Json(input): Json<EnterInput>,
) -> Result<Response, ApiError> {
    let secure = engine.tool_url().is_some_and(|u| u.starts_with("https://"));
    let (project, session) = blocking(move || {
        let project = engine.shared_link(&token)?.project;
        // In local mode everyone is the admin already; no guest needed.
        let session = if local {
            None
        } else {
            Some(engine.enter_share_link(&token, &input.name)?)
        };
        Ok((project, session))
    })
    .await?;
    let mut response = Json(Entered { project }).into_response();
    if let Some(value) = session
        .map(|s| session_cookie(&s, secure, SESSION_SECS))
        .and_then(|c| HeaderValue::from_str(&c).ok())
    {
        response.headers_mut().insert(SET_COOKIE, value);
    }
    Ok(response)
}
