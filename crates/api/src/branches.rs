//! Branch routes: list, start (or propose with files), push, review, merge, archive, reopen,
//! duplicate, catch up; and restoring a version.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{self, post};
use axum::{Json, Router};
use charet_engine::{BranchDetail, Engine};
use charet_model::{Branch, Version};
use serde::{Deserialize, Serialize};

use crate::projects::{FileInput, blocking, decode};
use crate::{ApiError, Caller};

/// The branch routes and version restore.
pub fn routes() -> Router<Engine> {
    Router::new()
        .route(
            "/api/projects/{key}/versions/{number}/restore",
            post(restore),
        )
        .route(
            "/api/projects/{key}/branches",
            routing::get(list).post(create),
        )
        .route("/api/projects/{key}/branches/{number}", routing::get(get))
        .route("/api/projects/{key}/branches/{number}/push", post(push))
        .route(
            "/api/projects/{key}/branches/{number}/request-review",
            post(request_review),
        )
        .route("/api/projects/{key}/branches/{number}/merge", post(merge))
        .route(
            "/api/projects/{key}/branches/{number}/archive",
            post(archive),
        )
        .route("/api/projects/{key}/branches/{number}/reopen", post(reopen))
        .route(
            "/api/projects/{key}/branches/{number}/duplicate",
            post(duplicate),
        )
        .route(
            "/api/projects/{key}/branches/{number}/catch-up",
            post(catch_up),
        )
}

#[derive(Serialize)]
pub struct BranchList {
    branches: Vec<Branch>,
}

pub async fn list(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(key): Path<String>,
) -> Result<Json<BranchList>, ApiError> {
    let branches = blocking(move || engine.branches(&actor, &key)).await?;
    Ok(Json(BranchList { branches }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewBranchInput {
    #[serde(default)]
    title: String,
    /// Where it starts (`main`, `v<n>`, a commit); `main` when absent. With `files`, the commit
    /// the changes were made on.
    from: Option<String>,
    /// Changes to push right away (a push from a `main` checkout).
    files: Option<Vec<FileInput>>,
}

/// `POST /api/projects/{key}/branches`: a new draft, empty or with changes.
pub async fn create(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(key): Path<String>,
    Json(body): Json<NewBranchInput>,
) -> Result<(StatusCode, Json<Branch>), ApiError> {
    let branch = match body.files {
        Some(files) => {
            let changes = decode(files)?;
            let base = body.from.unwrap_or_else(|| "main".to_owned());
            blocking(move || engine.propose(&actor, &key, &base, &body.title, &changes)).await?
        }
        None => {
            blocking(move || engine.create_branch(&actor, &key, &body.title, body.from.as_deref()))
                .await?
        }
    };
    Ok((StatusCode::CREATED, Json(branch)))
}

pub async fn get(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path((key, number)): Path<(String, u32)>,
) -> Result<Json<BranchDetail>, ApiError> {
    Ok(Json(
        blocking(move || engine.branch(&actor, &key, number)).await?,
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PushInput {
    base: String,
    #[serde(default)]
    message: String,
    files: Vec<FileInput>,
}

pub async fn push(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path((key, number)): Path<(String, u32)>,
    Json(body): Json<PushInput>,
) -> Result<Json<Branch>, ApiError> {
    let changes = decode(body.files)?;
    let branch = blocking(move || {
        engine.push_branch(&actor, &key, number, &body.base, &body.message, &changes)
    })
    .await?;
    Ok(Json(branch))
}

pub async fn request_review(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path((key, number)): Path<(String, u32)>,
) -> Result<Json<Branch>, ApiError> {
    Ok(Json(
        blocking(move || engine.request_review(&actor, &key, number)).await?,
    ))
}

pub async fn merge(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path((key, number)): Path<(String, u32)>,
) -> Result<(StatusCode, Json<Version>), ApiError> {
    let version = blocking(move || engine.merge_branch(&actor, &key, number)).await?;
    Ok((StatusCode::CREATED, Json(version)))
}

pub async fn archive(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path((key, number)): Path<(String, u32)>,
) -> Result<Json<Branch>, ApiError> {
    Ok(Json(
        blocking(move || engine.archive_branch(&actor, &key, number)).await?,
    ))
}

pub async fn reopen(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path((key, number)): Path<(String, u32)>,
) -> Result<Json<Branch>, ApiError> {
    Ok(Json(
        blocking(move || engine.reopen_branch(&actor, &key, number)).await?,
    ))
}

pub async fn duplicate(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path((key, number)): Path<(String, u32)>,
) -> Result<(StatusCode, Json<Branch>), ApiError> {
    let branch = blocking(move || engine.duplicate_branch(&actor, &key, number)).await?;
    Ok((StatusCode::CREATED, Json(branch)))
}

pub async fn catch_up(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path((key, number)): Path<(String, u32)>,
) -> Result<Json<Branch>, ApiError> {
    Ok(Json(
        blocking(move || engine.catch_up(&actor, &key, number)).await?,
    ))
}

/// `POST /api/projects/{key}/versions/{number}/restore`: a new version undoing that one.
pub async fn restore(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path((key, number)): Path<(String, u32)>,
) -> Result<(StatusCode, Json<Version>), ApiError> {
    let version = blocking(move || engine.restore_version(&actor, &key, number)).await?;
    Ok((StatusCode::CREATED, Json(version)))
}
