//! Admin routes over who gets in: the allowlist, access requests and members.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use board_engine::{AccessChange, AllowRule, Engine};
use board_model::{Role, User, UserStatus};
use serde::{Deserialize, Serialize};

use crate::projects::blocking;
use crate::{ApiError, Caller};

#[derive(Serialize)]
pub struct AllowRules {
    rules: Vec<AllowRule>,
}

pub async fn allow_rules(
    State(engine): State<Engine>,
    Caller(actor): Caller,
) -> Result<Json<AllowRules>, ApiError> {
    let rules = blocking(move || engine.allow_rules(&actor)).await?;
    Ok(Json(AllowRules { rules }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewAllowRule {
    kind: String,
    value: String,
}

pub async fn add_allow_rule(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Json(input): Json<NewAllowRule>,
) -> Result<(StatusCode, Json<AllowRule>), ApiError> {
    let rule = blocking(move || engine.add_allow_rule(&actor, &input.kind, &input.value)).await?;
    Ok((StatusCode::CREATED, Json(rule)))
}

pub async fn remove_allow_rule(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    blocking(move || engine.remove_allow_rule(&actor, id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub struct Users {
    users: Vec<User>,
}

pub async fn users(
    State(engine): State<Engine>,
    Caller(actor): Caller,
) -> Result<Json<Users>, ApiError> {
    let users = blocking(move || engine.users(&actor)).await?;
    Ok(Json(Users { users }))
}

pub async fn approve(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(id): Path<String>,
) -> Result<Json<User>, ApiError> {
    Ok(Json(
        blocking(move || engine.decide_access(&actor, &id, true)).await?,
    ))
}

pub async fn reject(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(id): Path<String>,
) -> Result<Json<User>, ApiError> {
    Ok(Json(
        blocking(move || engine.decide_access(&actor, &id, false)).await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserChange {
    role: Option<Role>,
    status: Option<UserStatus>,
}

pub async fn change_user(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(id): Path<String>,
    Json(input): Json<UserChange>,
) -> Result<Json<User>, ApiError> {
    let change = AccessChange {
        role: input.role,
        status: input.status,
    };
    Ok(Json(
        blocking(move || engine.change_access(&actor, &id, change)).await?,
    ))
}
