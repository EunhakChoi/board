//! Commands (merge, restore, comment.add, ...) and permission checks over the model; storage, database and rendering behind traits.

mod error;
pub mod policy;
mod ports;
mod preview;
mod projects;

use std::sync::Arc;

pub use error::EngineError;
pub use ports::{
    Author, Build, BuildState, BuildStep, BuiltFile, Change, ChangeKind, FileChange, ProjectStore,
    Records, Settings, StoreError, Workspace, WorkspaceError,
};
pub use preview::{Preview, PreviewSettings};
pub use projects::{Checkout, Import};

/// Everything the engine needs from the outside world.
#[derive(Clone)]
pub struct Ports {
    pub settings: Arc<dyn Settings>,
    pub records: Arc<dyn Records>,
    pub store: Arc<dyn ProjectStore>,
    pub workspace: Arc<dyn Workspace>,
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
        }
    }

    /// Enables previews: built commits are served from `settings.origin` with signed tokens.
    #[must_use]
    pub fn with_preview(mut self, settings: PreviewSettings) -> Self {
        self.preview = Some(Arc::new(settings));
        self
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
