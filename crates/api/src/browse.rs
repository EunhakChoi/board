//! Read-only routes for the web tool: a revision's manifest, file list and file contents.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use board_engine::Engine;
use serde::{Deserialize, Serialize};

use crate::projects::{COMMIT_HEADER, blocking};
use crate::{ApiError, Caller};

#[derive(Deserialize)]
pub struct RevQuery {
    rev: Option<String>,
}

fn rev(query: RevQuery) -> String {
    query.rev.unwrap_or_else(|| "main".into())
}

#[derive(Serialize)]
pub struct ManifestOut {
    commit: String,
    manifest: serde_json::Value,
}

pub async fn manifest(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(key): Path<String>,
    Query(query): Query<RevQuery>,
) -> Result<Json<ManifestOut>, ApiError> {
    let rev = rev(query);
    let (commit, manifest) = blocking(move || engine.manifest(&actor, &key, &rev)).await?;
    Ok(Json(ManifestOut { commit, manifest }))
}

#[derive(Serialize)]
pub struct FileList {
    commit: String,
    paths: Vec<String>,
}

pub async fn files(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(key): Path<String>,
    Query(query): Query<RevQuery>,
) -> Result<Json<FileList>, ApiError> {
    let rev = rev(query);
    let (commit, paths) = blocking(move || engine.files(&actor, &key, &rev)).await?;
    Ok(Json(FileList { commit, paths }))
}

#[derive(Deserialize)]
pub struct FileQuery {
    rev: Option<String>,
    path: String,
}

pub async fn file(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(key): Path<String>,
    Query(query): Query<FileQuery>,
) -> Result<Response, ApiError> {
    let rev = query.rev.unwrap_or_else(|| "main".into());
    let path = query.path;
    let (commit, bytes) = blocking(move || engine.file(&actor, &key, &rev, &path)).await?;
    let content_type = if std::str::from_utf8(&bytes).is_ok() {
        "text/plain; charset=utf-8"
    } else {
        "application/octet-stream"
    };
    Ok((
        [
            (header::CONTENT_TYPE.as_str(), content_type.to_owned()),
            (COMMIT_HEADER, commit),
            (
                header::X_CONTENT_TYPE_OPTIONS.as_str(),
                "nosniff".to_owned(),
            ),
        ],
        bytes,
    )
        .into_response())
}
