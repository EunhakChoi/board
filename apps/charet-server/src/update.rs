//! The new-version check: once a day, the latest GitHub release. Nothing about the server, its
//! projects or its users is sent; turn it off with `CHARET_UPDATE_CHECK=false`.

use std::time::Duration;

use charet_engine::Engine;
use charet_model::{REPOSITORY, VERSION};

const EVERY: Duration = Duration::from_hours(24);

pub fn spawn(engine: Engine) {
    tokio::spawn(async move {
        loop {
            let found = tokio::task::spawn_blocking(latest_release).await;
            if let Ok(Some(version)) = found
                && let Err(error) = engine.record_latest_version(&version)
            {
                eprintln!("charet-server: cannot record the latest version: {error}");
            }
            tokio::time::sleep(EVERY).await;
        }
    });
}

/// The latest release's version without a leading `v`; `None` when unknown (offline, no release).
fn latest_release() -> Option<String> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .new_agent();
    let mut response = agent
        .get(format!(
            "https://api.github.com/repos/{REPOSITORY}/releases/latest"
        ))
        .header("user-agent", format!("charet/{VERSION}"))
        .header("accept", "application/vnd.github+json")
        .call()
        .ok()?;
    let body = response.body_mut().read_to_string().ok()?;
    tag_version(&body)
}

fn tag_version(body: &str) -> Option<String> {
    let release: serde_json::Value = serde_json::from_str(body).ok()?;
    let tag = release.get("tag_name")?.as_str()?;
    let version = tag.strip_prefix('v').unwrap_or(tag);
    (!version.is_empty()).then(|| version.to_owned())
}

#[cfg(test)]
mod tests {
    use super::tag_version;

    #[test]
    fn reads_the_tag() {
        assert_eq!(
            tag_version(r#"{"tag_name":"v0.2.0"}"#).as_deref(),
            Some("0.2.0")
        );
        assert_eq!(
            tag_version(r#"{"tag_name":"1.0.0"}"#).as_deref(),
            Some("1.0.0")
        );
        assert_eq!(tag_version(r#"{"message":"Not Found"}"#), None);
        assert_eq!(tag_version("nope"), None);
    }
}
