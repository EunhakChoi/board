//! Comment routes: list, add, read, answer (reply · resolve · reopen) and the snapshot image.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use board_engine::{At, CommentFilter, Engine};
use board_model::{Comment, CommentStatus, ReplyKind};
use serde::{Deserialize, Serialize};

use crate::projects::blocking;
use crate::{ApiError, Caller};

#[derive(Deserialize)]
pub struct ListQuery {
    /// `open`, `resolved` or `all` (default `all`).
    status: Option<String>,
    kind: Option<String>,
    target: Option<String>,
}

#[derive(Serialize)]
pub struct CommentList {
    comments: Vec<Comment>,
}

pub async fn list(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(key): Path<String>,
    Query(query): Query<ListQuery>,
) -> Result<Json<CommentList>, ApiError> {
    let status = match query.status.as_deref() {
        None | Some("all") => None,
        Some("open") => Some(CommentStatus::Open),
        Some("resolved") => Some(CommentStatus::Resolved),
        Some(other) => {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid",
                format!("status must be open, resolved or all, not {other:?}"),
            ));
        }
    };
    let filter = CommentFilter {
        status,
        kind: query.kind,
        target: query.target,
    };
    let comments = blocking(move || engine.comments(&actor, &key, &filter)).await?;
    Ok(Json(CommentList { comments }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewCommentInput {
    kind: String,
    target: String,
    #[serde(default = "light")]
    theme: String,
    at: AtInput,
    body: String,
}

fn light() -> String {
    "light".into()
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum AtInput {
    Point { x: i64, y: i64 },
    Selector { selector: String },
}

pub async fn add(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(key): Path<String>,
    Json(input): Json<NewCommentInput>,
) -> Result<(StatusCode, Json<Comment>), ApiError> {
    let at = match input.at {
        AtInput::Point { x, y } => At::Point { x, y },
        AtInput::Selector { selector } => At::Selector(selector),
    };
    let comment = blocking(move || {
        engine.add_comment(
            &actor,
            &key,
            &input.kind,
            &input.target,
            &input.theme,
            &at,
            &input.body,
        )
    })
    .await?;
    Ok((StatusCode::CREATED, Json(comment)))
}

pub async fn get(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path((key, id)): Path<(String, u64)>,
) -> Result<Json<Comment>, ApiError> {
    Ok(Json(
        blocking(move || engine.comment(&actor, &key, id)).await?,
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnswerInput {
    /// The comment revision the caller saw.
    rev: u32,
    #[serde(default)]
    body: String,
}

async fn answer(
    engine: Engine,
    actor: board_model::Actor,
    key: String,
    id: u64,
    kind: ReplyKind,
    input: AnswerInput,
) -> Result<Json<Comment>, ApiError> {
    let comment =
        blocking(move || engine.answer_comment(&actor, &key, id, input.rev, kind, &input.body))
            .await?;
    Ok(Json(comment))
}

pub async fn reply(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path((key, id)): Path<(String, u64)>,
    Json(input): Json<AnswerInput>,
) -> Result<Json<Comment>, ApiError> {
    answer(engine, actor, key, id, ReplyKind::Reply, input).await
}

pub async fn resolve(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path((key, id)): Path<(String, u64)>,
    Json(input): Json<AnswerInput>,
) -> Result<Json<Comment>, ApiError> {
    answer(engine, actor, key, id, ReplyKind::Resolve, input).await
}

pub async fn reopen(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path((key, id)): Path<(String, u64)>,
    Json(input): Json<AnswerInput>,
) -> Result<Json<Comment>, ApiError> {
    answer(engine, actor, key, id, ReplyKind::Reopen, input).await
}

pub async fn snapshot(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path((key, id)): Path<(String, u64)>,
) -> Result<Response, ApiError> {
    let bytes = blocking(move || engine.comment_snapshot(&actor, &key, id)).await?;
    Ok((
        [
            (header::CONTENT_TYPE, "image/jpeg"),
            (
                header::CACHE_CONTROL,
                "private, max-age=31536000, immutable",
            ),
        ],
        bytes,
    )
        .into_response())
}
