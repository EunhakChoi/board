//! `GET /api/ws?project=`: the realtime channel of a project room (presence and events).

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::header::{HOST, ORIGIN};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use charet_engine::{Engine, Joined, Outgoing};
use serde::Deserialize;

use crate::projects::blocking;
use crate::{ApiError, Caller};

/// Presence messages larger than this are dropped (a cursor and a short chat fit easily).
const MAX_PRESENCE: usize = 4096;

#[derive(Deserialize)]
pub struct SocketQuery {
    project: String,
    since: Option<u64>,
}

pub async fn socket(
    State(engine): State<Engine>,
    Caller(actor): Caller,
    Query(query): Query<SocketQuery>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    // Browsers send cookies with cross-site socket requests; only the tool's own pages may open one.
    if !same_origin(&headers, engine.tool_url().as_deref()) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "origin",
            "realtime connections only from the charet tool",
        ));
    }
    let project = query.project.clone();
    let joined = {
        let engine = engine.clone();
        blocking(move || engine.join_room(&actor, &query.project, query.since)).await?
    };
    Ok(upgrade.on_upgrade(move |socket| run(socket, engine, project, joined)))
}

/// The `Origin` (when sent) names the host the request went to.
/// The page's origin is the address it was served from, or the charet's own address (a proxy in
/// front of the remote door may rewrite `Host`).
fn same_origin(headers: &HeaderMap, tool_url: Option<&str>) -> bool {
    let Some(origin) = headers.get(ORIGIN).and_then(|v| v.to_str().ok()) else {
        return true;
    };
    if tool_url.is_some_and(|url| url.trim_end_matches('/') == origin.trim_end_matches('/')) {
        return true;
    }
    let host = headers
        .get(HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    origin
        .split_once("://")
        .is_some_and(|(_, rest)| rest.trim_end_matches('/') == host)
}

async fn run(mut socket: WebSocket, engine: Engine, project: String, joined: Joined) {
    let Joined {
        client,
        hello,
        mut receiver,
    } = joined;
    if socket.send(Message::Text(hello.into())).await.is_ok() {
        loop {
            tokio::select! {
                outgoing = receiver.recv() => match outgoing {
                    Some(Outgoing::Text(text)) => {
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Some(Outgoing::Close) | None => {
                        let _ = socket.send(Message::Close(None)).await;
                        break;
                    }
                },
                incoming = socket.recv() => match incoming {
                    Some(Ok(Message::Text(text))) => {
                        if text.len() <= MAX_PRESENCE
                            && let Ok(message) = serde_json::from_str::<serde_json::Value>(&text)
                            && message["type"] == "presence"
                        {
                            engine.presence(&project, client, message["data"].clone());
                        }
                    }
                    Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                    Some(Ok(_)) => {}
                },
            }
        }
    }
    engine.leave_room(&project, client);
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn origin_must_match_the_host() {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, HeaderValue::from_static("charet.test"));
        assert!(same_origin(&headers, None), "no origin: not a browser");
        headers.insert(ORIGIN, HeaderValue::from_static("https://charet.test"));
        assert!(same_origin(&headers, None));
        headers.insert(ORIGIN, HeaderValue::from_static("https://evil.test"));
        assert!(!same_origin(&headers, None));
        // Behind a proxy that rewrites Host, the charet's own address still counts.
        headers.insert(HOST, HeaderValue::from_static("127.0.0.1:5284"));
        headers.insert(ORIGIN, HeaderValue::from_static("https://b.ngrok.app"));
        assert!(same_origin(&headers, Some("https://b.ngrok.app")));
        assert!(!same_origin(&headers, Some("https://other.ngrok.app")));
    }
}
