//! Commands (merge, restore, comment.add, ...) and permission checks over the model; storage, database and rendering behind traits.

mod browse;
mod captures;
mod comments;
mod error;
pub mod policy;
mod ports;
mod preview;
mod projects;
mod sheet;

use std::sync::Arc;

pub use captures::{CaptureStatus, RenderResult, THEMES};
pub use comments::{At, CommentFilter};
pub use error::EngineError;
pub use ports::{
    Author, Build, BuildState, BuildStep, BuiltFile, Capture, Captures, Change, ChangeKind,
    Comments, FileChange, ImageKind, NewComment, NewReply, ProjectStore, Records, RenderJob,
    Renderer, Settings, Shot, StoreError, Workspace, WorkspaceError,
};
pub use preview::{Preview, PreviewSettings};
pub use projects::{Checkout, Import};
pub use sheet::SheetItem;

/// Everything the engine needs from the outside world.
#[derive(Clone)]
pub struct Ports {
    pub settings: Arc<dyn Settings>,
    pub records: Arc<dyn Records>,
    pub store: Arc<dyn ProjectStore>,
    pub workspace: Arc<dyn Workspace>,
    pub renderer: Arc<dyn Renderer>,
    pub captures: Arc<dyn Captures>,
    pub comments: Arc<dyn Comments>,
}

impl std::fmt::Debug for Ports {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ports").finish_non_exhaustive()
    }
}

/// Runs commands. One instance per server, shared by every request.
#[derive(Clone)]
pub struct Engine {
    ports: Ports,
    local_mode: bool,
    preview: Option<Arc<PreviewSettings>>,
    tool_url: Option<Arc<str>>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("local_mode", &self.local_mode)
            .finish_non_exhaustive()
    }
}

impl Engine {
    pub fn new(ports: Ports, local_mode: bool) -> Self {
        Self {
            ports,
            local_mode,
            preview: None,
            tool_url: None,
        }
    }

    /// Enables previews: built commits are served from `settings.origin` with signed tokens.
    #[must_use]
    pub fn with_preview(mut self, settings: PreviewSettings) -> Self {
        self.preview = Some(Arc::new(settings));
        self
    }

    /// Where people open the web tool, for links (`board url`, notifications).
    #[must_use]
    pub fn with_tool_url(mut self, url: &str) -> Self {
        self.tool_url = Some(url.trim_end_matches('/').into());
        self
    }

    /// Versions, links and whether the internal services answer.
    pub fn system(&self) -> Result<System, StoreError> {
        let state = |result: Result<(), WorkspaceError>| match result {
            Ok(()) => ServiceState::Ok,
            Err(WorkspaceError::Unavailable(e) | WorkspaceError::Failed(e)) => {
                ServiceState::Failing(e)
            }
        };
        Ok(System {
            latest: self.latest_version()?,
            tool_url: self.tool_url.as_deref().map(str::to_owned),
            preview_url: self.preview.as_ref().map(|p| p.origin.clone()),
            local_mode: self.local_mode,
            workspace: state(self.ports.workspace.health()),
            render: state(self.ports.renderer.health()),
        })
    }

    /// True while the server only listens on localhost: every request acts as the built-in admin.
    pub fn local_mode(&self) -> bool {
        self.local_mode
    }

    /// The latest released version, if an update check has recorded one.
    pub fn latest_version(&self) -> Result<Option<String>, StoreError> {
        self.ports.settings.get(LATEST_VERSION)
    }

    pub fn record_latest_version(&self, version: &str) -> Result<(), StoreError> {
        self.ports.settings.set(LATEST_VERSION, version)
    }
}

const LATEST_VERSION: &str = "update.latest";

/// What `Engine::system` reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct System {
    pub latest: Option<String>,
    pub tool_url: Option<String>,
    pub preview_url: Option<String>,
    pub local_mode: bool,
    pub workspace: ServiceState,
    pub render: ServiceState,
}

/// Whether an internal service answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceState {
    Ok,
    Failing(String),
}

#[cfg(test)]
pub(crate) mod testing;

#[cfg(test)]
mod tests {
    use crate::testing::engine;

    #[test]
    fn latest_version_round_trips() {
        let engine = engine();
        assert_eq!(engine.latest_version(), Ok(None));
        engine.record_latest_version("1.2.3").unwrap();
        assert_eq!(engine.latest_version(), Ok(Some("1.2.3".into())));
    }
}
