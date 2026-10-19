//! CLI token routes: a member's own tokens and the device login `board login` uses.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use board_engine::{DEVICE_INTERVAL_SECS, DevicePoll, Engine};
use board_model::CliToken;
use serde::{Deserialize, Serialize};

use crate::projects::blocking;
use crate::{ApiError, Caller};

#[derive(Serialize)]
pub struct Tokens {
    tokens: Vec<CliToken>,
}

pub async fn list(
    State(engine): State<Engine>,
    Caller(actor): Caller,
) -> Result<Json<Tokens>, ApiError> {
    let tokens = blocking(move || engine.cli_tokens(&actor)).await?;
    Ok(Json(Tokens { tokens }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NameInput {
    #[serde(default)]
    name: String,
}

#[derive(Serialize)]
pub struct Created {
    #[serde(flatten)]
    info: CliToken,
    /// Shown once; only its hash is kept.
    token: String,
}

pub async fn create(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Json(input): Json<NameInput>,
) -> Result<(StatusCode, Json<Created>), ApiError> {
    let created = blocking(move || engine.create_cli_token(&actor, &input.name)).await?;
    Ok((
        StatusCode::CREATED,
        Json(Created {
            info: created.info,
            token: created.token,
        }),
    ))
}

pub async fn revoke(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    blocking(move || engine.revoke_cli_token(&actor, &id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceStarted {
    device_code: String,
    user_code: String,
    /// Where to approve (`<tool>/device`); `verification_url_complete` has the code filled in.
    verification_url: String,
    verification_url_complete: String,
    interval: u64,
    expires_in: u64,
}

/// `POST /api/auth/device`: a CLI starts a login (no sign-in needed).
pub async fn start(
    State(engine): State<Engine>,
    Json(input): Json<NameInput>,
) -> Result<Json<DeviceStarted>, ApiError> {
    let base = engine.tool_url().unwrap_or_default().to_owned();
    let started = blocking(move || engine.start_device_login(&input.name)).await?;
    Ok(Json(DeviceStarted {
        verification_url: format!("{base}/device"),
        verification_url_complete: format!("{base}/device?code={}", started.user_code),
        device_code: started.device_code,
        user_code: started.user_code,
        interval: DEVICE_INTERVAL_SECS,
        expires_in: started.expires_in,
    }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PollInput {
    device_code: String,
}

#[derive(Serialize)]
pub struct Issued {
    token: String,
}

/// `POST /api/auth/device/token`: the CLI asks whether its login was approved.
pub async fn poll(
    State(engine): State<Engine>,
    Json(input): Json<PollInput>,
) -> Result<Json<Issued>, ApiError> {
    match blocking(move || engine.poll_device_login(&input.device_code)).await? {
        DevicePoll::Approved(token) => Ok(Json(Issued { token })),
        DevicePoll::Pending => Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "authorization_pending",
            "Not approved yet",
        )),
        DevicePoll::Denied => Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "access_denied",
            "The login was denied in the browser",
        )),
        DevicePoll::Expired => Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "expired_token",
            "The login expired; run board login again",
        )),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    user_code: String,
    name: String,
    status: String,
}

/// `GET /api/auth/device/{code}`: the waiting login, for the approval page.
pub async fn request(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(code): Path<String>,
) -> Result<Json<Request>, ApiError> {
    let request = blocking(move || engine.device_request(&actor, &code)).await?;
    Ok(Json(Request {
        user_code: request.user_code,
        name: request.name,
        status: request.status,
    }))
}

pub async fn approve(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(code): Path<String>,
) -> Result<StatusCode, ApiError> {
    blocking(move || engine.decide_device_login(&actor, &code, true)).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn deny(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(code): Path<String>,
) -> Result<StatusCode, ApiError> {
    blocking(move || engine.decide_device_login(&actor, &code, false)).await?;
    Ok(StatusCode::NO_CONTENT)
}
