//! Domain model: projects, versions, branches, comments, anchors, screens and flows. Pure data, no I/O.

use serde::{Deserialize, Serialize};

/// Product version, reported by every app.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The GitHub repository releases come from.
pub const REPOSITORY: &str = "EunhakChoi/board";

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

/// A project: one UI kept in its own store (a git repository managed by the server).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    /// URL-safe name, unique on the server (`^[a-z0-9][a-z0-9-]{0,62}$`).
    pub key: String,
    pub title: String,
    pub visibility: Visibility,
    pub origin: Origin,
    pub created_at: String,
}

/// How a project started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    Empty,
    Template,
    Import,
}

impl Project {
    /// Whether `key` can name a project.
    pub fn valid_key(key: &str) -> bool {
        let bytes = key.as_bytes();
        (1..=63).contains(&bytes.len())
            && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
            && bytes
                .iter()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
    }
}

/// One recorded state of `main`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Version {
    /// 1, 2, 3, ... per project.
    pub number: u32,
    /// Git commit id of `main` at this version.
    pub commit: String,
    pub kind: VersionKind,
    /// Display name of whoever made it.
    pub actor: String,
    pub note: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VersionKind {
    /// The first state of an empty or template project.
    Initial,
    /// Brought in from the product repo.
    Import,
    /// Pushed straight to main (P0, before branches).
    Push,
    Merge,
    Restore,
}

/// A file path inside a project store. Always relative, `/`-separated, no `..`, never inside `.git`.
pub fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains('\0')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != ".." && part != ".git")
}

/// Who wrote something, as shown next to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Person {
    /// `local`, a user id, or `guest:<id>`.
    pub id: String,
    pub name: String,
}

impl Actor {
    pub fn person(&self) -> Person {
        match self {
            Self::Local => Person {
                id: "local".into(),
                name: self.name().to_owned(),
            },
            Self::User { id, name, .. } => Person {
                id: id.clone(),
                name: name.clone(),
            },
            Self::Guest { id, name, .. } => Person {
                id: format!("guest:{id}"),
                name: name.clone(),
            },
        }
    }
}

/// What a comment points at: an element of a capture (box in CSS pixels of the capture).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Anchor {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
    /// Component names, outermost first.
    #[serde(default)]
    pub chain: Vec<String>,
    #[serde(default)]
    pub sel: String,
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    pub text: String,
    /// `path:line:col` of the JSX that made the element.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src: Option<String>,
    /// The element is gone from the latest capture; the box is where it last was.
    #[serde(default)]
    pub detached: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CommentStatus {
    Open,
    Resolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReplyKind {
    Reply,
    Resolve,
    Reopen,
}

/// A comment on a screen or story, with its thread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub id: u64,
    /// `screen` or `story`.
    pub kind: String,
    pub target: String,
    /// The theme of the capture it was made on.
    pub theme: String,
    pub anchor: Anchor,
    pub body: String,
    pub author: Person,
    pub status: CommentStatus,
    /// Commit of the capture the comment was made on.
    pub created_commit: String,
    /// `main` when it was resolved.
    pub resolved_commit: Option<String>,
    /// Bumped by every reply, resolve and reopen; writers send the one they saw.
    pub rev: u32,
    pub created_at: String,
    pub updated_at: String,
    pub replies: Vec<Reply>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reply {
    pub id: u64,
    pub author: Person,
    pub body: String,
    pub kind: ReplyKind,
    pub created_at: String,
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
    fn project_keys() {
        for good in ["a", "meridian", "admin-web-2", "0x"] {
            assert!(Project::valid_key(good), "{good}");
        }
        for bad in ["", "-a", "A", "a_b", "a/b", &"a".repeat(64)] {
            assert!(!Project::valid_key(bad), "{bad}");
        }
    }

    #[test]
    fn store_paths() {
        for good in [
            "a",
            "apps/web/src/main.tsx",
            ".board/project.ts",
            ".gitignore",
        ] {
            assert!(valid_path(good), "{good}");
        }
        for bad in [
            "",
            "/etc/passwd",
            "a/../b",
            "./a",
            "a//b",
            ".git/config",
            "a/.git/x",
            "a\\b",
            "a\0",
        ] {
            assert!(!valid_path(bad), "{bad}");
        }
    }

    #[test]
    fn local_actor_is_admin() {
        assert_eq!(Actor::Local.role(), Role::Admin);
    }
}
