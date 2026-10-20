//! Project routes: create / import, list, delete, checkout, push, versions, diff.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use charet_engine::{ChangeKind, Engine, EngineError, FileChange, Import};
use charet_model::{Project, Version};
use serde::{Deserialize, Serialize};

use crate::{ApiError, Caller};

/// Header carrying the commit of a checkout archive.
pub const COMMIT_HEADER: &str = "x-charet-commit";

/// Runs a blocking engine call (git, SQLite) off the async runtime.
pub(crate) async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, EngineError> + Send + 'static,
) -> Result<T, ApiError> {
    match tokio::task::spawn_blocking(work).await {
        Ok(result) => result.map_err(ApiError::from),
        Err(error) => {
            eprintln!("charet-server: task failed: {error}");
            Err(ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "Internal error",
            ))
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileInput {
    path: String,
    /// Base64 bytes. Absent means delete (push only).
    content: Option<String>,
    #[serde(default)]
    executable: bool,
}

pub(crate) fn decode(files: Vec<FileInput>) -> Result<Vec<FileChange>, ApiError> {
    files
        .into_iter()
        .map(|file| {
            let content = match file.content {
                Some(text) => Some(STANDARD.decode(text).map_err(|_| {
                    ApiError::new(
                        StatusCode::BAD_REQUEST,
                        "invalid",
                        format!("content of {:?} is not base64", file.path),
                    )
                })?),
                None => None,
            };
            Ok(FileChange {
                path: file.path,
                content,
                executable: file.executable,
            })
        })
        .collect()
}

#[derive(Serialize)]
pub struct ProjectList {
    projects: Vec<Project>,
}

pub async fn list(
    State(engine): State<Engine>,
    Caller(actor): Caller,
) -> Result<Json<ProjectList>, ApiError> {
    let projects = blocking(move || engine.projects(&actor)).await?;
    Ok(Json(ProjectList { projects }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateProject {
    key: String,
    #[serde(default)]
    title: String,
    import: Option<ImportInput>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportInput {
    pub(crate) source: String,
    /// The product repo commit the files came from.
    #[serde(default)]
    pub(crate) commit: Option<String>,
    pub(crate) files: Vec<FileInput>,
}

#[derive(Serialize)]
pub struct Created {
    project: Project,
    version: Version,
}

pub async fn create(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Json(body): Json<CreateProject>,
) -> Result<(StatusCode, Json<Created>), ApiError> {
    let import = match body.import {
        Some(import) => Some(imported(import)?),
        None => None,
    };
    let (project, version) =
        blocking(move || engine.create_project(&actor, &body.key, &body.title, import)).await?;
    Ok((StatusCode::CREATED, Json(Created { project, version })))
}

/// An import's files; every one needs content.
pub(crate) fn imported(import: ImportInput) -> Result<Import, ApiError> {
    let files = decode(import.files)?;
    if files.iter().any(|f| f.content.is_none()) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid",
            "every imported file needs content",
        ));
    }
    Ok(Import {
        files,
        source: import.source,
        commit: import.commit.filter(|c| !c.trim().is_empty()),
    })
}

pub async fn get(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(key): Path<String>,
) -> Result<Json<Project>, ApiError> {
    Ok(Json(blocking(move || engine.project(&actor, &key)).await?))
}

/// Deletes the project with its versions, captures and comments (admins).
pub async fn delete(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(key): Path<String>,
) -> Result<StatusCode, ApiError> {
    blocking(move || engine.delete_project(&actor, &key)).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct RevQuery {
    rev: Option<String>,
}

pub async fn checkout(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(key): Path<String>,
    Query(query): Query<RevQuery>,
) -> Result<Response, ApiError> {
    let checkout = blocking(move || engine.checkout(&actor, &key, query.rev.as_deref())).await?;
    let commit = HeaderValue::from_str(&checkout.commit).map_err(|_| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "bad commit id",
        )
    })?;
    Ok((
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/gzip"),
            ),
            (header::HeaderName::from_static(COMMIT_HEADER), commit),
        ],
        checkout.archive,
    )
        .into_response())
}

#[derive(Serialize)]
pub struct VersionList {
    versions: Vec<Version>,
}

pub async fn versions(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(key): Path<String>,
) -> Result<Json<VersionList>, ApiError> {
    let versions = blocking(move || engine.versions(&actor, &key)).await?;
    Ok(Json(VersionList { versions }))
}

#[derive(Deserialize)]
pub struct DiffQuery {
    from: String,
    #[serde(default = "main")]
    to: String,
}

fn main() -> String {
    "main".to_owned()
}

#[derive(Serialize)]
pub struct ChangeOut {
    path: String,
    kind: &'static str,
}

#[derive(Serialize)]
pub struct ChangeList {
    changes: Vec<ChangeOut>,
}

pub async fn diff(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(key): Path<String>,
    Query(query): Query<DiffQuery>,
) -> Result<Json<ChangeList>, ApiError> {
    let changes = blocking(move || engine.diff(&actor, &key, &query.from, &query.to)).await?;
    let changes = changes
        .into_iter()
        .map(|change| ChangeOut {
            path: change.path,
            kind: match change.kind {
                ChangeKind::Added => "added",
                ChangeKind::Modified => "modified",
                ChangeKind::Deleted => "deleted",
            },
        })
        .collect();
    Ok(Json(ChangeList { changes }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectChange {
    title: Option<String>,
    visibility: Option<charet_model::Visibility>,
}

/// `PATCH /api/projects/{key}`: rename, or set who can see it (admins).
pub async fn update(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(key): Path<String>,
    Json(change): Json<ProjectChange>,
) -> Result<Json<Project>, ApiError> {
    let project = blocking(move || {
        engine.update_project(&actor, &key, change.title.as_deref(), change.visibility)
    })
    .await?;
    Ok(Json(project))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisitorsChange {
    /// `null`: follow the server's setting.
    visitors: Option<charet_model::VisitorAccess>,
}

/// `PUT /api/projects/{key}/visitors`: what visitors may do on this project (admins).
pub async fn set_visitors(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(key): Path<String>,
    Json(change): Json<VisitorsChange>,
) -> Result<Json<Project>, ApiError> {
    let project =
        blocking(move || engine.set_project_visitors(&actor, &key, change.visitors)).await?;
    Ok(Json(project))
}

#[derive(Serialize)]
pub struct Members {
    /// User ids.
    members: Vec<String>,
}

pub async fn members(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(key): Path<String>,
) -> Result<Json<Members>, ApiError> {
    let members = blocking(move || engine.members(&actor, &key)).await?;
    Ok(Json(Members { members }))
}

pub async fn add_member(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path((key, user)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    blocking(move || engine.set_member(&actor, &key, &user, true)).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_member(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path((key, user)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    blocking(move || engine.set_member(&actor, &key, &user, false)).await?;
    Ok(StatusCode::NO_CONTENT)
}
