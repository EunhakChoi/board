//! Admin routes over members, their tokens and what visitors may do.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use charet_engine::{Engine, MemberChange};
use charet_model::{Member, Role};
use serde::{Deserialize, Serialize};

use crate::projects::blocking;
use crate::{ApiError, Caller};

pub fn routes() -> Router<Engine> {
    Router::new()
        .route("/api/admin/users", get(list).post(add))
        .route(
            "/api/admin/users/{id}",
            axum::routing::patch(change).delete(remove),
        )
        .route("/api/admin/users/{id}/token", post(reissue))
        .route("/api/admin/access", get(access).put(set_access))
}

#[derive(Serialize)]
pub struct Members {
    users: Vec<Member>,
}

/// `GET /api/admin/users`: every member with their token.
async fn list(
    State(engine): State<Engine>,
    Caller(actor): Caller,
) -> Result<Json<Members>, ApiError> {
    let users = blocking(move || engine.members_list(&actor)).await?;
    Ok(Json(Members { users }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewMember {
    name: String,
    role: Role,
}

/// `POST /api/admin/users`: a new member and their token.
async fn add(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Json(input): Json<NewMember>,
) -> Result<(StatusCode, Json<Member>), ApiError> {
    let member = blocking(move || engine.add_member(&actor, &input.name, input.role)).await?;
    Ok((StatusCode::CREATED, Json(member)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    name: Option<String>,
    role: Option<Role>,
}

/// `PATCH /api/admin/users/{id}`: renames a member or changes their role.
async fn change(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(id): Path<String>,
    Json(input): Json<Change>,
) -> Result<Json<Member>, ApiError> {
    let change = MemberChange {
        name: input.name,
        role: input.role,
    };
    Ok(Json(
        blocking(move || engine.change_member(&actor, &id, &change)).await?,
    ))
}

/// `DELETE /api/admin/users/{id}`
async fn remove(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    blocking(move || engine.remove_member(&actor, &id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/admin/users/{id}/token`: a new token; the old one stops working.
async fn reissue(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(id): Path<String>,
) -> Result<Json<Member>, ApiError> {
    Ok(Json(
        blocking(move || engine.reissue_token(&actor, &id)).await?,
    ))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Access {
    /// What people without a token may do, unless a project says otherwise.
    visitors: charet_model::VisitorAccess,
}

/// `GET /api/admin/access`
async fn access(
    State(engine): State<Engine>,
    Caller(actor): Caller,
) -> Result<Json<Access>, ApiError> {
    let visitors = blocking(move || engine.visitors_setting(&actor)).await?;
    Ok(Json(Access { visitors }))
}

/// `PUT /api/admin/access`
async fn set_access(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Json(input): Json<Access>,
) -> Result<Json<Access>, ApiError> {
    let visitors = input.visitors;
    blocking(move || engine.set_server_visitors(&actor, visitors)).await?;
    Ok(Json(Access { visitors }))
}
