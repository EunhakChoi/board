//! Talks to `charet-workspace` (install, verify, build) over its internal HTTP API.

use std::fmt::Write as _;
use std::io::Read;
use std::time::Duration;

use charet_engine::{Build, BuildState, BuildStep, BuiltFile, Workspace, WorkspaceError};
use serde::Deserialize;
use ureq::Agent;

/// Largest built file the preview proxies (bundles, images, fonts).
const MAX_FILE: u64 = 64 * 1024 * 1024;

/// A `charet-workspace` at `url`, authenticated with `token`.
#[derive(Clone)]
pub struct WorkspaceClient {
    url: String,
    token: Option<String>,
    agent: Agent,
}

impl std::fmt::Debug for WorkspaceClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkspaceClient")
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
}

impl WorkspaceClient {
    pub fn new(url: &str, token: Option<String>) -> Self {
        let agent = Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(3)))
            .timeout_global(Some(Duration::from_secs(120)))
            .build()
            .new_agent();
        Self {
            url: url.trim_end_matches('/').to_owned(),
            token,
            agent,
        }
    }

    fn auth(&self) -> Option<String> {
        self.token.as_ref().map(|token| format!("Bearer {token}"))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireBuild {
    #[serde(default)]
    base: String,
    state: BuildState,
    steps: Vec<WireStep>,
    error: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireStep {
    name: String,
    state: String,
    duration_ms: u64,
    detail: String,
}

impl From<WireBuild> for Build {
    fn from(wire: WireBuild) -> Self {
        Self {
            base: wire.base,
            state: wire.state,
            steps: wire
                .steps
                .into_iter()
                .map(|s| BuildStep {
                    name: s.name,
                    state: s.state,
                    duration_ms: s.duration_ms,
                    detail: s.detail,
                })
                .collect(),
            error: wire.error,
        }
    }
}

fn unavailable(error: &ureq::Error) -> WorkspaceError {
    WorkspaceError::Unavailable(format!("charet-workspace: {error}"))
}

fn unexpected(status: u16, body: &str) -> WorkspaceError {
    WorkspaceError::Failed(format!(
        "status {status}: {}",
        body.chars().take(300).collect::<String>()
    ))
}

fn parse(body: &str) -> Result<Build, WorkspaceError> {
    serde_json::from_str::<WireBuild>(body)
        .map(Build::from)
        .map_err(|e| WorkspaceError::Failed(format!("bad build JSON: {e}")))
}

/// Percent-encodes one path segment's worth of a file path, keeping `/`.
fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

impl Workspace for WorkspaceClient {
    fn health(&self) -> Result<(), WorkspaceError> {
        let mut response = self
            .agent
            .get(format!("{}/health", self.url))
            .call()
            .map_err(|e| unavailable(&e))?;
        match response.status().as_u16() {
            200 => Ok(()),
            status => Err(unexpected(
                status,
                &response.body_mut().read_to_string().unwrap_or_default(),
            )),
        }
    }

    fn build(&self, project: &str, commit: &str) -> Result<Option<Build>, WorkspaceError> {
        let mut request = self
            .agent
            .get(format!("{}/v1/builds/{project}/{commit}", self.url));
        if let Some(auth) = self.auth() {
            request = request.header("authorization", auth);
        }
        let mut response = request.call().map_err(|e| unavailable(&e))?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|e| unavailable(&e))?;
        match status {
            200 => parse(&body).map(Some),
            404 => Ok(None),
            _ => Err(unexpected(status, &body)),
        }
    }

    fn start_build(
        &self,
        project: &str,
        commit: &str,
        base: &str,
        source: Vec<u8>,
        retry: bool,
    ) -> Result<Build, WorkspaceError> {
        let mut request = self
            .agent
            .put(format!("{}/v1/builds/{project}/{commit}", self.url))
            .query("base", base)
            .header("content-type", "application/gzip");
        if retry {
            request = request.query("retry", "1");
        }
        if let Some(auth) = self.auth() {
            request = request.header("authorization", auth);
        }
        let mut response = request.send(&source[..]).map_err(|e| unavailable(&e))?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|e| unavailable(&e))?;
        match status {
            200 | 202 => parse(&body),
            _ => Err(unexpected(status, &body)),
        }
    }

    fn file(
        &self,
        project: &str,
        commit: &str,
        path: &str,
    ) -> Result<Option<BuiltFile>, WorkspaceError> {
        let url = format!(
            "{}/v1/files/{project}/{commit}/{}",
            self.url,
            encode_path(path)
        );
        let mut request = self.agent.get(url);
        if let Some(auth) = self.auth() {
            request = request.header("authorization", auth);
        }
        let mut response = request.call().map_err(|e| unavailable(&e))?;
        let status = response.status().as_u16();
        if status == 404 || status == 400 {
            return Ok(None);
        }
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_owned()
        };
        let content_type = header("content-type");
        let cache_control = header("cache-control");
        let mut body = Vec::new();
        response
            .body_mut()
            .with_config()
            .limit(MAX_FILE)
            .reader()
            .read_to_end(&mut body)
            .map_err(|e| WorkspaceError::Failed(e.to_string()))?;
        if status != 200 {
            return Err(unexpected(status, &String::from_utf8_lossy(&body)));
        }
        Ok(Some(BuiltFile {
            content_type,
            cache_control,
            body,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_file_paths() {
        assert_eq!(encode_path(".charet/frame.html"), ".charet/frame.html");
        assert_eq!(encode_path("a b/ü?.js"), "a%20b/%C3%BC%3F.js");
    }

    #[test]
    fn parses_builds() {
        let build = parse(
            r#"{"project":"p","commit":"c","state":"failed","error":"vite build failed",
               "steps":[{"name":"install","state":"ok","durationMs":12,"detail":""}]}"#,
        )
        .unwrap();
        assert_eq!(build.state, BuildState::Failed);
        assert_eq!(build.steps[0].duration_ms, 12);
        assert!(parse("{}").is_err());
    }

    #[test]
    fn a_missing_service_is_unavailable() {
        let client = WorkspaceClient::new("http://127.0.0.1:1", None);
        assert!(matches!(
            client.build("p", "c"),
            Err(WorkspaceError::Unavailable(_))
        ));
    }
}
