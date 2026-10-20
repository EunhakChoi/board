//! The web agent: asking it from the web tool, following its runs, and the agent door its
//! containers come in through.

use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use charet_engine::{AgentAccess, AgentSettingsChange, AgentStatus, Costs, Engine, RunInput};
use charet_model::{AgentKind, AgentMessage, AgentRun, Attachment};
use serde::Deserialize;

use crate::auth::bearer;
use crate::projects::blocking;
use crate::{ApiError, Caller, Remote};

pub fn routes() -> Router<Engine> {
    Router::new()
        .route("/api/agent", get(status))
        .route("/api/admin/agent", get(status).put(settings))
        .route("/api/admin/costs", get(costs))
        .route("/api/projects/{key}/agent-runs", get(list).post(start))
        .route(
            "/api/projects/{key}/agent-runs/{id}/messages",
            get(messages),
        )
        .route(
            "/api/projects/{key}/agent-runs/{id}/attachments/{seq}",
            get(attachment),
        )
        .route("/api/projects/{key}/agent-runs/{id}/cancel", post(cancel))
}

/// The member a run's container acts as, set by the agent door.
#[derive(Debug, Clone)]
pub struct AgentCaller(pub AgentAccess);

/// `GET /api/agent` (members), `GET /api/admin/agent`
async fn status(
    State(engine): State<Engine>,
    Caller(actor): Caller,
) -> Result<Json<AgentStatus>, ApiError> {
    Ok(Json(blocking(move || engine.agent_status(&actor)).await?))
}

/// `PUT /api/admin/agent`
async fn settings(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Json(change): Json<AgentSettingsChange>,
) -> Result<Json<AgentStatus>, ApiError> {
    Ok(Json(
        blocking(move || engine.set_agent_settings(&actor, &change)).await?,
    ))
}

