//! Previews: a commit's static build, served on the preview origin (`docs/design/store-runtime.md` §4).
//!
//! The preview origin never sees sign-in cookies. Access is a short-lived token signed for one
//! (project, commit); the preview listener checks it with [`Engine::preview_access`].

use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use charet_model::Actor;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use crate::policy::Command;
use crate::{Build, BuildState, BuiltFile, Engine, EngineError};

/// How previews are reached and signed.
#[derive(Clone)]
pub struct PreviewSettings {
    /// Public origin of the preview listener, e.g. `http://127.0.0.1:5281` (no trailing slash).
    pub origin: String,
    /// The same listener as `charet-render` reaches it (differs inside docker).
    pub render_origin: String,
    /// Key for preview tokens. Kept by the server, never sent anywhere.
    pub secret: [u8; 32],
    /// How long a token is valid.
    pub ttl_secs: u64,
}

impl std::fmt::Debug for PreviewSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreviewSettings")
            .field("origin", &self.origin)
            .field("ttl_secs", &self.ttl_secs)
            .finish_non_exhaustive()
    }
}

/// A commit's preview: its build, and once ready, where to open it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    pub commit: String,
    pub build: Build,
    /// `<origin>/<project>/<commit>/` once the build is ready. Pages are under it, e.g.
    /// `.charet/frame.html?screen=<id>&t=<token>`.
    pub base_url: Option<String>,
    pub token: Option<String>,
}

impl Engine {
    /// The preview of `rev`, starting its build when there is none (or, with `retry`, when the last
    /// one failed). Callers poll until the build is ready or failed.
    pub fn preview(
        &self,
        actor: &Actor,
        key: &str,
        rev: &str,
        retry: bool,
    ) -> Result<Preview, EngineError> {
        let settings = self.preview_settings()?;
        self.visible(actor, key, Command::ProjectView)?;
        let commit = self.resolve(key, rev)?;
        let workspace = &self.ports.workspace;
        let build = match workspace.build(key, &commit)? {
            // A build made for another base (an older charet) is made again.
            Some(build)
                if build.base == BUILD_BASE && !(retry && build.state == BuildState::Failed) =>
            {
                build
            }
            _ => {
                let source = self.ports.store.archive(key, &commit)?;
                workspace.start_build(key, &commit, BUILD_BASE, source, retry)?
            }
        };
        let token = (build.state == BuildState::Ready).then(|| token(settings, key, &commit));
        Ok(Preview {
            base_url: token
                .as_ref()
                .map(|t| format!("{}{}", settings.origin, preview_path(t, key, &commit))),
            token,
            commit,
            build,
        })
    }

    /// Seconds left on `token` for (`key`, `commit`), or `None` when it is invalid or expired.
    pub fn preview_access(&self, key: &str, commit: &str, token: &str) -> Option<u64> {
        let settings = self.preview.as_deref()?;
        check(settings, key, commit, token, now())
    }

    /// A file of `commit`'s build, for the preview listener (after [`Self::preview_access`]).
    pub fn preview_file(
        &self,
        key: &str,
        commit: &str,
        path: &str,
    ) -> Result<Option<BuiltFile>, EngineError> {
        Ok(self.ports.workspace.file(key, commit, path)?)
    }

    pub(crate) fn preview_settings(&self) -> Result<&PreviewSettings, EngineError> {
        self.preview
            .as_deref()
            .ok_or_else(|| EngineError::Unavailable("previews are not configured".into()))
    }
}

/// A fresh token for (`key`, `commit`).
pub(crate) fn token(settings: &PreviewSettings, key: &str, commit: &str) -> String {
    sign(settings, key, commit, now() + settings.ttl_secs)
}

/// Where previews live on whichever origin serves them (the preview listener, or the tool itself
/// when both share one address).
pub const PREVIEW_PREFIX: &str = "/_preview";

/// Preview builds are relative, so one build serves every viewer's path.
pub const BUILD_BASE: &str = "./";

