//! Talks to `board-render` (Chromium screenshots and checks) over its internal HTTP API.

use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use board_engine::{ImageKind, RenderJob, Renderer, Shot, WorkspaceError};
use serde::{Deserialize, Serialize};
use ureq::Agent;

/// Largest render response (a full-page PNG plus the element map).
const MAX_RESPONSE: u64 = 64 * 1024 * 1024;

/// A `board-render` at `url`, authenticated with `token`.
#[derive(Clone)]
pub struct RenderClient {
    url: String,
    token: Option<String>,
    agent: Agent,
}

impl std::fmt::Debug for RenderClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderClient")
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
}

impl RenderClient {
    pub fn new(url: &str, token: Option<String>) -> Self {
        let agent = Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(3)))
            // A page may take up to a minute to navigate plus 30 s to get ready.
            .timeout_global(Some(Duration::from_secs(180)))
            .build()
            .new_agent();
        Self {
            url: url.trim_end_matches('/').to_owned(),
            token,
            agent,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WireJob<'a> {
    url: &'a str,
    width: u32,
    height: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    image: Option<&'static str>,
    map: bool,
}

#[derive(Deserialize)]
struct WireShot {
    #[serde(default)]
    ready: Option<String>,
    issues: Vec<String>,
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    elements: Option<serde_json::Value>,
}

impl Renderer for RenderClient {
    fn render(&self, job: &RenderJob) -> Result<Shot, WorkspaceError> {
        let body = WireJob {
            url: &job.url,
            width: job.width,
            height: job.height,
            image: job.image.map(|kind| match kind {
                ImageKind::Jpeg => "jpeg",
                ImageKind::Png => "png",
            }),
            map: job.map,
        };
        let mut request = self.agent.post(format!("{}/v1/render", self.url));
        if let Some(token) = &self.token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        let mut response = request
            .send_json(&body)
            .map_err(|e| WorkspaceError::Unavailable(format!("board-render: {e}")))?;
        let status = response.status().as_u16();
        let text = response
            .body_mut()
            .with_config()
            .limit(MAX_RESPONSE)
            .read_to_string()
            .map_err(|e| WorkspaceError::Unavailable(format!("board-render: {e}")))?;
        if status != 200 {
            let head: String = text.chars().take(300).collect();
            return Err(WorkspaceError::Failed(format!(
                "board-render status {status}: {head}"
            )));
        }
        let wire: WireShot = serde_json::from_str(&text)
            .map_err(|e| WorkspaceError::Failed(format!("bad render JSON: {e}")))?;
        let image = wire
            .image
            .map(|b64| STANDARD.decode(b64))
            .transpose()
            .map_err(|e| WorkspaceError::Failed(format!("bad render image: {e}")))?;
        Ok(Shot {
            ready: wire.ready,
            issues: wire.issues,
            image,
            elements: wire.elements,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_service_is_unavailable() {
        let client = RenderClient::new("http://127.0.0.1:1", None);
        let job = RenderJob {
            url: "http://x/".into(),
            width: 1,
            height: 1,
            image: None,
            map: false,
        };
        assert!(matches!(
            client.render(&job),
            Err(WorkspaceError::Unavailable(_))
        ));
    }
}
