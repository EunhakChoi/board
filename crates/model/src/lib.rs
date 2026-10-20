//! Domain model: projects, versions, branches, comments, anchors, screens and flows. Pure data, no I/O.

use serde::{Deserialize, Serialize};

/// Product version, reported by every app.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The GitHub repository releases come from.
pub const REPOSITORY: &str = "EunhakChoi/charet";

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
    /// The built-in admin before the charet has an owner (tests, first start).
    Local,
    /// A member: came in with their token (or is the owner at this machine).
    User {
        id: String,
        name: String,
        role: Role,
    },
    /// Someone without a token, when the charet lets visitors look. Always a viewer.
    Visitor { id: String, name: String },
}

impl Actor {
    pub fn role(&self) -> Role {
        match self {
            Self::Local => Role::Admin,
            Self::User { role, .. } => *role,
            Self::Visitor { .. } => Role::Viewer,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Local => "Local admin",
            Self::User { name, .. } | Self::Visitor { name, .. } => name,
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

/// What people without a token may do: on the whole server, or on one team project.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VisitorAccess {
    /// Nothing: a token is needed.
    None,
    /// Look: screens, flows, code view, comments.
    #[default]
    View,
    /// Look and comment under their made-up name.
    Comment,
}

/// A project: one UI kept in its own store (a git repository managed by the server).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    /// URL-safe name, unique on the server (`^[a-z0-9][a-z0-9-]{0,62}$`).
    pub key: String,
    pub title: String,
    pub visibility: Visibility,
    /// What visitors may do here; `None` follows the server's setting.
    #[serde(default)]
    pub visitors: Option<VisitorAccess>,
    /// What visitors may do here in effect (this project's setting, else the server's; nothing
    /// while private). Filled in by the engine, not stored.
    #[serde(default, skip_deserializing)]
    pub visitor_access: VisitorAccess,
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
    /// The branch a `merge` version took in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<u32>,
    /// The version a `restore` version undid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restored: Option<u32>,
    /// For an `import` version: the product repo commit its files came from, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_commit: Option<String>,
    /// For an `import` joined with work not handed off yet: the store commit with exactly the
    /// repo's files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_tree: Option<String>,
    /// Set when an admin marked this version (and every one before it) as in the product repo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff: Option<Handoff>,
    pub created_at: String,
}

/// A version marked as in the product repo, at `commit` there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Handoff {
    /// The product repo commit that has it.
    pub commit: String,
    pub actor: String,
    pub at: String,
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

/// A line of work on top of `main`: a draft while its author works (`open`), then a change the team
/// reviews (`in_review`), until it is merged into a version or archived.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Branch {
    /// 1, 2, 3, ... per project (git ref `b/<number>`).
    pub number: u32,
    pub title: String,
    pub author: Person,
    pub status: BranchStatus,
    /// The `main` commit it last started from or caught up with.
    pub base: String,
    /// Its newest commit.
    pub head: String,
    /// Files that clashed when catching up with `main` or merging; empty when clean.
    pub conflicts: Vec<String>,
    /// The version it became, once merged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_version: Option<u32>,
    /// The commit an admin approved its protected-file changes at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved: Option<String>,
    /// The last check of what merging it would give.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<MergeCheck>,
    pub created_at: String,
    pub updated_at: String,
}

