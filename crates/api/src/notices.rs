//! The inbox (`/api/me/notifications`) and webhook settings (`/api/admin/webhooks`).

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use board_engine::{Engine, WEBHOOK_EVENTS};
use board_model::{Notification, Webhook};
use serde::{Deserialize, Serialize};

use crate::projects::blocking;
use crate::{ApiError, Caller};

pub fn routes() -> Router<Engine> {
    Router::new()
        .route("/api/me/notifications", get(inbox))
        .route("/api/me/notifications/read", post(read))
        .route("/api/admin/webhooks", get(webhooks).post(create_webhook))
        .route("/api/admin/webhooks/{id}", delete(delete_webhook))
        .route("/api/admin/webhooks/{id}/rotate", post(rotate_webhook))
        .route("/api/admin/webhooks/{id}/test", post(test_webhook))
}

#[derive(Deserialize)]
pub struct InboxQuery {
    #[serde(default)]
    unread: bool,
    limit: Option<u32>,
}

#[derive(Serialize)]
pub struct Inbox {
    notifications: Vec<Notification>,
    unread: u32,
}

pub async fn inbox(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Query(query): Query<InboxQuery>,
) -> Result<Json<Inbox>, ApiError> {
    let inbox =
        blocking(move || engine.inbox(&actor, query.unread, query.limit.unwrap_or(50))).await?;
    Ok(Json(Inbox {
        notifications: inbox.notifications,
        unread: inbox.unread,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadInput {
    /// These notifications; leave out to mark every one read.
    ids: Option<Vec<u64>>,
}

#[derive(Serialize)]
pub struct Unread {
    unread: u32,
}

pub async fn read(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Json(input): Json<ReadInput>,
) -> Result<Json<Unread>, ApiError> {
    let unread = blocking(move || engine.mark_read(&actor, input.ids.as_deref())).await?;
    Ok(Json(Unread { unread }))
}

#[derive(Serialize)]
pub struct Webhooks {
    webhooks: Vec<Webhook>,
    /// Every event a webhook can choose.
    events: &'static [&'static str],
}

pub async fn webhooks(
    State(engine): State<Engine>,
    Caller(actor): Caller,
) -> Result<Json<Webhooks>, ApiError> {
    let webhooks = blocking(move || engine.webhooks(&actor)).await?;
    Ok(Json(Webhooks {
        webhooks,
        events: WEBHOOK_EVENTS,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebhookInput {
    url: String,
    #[serde(default)]
    events: Vec<String>,
    project: Option<String>,
}

#[derive(Serialize)]
pub struct WithSecret {
    webhook: Webhook,
    /// Shown once: the key receivers check `X-Board-Signature` with.
    secret: String,
}

pub async fn create_webhook(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Json(input): Json<WebhookInput>,
) -> Result<(StatusCode, Json<WithSecret>), ApiError> {
    let created = blocking(move || {
        engine.create_webhook(&actor, &input.url, &input.events, input.project.as_deref())
    })
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(WithSecret {
            webhook: created.webhook,
            secret: created.secret,
        }),
    ))
}

pub async fn delete_webhook(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    blocking(move || engine.delete_webhook(&actor, &id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn rotate_webhook(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(id): Path<String>,
) -> Result<Json<WithSecret>, ApiError> {
    let rotated = blocking(move || engine.rotate_webhook_secret(&actor, &id)).await?;
    Ok(Json(WithSecret {
        webhook: rotated.webhook,
        secret: rotated.secret,
    }))
}

/// Sends a `ping` now and answers with the webhook, its outcome recorded.
pub async fn test_webhook(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(id): Path<String>,
) -> Result<Json<Webhook>, ApiError> {
    Ok(Json(
        blocking(move || engine.test_webhook(&actor, &id)).await?,
    ))
}
