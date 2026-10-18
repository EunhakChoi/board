//! Domain model: projects, versions, branches, comments, anchors, screens and flows. Pure data, no I/O.

use serde::{Deserialize, Serialize};

/// Product version, reported by every app.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// What someone may do. Ordered from least to most: each role can do everything the roles before it can.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Viewer,
    Commenter,
    Editor,
    Admin,
}

/// Who is making a request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Actor {
    /// The built-in admin of local mode: the server only listens on localhost and needs no sign-in.
    Local,
    /// A signed-in member.
    User {
        id: String,
        name: String,
        email: String,
        role: Role,
    },
    /// Someone who came in through a share link. The link fixes the role and the one project.
    Guest {
        id: String,
        name: String,
        role: Role,
        project: String,
    },
}

impl Actor {
    pub fn role(&self) -> Role {
        match self {
            Self::Local => Role::Admin,
            Self::User { role, .. } | Self::Guest { role, .. } => *role,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Local => "Local admin",
            Self::User { name, .. } | Self::Guest { name, .. } => name,
        }
    }
}

/// Who can see a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    /// Everyone admitted to the server.
    Team,
    /// Only the listed members (and admins).
    Private,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_set() {
        assert_ne!(VERSION, "");
    }

    #[test]
    fn roles_are_ordered() {
        assert!(Role::Viewer < Role::Commenter);
        assert!(Role::Commenter < Role::Editor);
        assert!(Role::Editor < Role::Admin);
    }

    #[test]
    fn local_actor_is_admin() {
        assert_eq!(Actor::Local.role(), Role::Admin);
    }
}