/// A check of the commit merging a branch would make: it is built and the screens the change can
/// reach are opened. New problems hold the merge back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeCheck {
    /// The `main` and branch commits that were joined.
    pub main: String,
    pub head: String,
    /// The joined commit (on no line until the merge).
    pub commit: String,
    pub state: MergeCheckState,
    /// What failed: the build error, or `<target> (<theme>): <problem>` lines.
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeCheckState {
    Checking,
    Passed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BranchStatus {
    Open,
    InReview,
    Merged,
    Archived,
}

impl BranchStatus {
    /// Still being worked on (open or in review).
    pub fn active(self) -> bool {
        matches!(self, Self::Open | Self::InReview)
    }
}

impl Branch {
    /// The store line of branch `number`.
    pub fn line(number: u32) -> String {
        format!("b/{number}")
    }
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
    /// `local`, a user id, or `visitor:<id>`.
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
            Self::Visitor { id, name } => Person {
                id: format!("visitor:{id}"),
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
    /// Made on this branch's screen (a live preview); absent = on main.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<u32>,
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

/// Which coding agent runs a web agent run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    /// Claude Agent SDK (Anthropic API key).
    Claude,
    /// Codex SDK (an `OpenAI` API key).
    Codex,
    /// A scripted stand-in for tests: makes a fixed change, no model and no key.
    Script,
}

/// Where a web agent run is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunState {
    /// Waiting for an earlier run on the same branch (or a free slot).
    Queued,
    Running,
    Done,
    Failed,
    Canceled,
}

impl RunState {
    pub fn finished(self) -> bool {
        matches!(self, Self::Done | Self::Failed | Self::Canceled)
    }
}

/// One instruction to the web agent and what came of it. Its changes land on `branch` as pushes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRun {
    pub id: u64,
    pub project: String,
    pub branch: u32,
    pub actor: Person,
    pub agent: AgentKind,
    /// The model asked for; empty is the agent's default.
    pub model: String,
    pub prompt: String,
    /// What the person pointed at when asking (screen, element, source), as given.
    pub context: serde_json::Value,
    /// Images attached to the prompt.
    pub attachments: u32,
    pub state: RunState,
    pub error: Option<String>,
    pub tokens_in: u64,
    pub tokens_out: u64,
    /// Estimated cost; `None` when the model's price is not known.
    pub cost_usd: Option<f64>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    /// Events the runner has handed over so far (where the server's next poll starts).
    #[serde(skip)]
    pub runner_seq: u32,
}

/// A line of a run's conversation, as people read it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMessage {
    pub run: u64,
    pub seq: u32,
    pub kind: MessageKind,
    pub text: String,
    /// For `tool`: the tool's name (`Edit`, `Bash` …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageKind {
    /// What the person asked.
    User,
    /// What the agent says.
    Assistant,
    /// A tool the agent used, summarised in one line.
    Tool,
    /// Progress from charet (saved to the branch, checked, failed …).
    Status,
}

/// An image attached to a run's prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub media_type: String,
    pub data: Vec<u8>,
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
        for good in ["a", "acme", "admin-web-2", "0x"] {
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
            ".charet/project.ts",
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

/// A member of the charet: a name, a role and a token (kept apart, see [`Member`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub name: String,
    pub role: Role,
    pub created_at: String,
    /// When the token was last used to come in (browser or CLI).
    pub last_used_at: Option<String>,
}

impl User {
    /// The actor for requests this user makes.
    pub fn actor(&self) -> Actor {
        Actor::User {
            id: self.id.clone(),
            name: self.name.clone(),
            role: self.role,
        }
    }
}

/// A member as admins see them: with the token to hand over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    #[serde(flatten)]
    pub user: User,
    pub token: String,
    /// The account this machine's admin acts as (cannot be removed or demoted).
    pub owner: bool,
}

/// Something that happened that a member should know about (their inbox).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    pub id: u64,
    /// The event: `comment.added`, `comment.replied`, `comment.resolved`, `comment.reopened`,
    /// `version.created`.
    pub kind: String,
    pub project: Option<String>,
    /// Who did it.
    pub actor: String,
    /// One line for people (CLI, chat): "Ann commented on screen tasks-list: …".
    pub text: String,
    /// Where to look, as a path on the tool ("/p/web/t/screen/tasks-list?comment=12").
    pub link: String,
    /// Event details (comment id, target, body …) for clients that format it themselves.
    pub data: serde_json::Value,
    pub created_at: String,
    pub read_at: Option<String>,
}

/// An address that gets a signed POST when chosen events happen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Webhook {
    pub id: String,
    pub url: String,
    pub events: Vec<String>,
    /// Only this project's events; `None` is every project that is not private.
    pub project: Option<String>,
    /// The last 4 characters of the signing secret, to tell secrets apart.
    pub secret_last4: String,
    pub created_by: String,
    pub created_at: String,
    /// HTTP status of the last delivery, if one got an answer.
    pub last_status: Option<u16>,
    pub last_error: Option<String>,
    pub last_delivery_at: Option<String>,
}
