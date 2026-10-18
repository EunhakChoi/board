use crate::{StoreError, WorkspaceError};

/// Why a command did not run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    /// The actor may not run this command here.
    Forbidden,
    /// The named thing does not exist (or is hidden from the actor).
    NotFound(&'static str),
    /// Bad input, with a message for the caller.
    Invalid(String),
    /// Something already exists under that name.
    Exists(&'static str),
    /// `main` moved since the caller's base. `head` is where it is now.
    Conflict {
        head: String,
    },
    /// A service the command needs is down or not configured.
    Unavailable(String),
    Store(StoreError),
}

impl From<WorkspaceError> for EngineError {
    fn from(error: WorkspaceError) -> Self {
        match error {
            WorkspaceError::Unavailable(message) => Self::Unavailable(message),
            WorkspaceError::Failed(message) => {
                Self::Store(StoreError(format!("workspace: {message}")))
            }
        }
    }
}

impl From<StoreError> for EngineError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Forbidden => f.write_str("not allowed"),
            Self::NotFound(what) => write!(f, "{what} not found"),
            Self::Invalid(message) => f.write_str(message),
            Self::Exists(what) => write!(f, "{what} already exists"),
            Self::Conflict { head } => write!(f, "main moved to {head}; pull and try again"),
            Self::Unavailable(message) => write!(f, "unavailable: {message}"),
            Self::Store(error) => write!(f, "storage error: {error}"),
        }
    }
}

impl std::error::Error for EngineError {}
