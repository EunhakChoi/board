//! Sharing a local charet from the web tool: the tunnel that runs and the address links use.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use charet_engine::{Engine, ShareMode, ShareRequest, SharingStatus};
use serde::{Deserialize, Serialize};

use crate::projects::blocking;
use crate::{ApiError, Caller};

pub fn routes() -> Router<Engine> {
    Router::new().route("/api/admin/sharing", get(status).put(start).delete(stop))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharingView {
    fixed: bool,
    mode: Option<ShareMode>,
    url: Option<String>,
    domain: Option<String>,
    modes: Vec<ModeView>,
    warning: Option<String>,
    ngrok_token: Option<String>,
}

#[derive(Serialize)]
pub struct ModeView {
    mode: ShareMode,
    available: bool,
    detail: String,
}

impl From<SharingStatus> for SharingView {
    fn from(status: SharingStatus) -> Self {
        Self {
            fixed: status.fixed,
            mode: status.sharing.as_ref().map(|s| s.mode),
            domain: status.sharing.as_ref().and_then(|s| s.domain.clone()),
            url: status.sharing.map(|s| s.url),
            modes: status
                .modes
                .into_iter()
                .map(|m| ModeView {
                    mode: m.mode,
                    available: m.available,
                    detail: m.detail,
                })
                .collect(),
            warning: status.warning,
            ngrok_token: status.ngrok_token,
        }
    }
}

/// `GET /api/admin/sharing`
async fn status(
    State(engine): State<Engine>,
    Caller(actor): Caller,
) -> Result<Json<SharingView>, ApiError> {
    Ok(Json(blocking(move || engine.sharing(&actor)).await?.into()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StartInput {
    mode: ShareMode,
    #[serde(default)]
    url: String,
    authtoken: Option<String>,
    domain: Option<String>,
}

/// `PUT /api/admin/sharing`: turns sharing on, or changes its address (admins).
async fn start(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Json(input): Json<StartInput>,
) -> Result<Json<SharingView>, ApiError> {
    let request = ShareRequest {
        mode: input.mode,
        url: input.url,
        authtoken: input.authtoken,
        domain: input.domain,
    };
    Ok(Json(
        blocking(move || engine.start_sharing(&actor, &request))
            .await?
            .into(),
    ))
}

/// `DELETE /api/admin/sharing`: turns sharing off (admins).
async fn stop(
    State(engine): State<Engine>,
    Caller(actor): Caller,
) -> Result<Json<SharingView>, ApiError> {
    Ok(Json(
        blocking(move || engine.stop_sharing(&actor)).await?.into(),
    ))
}
