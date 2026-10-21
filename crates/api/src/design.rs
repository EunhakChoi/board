//! Hand edit routes (story 05): what can change on an element, apply one edit on a draft, check
//! it (pending while the draft builds; a failing edit is taken back) and undo / redo.

use axum::Json;
use axum::extract::{Path, Query, State};
use charet_engine::{DesignInfo, Edit, EditCheck, Edited, Engine, HandEdit};
use serde::Deserialize;

use crate::projects::blocking;
use crate::{ApiError, Caller};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesignQuery {
    rev: Option<String>,
    src: Option<String>,
    #[serde(rename = "use")]
    used: Option<String>,
}

pub async fn design(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(key): Path<String>,
    Query(query): Query<DesignQuery>,
) -> Result<Json<DesignInfo>, ApiError> {
    let rev = query.rev.unwrap_or_else(|| "main".into());
    Ok(Json(
        blocking(move || {
            engine.design(
                &actor,
                &key,
                &rev,
                query.src.as_deref(),
                query.used.as_deref(),
            )
        })
        .await?,
    ))
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum EditInput {
    Text { text: String },
    ClassName { value: String },
    Prop { name: String, value: Option<String> },
    Move { by: i8 },
}

impl From<EditInput> for Edit {
    fn from(input: EditInput) -> Self {
        match input {
            EditInput::Text { text } => Edit::Text(text),
            EditInput::ClassName { value } => Edit::ClassName(value),
            EditInput::Prop { name, value } => Edit::Prop { name, value },
            EditInput::Move { by } => Edit::Move(by.signum()),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HandEditInput {
    branch: Option<u32>,
    #[serde(default)]
    title: String,
    at: String,
    edit: EditInput,
}

pub async fn edit(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(key): Path<String>,
    Json(input): Json<HandEditInput>,
) -> Result<Json<Edited>, ApiError> {
    let request = HandEdit {
        branch: input.branch,
        title: input.title,
        at: input.at,
        edit: input.edit.into(),
    };
    Ok(Json(
        blocking(move || engine.hand_edit(&actor, &key, &request)).await?,
    ))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckInput {
    branch: u32,
    path: String,
    before: String,
    commit: String,
}

pub async fn check(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(key): Path<String>,
    Json(input): Json<CheckInput>,
) -> Result<Json<EditCheck>, ApiError> {
    Ok(Json(
        blocking(move || {
            engine.check_edit(
                &actor,
                &key,
                input.branch,
                &input.path,
                &input.before,
                &input.commit,
            )
        })
        .await?,
    ))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RestoreInput {
    branch: u32,
    path: String,
    to: String,
    expect: String,
}

pub async fn restore(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(key): Path<String>,
    Json(input): Json<RestoreInput>,
) -> Result<Json<Edited>, ApiError> {
    Ok(Json(
        blocking(move || {
            engine.restore_edit(
                &actor,
                &key,
                input.branch,
                &input.path,
                &input.to,
                &input.expect,
            )
        })
        .await?,
    ))
}
