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
    /// The line (`main` or a branch) moved since the caller's base. `head` is where it is now.
    Conflict {
        head: String,
    },
    /// Joining two lines of work clashed in these files; someone has to settle them by hand.
    Clash(Vec<String>),
    /// A service the command needs is down or not configured.
    Unavailable(String),
    /// Not possible yet (a build still running); try again later.
    Pending(String),
    /// Someone wrote to it since the caller read it; this is its revision now.
    Changed(u32),
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
            Self::Invalid(message) | Self::Pending(message) => f.write_str(message),
            Self::Exists(what) => write!(f, "{what} already exists"),
            Self::Conflict { head } => write!(f, "it moved to {head}; pull and try again"),
            Self::Clash(files) => write!(f, "changed on both sides: {}", files.join(", ")),
            Self::Unavailable(message) => write!(f, "unavailable: {message}"),
            Self::Changed(rev) => write!(
                f,
                "changed meanwhile (now at revision {rev}); read it again"
            ),
            Self::Store(error) => write!(f, "storage error: {error}"),
        }
    }
}

impl std::error::Error for EngineError {}
