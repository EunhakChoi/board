//! Commands (merge, restore, comment.add, ...) and permission checks over the model; storage, database and rendering behind traits.

pub mod policy;

use std::sync::Arc;

/// Settings and other key-value state kept by the database.
pub trait Settings: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>, StoreError>;
    fn set(&self, key: &str, value: &str) -> Result<(), StoreError>;
}

/// A storage failure, with enough text to log. Callers map it to a 500.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError(pub String);

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for StoreError {}

/// Runs commands. One instance per server, shared by every request.
#[derive(Clone)]
pub struct Engine {
    settings: Arc<dyn Settings>,
    local_mode: bool,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("local_mode", &self.local_mode)
            .finish_non_exhaustive()
    }
}

impl Engine {
    pub fn new(settings: Arc<dyn Settings>, local_mode: bool) -> Self {
        Self {
            settings,
            local_mode,
        }
    }

    /// True while the server only listens on localhost: every request acts as the built-in admin.
    pub fn local_mode(&self) -> bool {
        self.local_mode
    }

    /// The latest released version, if an update check has recorded one.
    pub fn latest_version(&self) -> Result<Option<String>, StoreError> {
        self.settings.get(LATEST_VERSION)
    }

    pub fn record_latest_version(&self, version: &str) -> Result<(), StoreError> {
        self.settings.set(LATEST_VERSION, version)
    }
}

const LATEST_VERSION: &str = "update.latest";

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Memory(Mutex<HashMap<String, String>>);

    impl Settings for Memory {
        fn get(&self, key: &str) -> Result<Option<String>, StoreError> {
            Ok(self.0.lock().unwrap().get(key).cloned())
        }
        fn set(&self, key: &str, value: &str) -> Result<(), StoreError> {
            self.0.lock().unwrap().insert(key.into(), value.into());
            Ok(())
        }
    }

    #[test]
    fn latest_version_round_trips() {
        let engine = Engine::new(Arc::new(Memory::default()), true);
        assert_eq!(engine.latest_version(), Ok(None));
        engine.record_latest_version("1.2.3").unwrap();
        assert_eq!(engine.latest_version(), Ok(Some("1.2.3".into())));
    }
}