/// `GET /api/admin/costs`
async fn costs(
    State(engine): State<Engine>,
    Caller(actor): Caller,
) -> Result<Json<Costs>, ApiError> {
    Ok(Json(blocking(move || engine.agent_costs(&actor)).await?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartInput {
    prompt: String,
    #[serde(default)]
    context: serde_json::Value,
    branch: Option<u32>,
    agent: Option<AgentKind>,
    model: Option<String>,
    #[serde(default)]
    images: Vec<ImageInput>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImageInput {
    media_type: String,
    /// base64
    data: String,
}

/// `POST /api/projects/{key}/agent-runs`
async fn start(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(key): Path<String>,
    Json(input): Json<StartInput>,
) -> Result<(StatusCode, Json<AgentRun>), ApiError> {
    let images = input
        .images
        .into_iter()
        .map(|image| {
            Ok(Attachment {
                data: STANDARD.decode(image.data).map_err(|_| {
                    ApiError::new(StatusCode::BAD_REQUEST, "invalid", "an image is not base64")
                })?,
                media_type: image.media_type,
            })
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    let run = RunInput {
        prompt: input.prompt,
        context: input.context,
        branch: input.branch,
        agent: input.agent,
        model: input.model,
        images,
    };
    let run = blocking(move || engine.start_agent_run(&actor, &key, run)).await?;
    Ok((StatusCode::CREATED, Json(run)))
}

#[derive(Deserialize)]
pub struct ListQuery {
    branch: Option<u32>,
}

/// `GET /api/projects/{key}/agent-runs`
async fn list(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path(key): Path<String>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<AgentRun>>, ApiError> {
    Ok(Json(
        blocking(move || engine.agent_runs(&actor, &key, query.branch)).await?,
    ))
}

#[derive(Deserialize)]
pub struct MessagesQuery {
    #[serde(default)]
    after: u32,
}

/// `GET /api/projects/{key}/agent-runs/{id}/messages`
async fn messages(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path((key, id)): Path<(String, u64)>,
    Query(query): Query<MessagesQuery>,
) -> Result<Json<Vec<AgentMessage>>, ApiError> {
    Ok(Json(
        blocking(move || engine.agent_messages(&actor, &key, id, query.after)).await?,
    ))
}

/// `GET /api/projects/{key}/agent-runs/{id}/attachments/{seq}`
async fn attachment(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path((key, id, seq)): Path<(String, u64, usize)>,
) -> Result<Response, ApiError> {
    let image = blocking(move || engine.agent_attachment(&actor, &key, id, seq)).await?;
    let kind = HeaderValue::from_str(&image.media_type)
        .unwrap_or(HeaderValue::from_static("application/octet-stream"));
    Ok((
        [
            (header::CONTENT_TYPE, kind),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, max-age=86400"),
            ),
        ],
        image.data,
    )
        .into_response())
}

/// `POST /api/projects/{key}/agent-runs/{id}/cancel`
async fn cancel(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Path((key, id)): Path<(String, u64)>,
) -> Result<Json<AgentRun>, ApiError> {
    Ok(Json(
        blocking(move || engine.cancel_agent_run(&actor, &key, id)).await?,
    ))
}

/// Lets a run's container in: its token must belong to a running run, and it reaches only what
/// the charet CLI needs to work on that run's branch.
pub(crate) async fn agent_door(
    State(engine): State<Engine>,
    mut request: Request,
    next: Next,
) -> Response {
    let Some(token) = bearer(request.headers()) else {
        return ApiError::unauthorized().into_response();
    };
    let lookup = engine.clone();
    let access = match blocking(move || lookup.agent_door(&token)).await {
        Ok(Some(access)) => access,
        Ok(None) => {
            return ApiError::new(
                StatusCode::UNAUTHORIZED,
                "invalid_token",
                "this run has ended",
            )
            .into_response();
        }
        Err(error) => return error.into_response(),
    };
    if !allowed(request.method(), request.uri().path(), &access) {
        return ApiError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "the agent may only work on its own branch",
        )
        .into_response();
    }
    request.extensions_mut().insert(Remote);
    request.extensions_mut().insert(AgentCaller(access));
    next.run(request).await
}

/// What a run's container may call.
pub(crate) fn allowed(method: &Method, path: &str, access: &AgentAccess) -> bool {
    let parts: Vec<&str> = path.trim_end_matches('/').split('/').collect();
    let get = *method == Method::GET || *method == Method::HEAD;
    match parts.as_slice() {
        ["", "api", "health" | "version" | "system"] | ["", "api", "auth", "me"] => get,
        ["", "api", "projects", key, rest @ ..] if *key == access.project => {
            let branch = access.branch.to_string();
            match rest {
                // Its own branch: read it, save to it.
                ["branches", n] => get && *n == branch,
                ["branches", n, "push"] => *method == Method::POST && *n == branch,
                // Checks and captures of what it made.
                ["check" | "captures"] => true,
                // Not the agent's business: other runs, access, handoff, restores.
                [
                    "agent-runs" | "visitors" | "members" | "import" | "handoff",
                    ..,
                ]
                | ["versions", _, ..] => false,
                _ => get,
            }
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use charet_model::Actor;

    #[test]
    fn the_agent_door_lets_a_run_touch_only_its_branch() {
        let access = AgentAccess {
            actor: Actor::Local,
            project: "shop".into(),
            branch: 3,
        };
        let ok = |m: Method, p: &str| allowed(&m, p, &access);
        assert!(ok(Method::GET, "/api/projects/shop"));
        assert!(ok(Method::GET, "/api/projects/shop/checkout"));
        assert!(ok(Method::GET, "/api/projects/shop/branches/3"));
        assert!(ok(Method::POST, "/api/projects/shop/branches/3/push"));
        assert!(ok(Method::POST, "/api/projects/shop/check"));
        assert!(ok(Method::GET, "/api/projects/shop/file"));
        assert!(ok(Method::GET, "/api/auth/me"));
        assert!(!ok(Method::POST, "/api/projects/shop/branches/4/push"));
        assert!(!ok(Method::POST, "/api/projects/shop/branches/3/merge"));
        assert!(!ok(Method::POST, "/api/projects/shop/branches"));
        assert!(!ok(Method::POST, "/api/projects/shop/comments"));
        assert!(!ok(Method::GET, "/api/projects/other"));
        assert!(!ok(Method::GET, "/api/projects"));
        assert!(!ok(Method::GET, "/api/admin/users"));
        assert!(!ok(Method::GET, "/api/ws"));
        assert!(!ok(Method::POST, "/api/projects/shop/agent-runs"));
        assert!(!ok(Method::GET, "/api/projects/shop/agent-runs"));
    }
}
