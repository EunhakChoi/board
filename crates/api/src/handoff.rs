//! Handoff routes (story 07): where the project stands against the product repo, the patch to hand
//! over, marking a version as handed off, and importing the repo again.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use charet_engine::{Engine, Export, HandoffStatus};
use charet_model::Version;
use serde::Deserialize;

use crate::projects::{ImportInput, blocking, imported};
use crate::{ApiError, Caller};

pub fn routes() -> Router<Engine> {
    Router::new()
        .route("/api/projects/{key}/handoff", get(status))
        .route("/api/projects/{key}/handoff/export", get(export))
        .route("/api/projects/{key}/versions/{number}/handoff", post(mark))
        .route("/api/projects/{key}/import", post(update))
}

pub async fn status(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(key): Path<String>,
) -> Result<Json<HandoffStatus>, ApiError> {
    Ok(Json(
        blocking(move || engine.handoff_status(&actor, &key)).await?,
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportQuery {
    since: Option<u32>,
    #[serde(default)]
    with_mocks: bool,
}

pub async fn export(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(key): Path<String>,
    Query(query): Query<ExportQuery>,
) -> Result<Json<Export>, ApiError> {
    Ok(Json(
        blocking(move || engine.export(&actor, &key, query.since, query.with_mocks)).await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkInput {
    commit: String,
}

pub async fn mark(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path((key, number)): Path<(String, u32)>,
    Json(body): Json<MarkInput>,
) -> Result<Json<Version>, ApiError> {
    Ok(Json(
        blocking(move || engine.mark_handoff(&actor, &key, number, &body.commit)).await?,
    ))
}

pub async fn update(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(key): Path<String>,
    Json(body): Json<ImportInput>,
) -> Result<(StatusCode, Json<Version>), ApiError> {
    let import = imported(body)?;
    let version = blocking(move || engine.update_import(&actor, &key, &import)).await?;
    Ok((StatusCode::CREATED, Json(version)))
}