/// Where a commit's build is served for the holder of `token`. The token rides in the path:
/// a sandboxed preview sends no cookies, and relative URLs (assets, lazy chunks) keep it.
pub fn preview_path(token: &str, key: &str, commit: &str) -> String {
    format!("{PREVIEW_PREFIX}/{token}/{key}/{commit}/")
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn mac(settings: &PreviewSettings, key: &str, commit: &str, expires: u64) -> Hmac<Sha256> {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(&settings.secret).expect("HMAC takes keys of any size");
    mac.update(format!("preview\n{key}\n{commit}\n{expires}").as_bytes());
    mac
}

/// `<expires>.<signature>`.
fn sign(settings: &PreviewSettings, key: &str, commit: &str, expires: u64) -> String {
    let signature = mac(settings, key, commit, expires).finalize().into_bytes();
    format!("{expires}.{}", URL_SAFE_NO_PAD.encode(signature))
}

fn check(
    settings: &PreviewSettings,
    key: &str,
    commit: &str,
    token: &str,
    now: u64,
) -> Option<u64> {
    let (expires, signature) = token.split_once('.')?;
    let expires: u64 = expires.parse().ok()?;
    let signature = URL_SAFE_NO_PAD.decode(signature).ok()?;
    mac(settings, key, commit, expires)
        .verify_slice(&signature)
        .ok()?;
    expires.checked_sub(now).filter(|left| *left > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::engine_with_preview;
    use charet_model::Actor;

    fn settings() -> PreviewSettings {
        PreviewSettings {
            origin: "http://127.0.0.1:5281".into(),
            render_origin: "http://127.0.0.1:5281".into(),
            secret: [7; 32],
            ttl_secs: 3600,
        }
    }

    #[test]
    fn tokens_are_bound_to_project_commit_and_time() {
        let s = settings();
        let token = sign(&s, "shop", "abc", 1_000);
        assert_eq!(check(&s, "shop", "abc", &token, 400), Some(600));
        assert_eq!(check(&s, "shop", "abc", &token, 1_000), None, "expired");
        assert_eq!(check(&s, "shop", "abd", &token, 400), None, "other commit");
        assert_eq!(check(&s, "shoe", "abc", &token, 400), None, "other project");
        let forged = token.replacen("1000", "9000", 1);
        assert_eq!(
            check(&s, "shop", "abc", &forged, 400),
            None,
            "changed expiry"
        );
        let other_key = PreviewSettings {
            secret: [8; 32],
            ..settings()
        };
        assert_eq!(check(&other_key, "shop", "abc", &token, 400), None);
        assert_eq!(check(&s, "shop", "abc", "garbage", 400), None);
    }

    #[test]
    fn preview_starts_a_build_once_and_signs_when_ready() {
        let (engine, workspace) = engine_with_preview(settings());
        engine
            .create_project(&Actor::Local, "shop", "Shop", None)
            .unwrap();
        let first = engine
            .preview(&Actor::Local, "shop", "main", false)
            .unwrap();
        assert_eq!(first.build.state, BuildState::Queued);
        assert_eq!(first.base_url, None);
        assert_eq!(first.token, None);
        let again = engine
            .preview(&Actor::Local, "shop", "main", false)
            .unwrap();
        assert_eq!(again.build.state, BuildState::Queued);
        assert_eq!(workspace.starts(), 1, "no second build for the same commit");

        workspace.finish(BuildState::Ready);
        let ready = engine
            .preview(&Actor::Local, "shop", "main", false)
            .unwrap();
        let base = format!(
            "http://127.0.0.1:5281/_preview/{}/shop/{}/",
            ready.token.clone().unwrap(),
            ready.commit
        );
        assert_eq!(ready.base_url.as_deref(), Some(base.as_str()));
        let token = ready.token.unwrap();
        assert!(
            engine
                .preview_access("shop", &ready.commit, &token)
                .is_some()
        );
        assert!(engine.preview_access("shop", "0000", &token).is_none());
    }

    #[test]
    fn failed_builds_rerun_only_on_retry() {
        let (engine, workspace) = engine_with_preview(settings());
        engine
            .create_project(&Actor::Local, "shop", "Shop", None)
            .unwrap();
        engine
            .preview(&Actor::Local, "shop", "main", false)
            .unwrap();
        workspace.finish(BuildState::Failed);
        let failed = engine
            .preview(&Actor::Local, "shop", "main", false)
            .unwrap();
        assert_eq!(failed.build.state, BuildState::Failed);
        assert_eq!(workspace.starts(), 1);
        let retried = engine.preview(&Actor::Local, "shop", "main", true).unwrap();
        assert_eq!(retried.build.state, BuildState::Queued);
        assert_eq!(workspace.starts(), 2);
    }

    #[test]
    fn previews_need_settings_and_view_rights() {
        let engine = crate::testing::engine();
        engine
            .create_project(&Actor::Local, "shop", "Shop", None)
            .unwrap();
        assert!(matches!(
            engine.preview(&Actor::Local, "shop", "main", false),
            Err(EngineError::Unavailable(_))
        ));
        let (engine, _) = engine_with_preview(settings());
        assert_eq!(
            engine.preview(&Actor::Local, "nope", "main", false),
            Err(EngineError::NotFound("project"))
        );
    }
}
