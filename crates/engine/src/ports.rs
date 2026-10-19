//! What the engine needs from storage. Implemented by `board-db` and `board-store`.

use board_model::{Project, Version, VersionKind};

/// A storage failure, with enough text to log. Callers map it to a 500.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError(pub String);

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for StoreError {}

/// Settings and other key-value state kept by the database.
pub trait Settings: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>, StoreError>;
    fn set(&self, key: &str, value: &str) -> Result<(), StoreError>;
}

/// Projects and versions kept by the database.
pub trait Records: Send + Sync {
    /// Adds a project. Returns false when the key is taken.
    fn insert_project(&self, project: &Project) -> Result<bool, StoreError>;
    fn project(&self, key: &str) -> Result<Option<Project>, StoreError>;
    fn projects(&self) -> Result<Vec<Project>, StoreError>;
    /// Records the next version of `project` (numbers start at 1) and returns it.
    fn add_version(
        &self,
        project: &str,
        commit: &str,
        kind: VersionKind,
        actor: &str,
        note: &str,
    ) -> Result<Version, StoreError>;
    /// Newest first.
    fn versions(&self, project: &str) -> Result<Vec<Version>, StoreError>;
    /// Removes a project with its versions, captures and comments. False when there was none.
    fn delete_project(&self, key: &str) -> Result<bool, StoreError>;
    /// Renames a project and sets who can see it.
    fn update_project(
        &self,
        key: &str,
        title: &str,
        visibility: board_model::Visibility,
    ) -> Result<(), StoreError>;
    /// Members (user ids) of a private project.
    fn members(&self, project: &str) -> Result<Vec<String>, StoreError>;
    fn is_member(&self, project: &str, user: &str) -> Result<bool, StoreError>;
    /// Adds (`true`) or removes a member.
    fn set_member(&self, project: &str, user: &str, member: bool) -> Result<(), StoreError>;
}

/// Who a commit is attributed to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Author {
    pub name: String,
    pub email: String,
}

/// One file to write (`content: Some`) or delete (`content: None`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    pub content: Option<Vec<u8>>,
    pub executable: bool,
}

/// A changed path between two commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub path: String,
    pub kind: ChangeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
}

/// The project store: one git repository per project.
pub trait ProjectStore: Send + Sync {
    /// Creates the empty repository for `project`.
    fn create(&self, project: &str) -> Result<(), StoreError>;
    /// Deletes the project's history; nothing to delete is fine.
    fn remove(&self, project: &str) -> Result<(), StoreError>;
    /// The commit `main` points at, or `None` before the first commit.
    fn head(&self, project: &str) -> Result<Option<String>, StoreError>;
    /// Commits `changes` on top of `parent` (`None` = first commit) and moves `main` there, but only
    /// if `main` is still at `parent`. Returns the new commit, or `None` when `main` had moved.
    fn commit_main(
        &self,
        project: &str,
        parent: Option<&str>,
        changes: &[FileChange],
        author: &Author,
        message: &str,
    ) -> Result<Option<String>, StoreError>;
    /// Marks `commit` as version `number` (tag `v/<number>`).
    fn tag_version(&self, project: &str, number: u32, commit: &str) -> Result<(), StoreError>;
    /// Resolves `main`, `v<number>` or a commit id to a commit id.
    fn resolve(&self, project: &str, rev: &str) -> Result<Option<String>, StoreError>;
    /// Every file path of `commit`, sorted.
    fn list(&self, project: &str, commit: &str) -> Result<Vec<String>, StoreError>;
    /// One file of `commit`, or `None` when it does not exist.
    fn read(&self, project: &str, commit: &str, path: &str) -> Result<Option<Vec<u8>>, StoreError>;
    /// The files of `commit` as a gzipped tar.
    fn archive(&self, project: &str, commit: &str) -> Result<Vec<u8>, StoreError>;
    /// Paths that differ from `from` to `to`.
    fn diff(&self, project: &str, from: &str, to: &str) -> Result<Vec<Change>, StoreError>;
}

/// Where a static build of one commit is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BuildState {
    Queued,
    Running,
    Ready,
    Failed,
}

/// One step of a build (extract, install, typecheck, lint, build).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildStep {
    pub name: String,
    /// `ok`, `failed` or `skipped`.
    pub state: String,
    pub duration_ms: u64,
    pub detail: String,
}

/// A static build of one commit, as `board-workspace` reports it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Build {
    /// The base path it was built for; a build for another one is made again.
    #[serde(skip)]
    pub base: String,
    pub state: BuildState,
    pub steps: Vec<BuildStep>,
    pub error: String,
}

/// A built file, ready to send to the preview origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltFile {
    pub content_type: String,
    pub cache_control: String,
    pub body: Vec<u8>,
}

/// Why `board-workspace` could not answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceError {
    /// Not running or not reachable.
    Unavailable(String),
    /// Answered with something unexpected.
    Failed(String),
}

/// `board-workspace`: installs, checks and builds commits (`apps/board-workspace`).
pub trait Workspace: Send + Sync {
    /// The build of `commit`, if one was started.
    fn build(&self, project: &str, commit: &str) -> Result<Option<Build>, WorkspaceError>;
    /// Starts building `commit` from `source` (gzipped tar of its files), served under `base`.
    /// Returns the current build unchanged when one exists, unless it failed and `retry` is set.
    fn start_build(
        &self,
        project: &str,
        commit: &str,
        base: &str,
        source: Vec<u8>,
        retry: bool,
    ) -> Result<Build, WorkspaceError>;
    /// A file of a ready build; `None` when there is no such file.
    fn file(
        &self,
        project: &str,
        commit: &str,
        path: &str,
    ) -> Result<Option<BuiltFile>, WorkspaceError>;
    /// Whether the service answers.
    fn health(&self) -> Result<(), WorkspaceError> {
        Ok(())
    }
}

/// Picture format of a render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    Jpeg,
    Png,
}

/// One page for `board-render` to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderJob {
    pub url: String,
    pub width: u32,
    pub height: u32,
    /// `None` = check only.
    pub image: Option<ImageKind>,
    /// Collect the element map.
    pub map: bool,
}

/// What `board-render` saw.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Shot {
    /// `data-board-ready` (`true` / `error`), `None` when the page never got ready.
    pub ready: Option<String>,
    pub issues: Vec<String>,
    pub image: Option<Vec<u8>>,
    /// Element map: `[{x, y, w, h, chain, sel, tag, text, src?, slot?}]`.
    pub elements: Option<serde_json::Value>,
}

/// `board-render`: Chromium screenshots and checks (`apps/board-render`).
pub trait Renderer: Send + Sync {
    fn render(&self, job: &RenderJob) -> Result<Shot, WorkspaceError>;
    /// Whether the service answers.
    fn health(&self) -> Result<(), WorkspaceError> {
        Ok(())
    }
}

/// A stored capture of one screen or story in one theme (the latest one).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capture {
    /// `screen` or `story`.
    pub kind: String,
    pub target: String,
    /// `light` or `dark`.
    pub theme: String,
    pub commit: String,
    pub width: u32,
    pub height: u32,
    pub issues: Vec<String>,
    /// Source files the rendered elements came from (`data-src`, relative to the app).
    pub sources: Vec<String>,
    pub captured_at: String,
}

/// Where captures live: metadata plus the image and element map files.
pub trait Captures: Send + Sync {
    /// Stores `capture`, replacing the previous one for the same (kind, target, theme).
    fn put(
        &self,
        project: &str,
        capture: &Capture,
        image: &[u8],
        elements: &serde_json::Value,
    ) -> Result<(), StoreError>;
    fn list(&self, project: &str) -> Result<Vec<Capture>, StoreError>;
    fn image(
        &self,
        project: &str,
        kind: &str,
        target: &str,
        theme: &str,
    ) -> Result<Option<Vec<u8>>, StoreError>;
    fn elements(
        &self,
        project: &str,
        kind: &str,
        target: &str,
        theme: &str,
    ) -> Result<Option<serde_json::Value>, StoreError>;
    /// Deletes every capture file of `project` (its rows go with the project).
    fn remove_project(&self, project: &str) -> Result<(), StoreError>;
}

/// A comment to store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewComment {
    pub kind: String,
    pub target: String,
    pub theme: String,
    pub anchor: board_model::Anchor,
    pub body: String,
    pub author: board_model::Person,
    pub commit: String,
}

/// A thread entry to store, with the status it sets (resolve, reopen).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewReply {
    pub author: board_model::Person,
    pub body: String,
    pub kind: board_model::ReplyKind,
    pub status: Option<board_model::CommentStatus>,
    /// Set with a resolve.
    pub resolved_commit: Option<String>,
}

/// Comments, their threads and the capture snapshot each was made on.
pub trait Comments: Send + Sync {
    fn add(
        &self,
        project: &str,
        comment: &NewComment,
        snapshot: Option<&[u8]>,
    ) -> Result<board_model::Comment, StoreError>;
    /// Every comment of `project` with its replies, oldest first.
    fn list(&self, project: &str) -> Result<Vec<board_model::Comment>, StoreError>;
    fn get(&self, project: &str, id: u64) -> Result<Option<board_model::Comment>, StoreError>;
    /// Adds `reply` when the comment is still at `rev`; returns `None` when it moved on.
    fn reply(
        &self,
        project: &str,
        id: u64,
        rev: u32,
        reply: &NewReply,
    ) -> Result<Option<board_model::Comment>, StoreError>;
    /// Moves the anchor after a new capture (does not count as a change by a person).
    fn move_anchor(
        &self,
        project: &str,
        id: u64,
        anchor: &board_model::Anchor,
    ) -> Result<(), StoreError>;
    fn snapshot(&self, project: &str, id: u64) -> Result<Option<Vec<u8>>, StoreError>;
    /// Deletes every snapshot file of `project` (its rows go with the project).
    fn remove_project(&self, project: &str) -> Result<(), StoreError>;
}

/// A new account, as the first sign-in creates it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewUser {
    pub id: String,
    pub provider: String,
    pub subject: String,
    pub email: String,
    pub name: String,
    pub avatar_url: Option<String>,
    pub role: board_model::Role,
    pub status: board_model::UserStatus,
}

/// Domains and addresses that are let in on their first sign-in.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AllowRule {
    pub id: i64,
    /// `domain` (`example.com`) or `email` (`someone@example.com`).
    pub kind: String,
    pub value: String,
}

/// Accounts, sessions and admission rules kept by the database. Tokens arrive hashed.
pub trait Accounts: Send + Sync {
    fn user(&self, id: &str) -> Result<Option<board_model::User>, StoreError>;
    /// The account the provider knows as `subject`.
    fn user_by_subject(
        &self,
        provider: &str,
        subject: &str,
    ) -> Result<Option<board_model::User>, StoreError>;
    fn insert_user(&self, user: &NewUser) -> Result<board_model::User, StoreError>;
    /// Refreshes what the provider says about the person (name, picture, address).
    fn update_profile(
        &self,
        id: &str,
        email: &str,
        name: &str,
        avatar_url: Option<&str>,
    ) -> Result<(), StoreError>;
    fn set_access(
        &self,
        id: &str,
        role: board_model::Role,
        status: board_model::UserStatus,
    ) -> Result<(), StoreError>;
    /// Whether any active admin exists (until one does, the setup link is the way in).
    fn has_admin(&self) -> Result<bool, StoreError>;
    fn insert_session(
        &self,
        token_hash: &str,
        user: &str,
        expires_at: u64,
    ) -> Result<(), StoreError>;
    /// The account behind a session that has not expired at `now`.
    fn session_user(
        &self,
        token_hash: &str,
        now: u64,
    ) -> Result<Option<board_model::User>, StoreError>;
    fn delete_session(&self, token_hash: &str) -> Result<(), StoreError>;
    /// Replaces any unused setup token with this one.
    fn set_setup_token(&self, token_hash: &str) -> Result<(), StoreError>;
    /// Uses up the setup token. False when it is not the current unused one.
    fn take_setup_token(&self, token_hash: &str) -> Result<bool, StoreError>;
    fn allow_rules(&self) -> Result<Vec<AllowRule>, StoreError>;
    fn add_allow_rule(&self, kind: &str, value: &str) -> Result<AllowRule, StoreError>;
    /// False when there was no such rule.
    fn remove_allow_rule(&self, id: i64) -> Result<bool, StoreError>;
    /// Every account, oldest first.
    fn users(&self) -> Result<Vec<board_model::User>, StoreError>;
    fn insert_share_link(&self, link: &NewShareLink) -> Result<board_model::ShareLink, StoreError>;
    /// A project's links, newest first, revoked ones included.
    fn share_links(&self, project: &str) -> Result<Vec<board_model::ShareLink>, StoreError>;
    fn share_link_by_token(
        &self,
        token_hash: &str,
    ) -> Result<Option<board_model::ShareLink>, StoreError>;
    /// Marks the link revoked and ends every session that came in through it. False when the
    /// project has no such link or it was revoked already.
    fn revoke_share_link(&self, project: &str, id: &str) -> Result<bool, StoreError>;
    fn insert_guest(&self, id: &str, link: &str, name: &str) -> Result<(), StoreError>;
    fn insert_guest_session(
        &self,
        token_hash: &str,
        guest: &str,
        expires_at: u64,
    ) -> Result<(), StoreError>;
    fn insert_cli_token(
        &self,
        id: &str,
        user: &str,
        name: &str,
        token_hash: &str,
        via: &str,
    ) -> Result<board_model::CliToken, StoreError>;
    fn cli_tokens(&self, user: &str) -> Result<Vec<board_model::CliToken>, StoreError>;
    /// False when the user has no such token.
    fn delete_cli_token(&self, user: &str, id: &str) -> Result<bool, StoreError>;
    fn delete_cli_token_by_hash(&self, token_hash: &str) -> Result<(), StoreError>;
    /// The account behind a CLI token; records the use.
    fn token_user(&self, token_hash: &str) -> Result<Option<board_model::User>, StoreError>;
    fn insert_device(
        &self,
        code_hash: &str,
        user_code: &str,
        name: &str,
        expires_at: u64,
    ) -> Result<(), StoreError>;
    fn device_by_user_code(&self, user_code: &str) -> Result<Option<Device>, StoreError>;
    fn device_by_code(&self, code_hash: &str) -> Result<Option<Device>, StoreError>;
    /// Approves (for `user`) or denies a waiting device login. False when it is not waiting.
    fn decide_device(&self, user_code: &str, user: &str, approve: bool)
    -> Result<bool, StoreError>;
    fn delete_device(&self, code_hash: &str) -> Result<(), StoreError>;
    /// The guest behind a live session, with the (unrevoked) link they came through.
    fn guest_session(
        &self,
        token_hash: &str,
        now: u64,
    ) -> Result<Option<(Guest, board_model::ShareLink)>, StoreError>;
}

/// A share link to create; the token arrives hashed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewShareLink {
    pub id: String,
    pub project: String,
    pub token_hash: String,
    pub role: board_model::Role,
    pub label: String,
    pub created_by: String,
}

/// A CLI login waiting for someone to approve it in the browser (device flow).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub user_code: String,
    pub name: String,
    /// `pending`, `approved` or `denied`.
    pub status: String,
    pub user: Option<String>,
    pub expires_at: u64,
}

/// Someone who came in through a share link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Guest {
    pub id: String,
    pub name: String,
}

/// What the identity provider says about the person who signed in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claims {
    pub subject: String,
    pub email: String,
    pub email_verified: bool,
    pub name: Option<String>,
    pub picture: Option<String>,
}

/// One sign-in round trip the provider is asked to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthRequest {
    pub state: String,
    pub nonce: String,
    /// PKCE `S256` challenge of the verifier the engine keeps.
    pub code_challenge: String,
}

/// The sign-in provider (OIDC). Implemented by `board-oidc`.
pub trait Identity: Send + Sync {
    /// Short name stored with accounts (`google`, or the issuer's host).
    fn provider(&self) -> &str;
    /// Where to send the browser to sign in.
    fn authorize_url(&self, request: &AuthRequest) -> Result<String, WorkspaceError>;
    /// Trades the callback's `code` for the person's claims, checking `nonce`.
    fn exchange(
        &self,
        code: &str,
        code_verifier: &str,
        nonce: &str,
    ) -> Result<Claims, WorkspaceError>;
    /// Whether the provider answers (its discovery document loads).
    fn health(&self) -> Result<(), WorkspaceError> {
        Ok(())
    }
    /// The callback address registered at the provider.
    #[expect(
        clippy::unnecessary_literal_bound,
        reason = "implementations return a field of their own"
    )]
    fn callback_url(&self) -> &str {
        ""
    }
}

/// Someone in a project room, as the others see them.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Peer {
    /// This connection (one person can have several tabs).
    pub id: String,
    /// `local`, a user id or `guest:<id>` (same as comment authors).
    pub person: String,
    pub name: String,
    /// `local`, `user` or `guest`.
    pub kind: String,
    pub role: board_model::Role,
    pub avatar_url: Option<String>,
}

/// What a connection receives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outgoing {
    /// A JSON message.
    Text(String),
    /// Close the connection (access was taken away).
    Close,
}

/// A connection that joined a room.
#[derive(Debug)]
pub struct Joined {
    pub client: u64,
    /// The first message: who is here, the latest sequence number and missed events.
    pub hello: String,
    pub receiver: tokio::sync::mpsc::UnboundedReceiver<Outgoing>,
}

/// Whose connections to close.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kick {
    /// Every connection of this person (`local`, user id or `guest:<id>`).
    Person(String),
    /// Every guest in this project (their link was revoked; the rest reconnect).
    Guests(String),
}

/// The realtime hub (`board-collab`): rooms per project, presence relayed as is, events with a
/// sequence number so a reconnecting client can catch up.
pub trait Realtime: Send + Sync {
    fn join(&self, project: &str, peer: Peer, since: Option<u64>) -> Joined;
    /// Relays a presence message (cursor, selection, viewport, chat …) to the others in the room.
    fn presence(&self, project: &str, client: u64, data: serde_json::Value);
    fn leave(&self, project: &str, client: u64);
    /// An event everyone in the project room gets.
    fn publish(&self, project: &str, kind: &str, data: serde_json::Value);
    /// An event only admins get, wherever they are.
    fn publish_admins(&self, kind: &str, data: serde_json::Value);
    /// Tells these people (user ids) their inbox changed, wherever they are connected.
    fn notify(&self, people: &[String]);
    fn kick(&self, who: Kick);
    /// Marks someone working on the project from their own checkout (until they push).
    fn working(&self, project: &str, person: &board_model::Person, on: bool);
}

/// A notification to store for one member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewNotification {
    pub user: String,
    pub kind: String,
    pub project: Option<String>,
    pub actor: String,
    pub text: String,
    pub link: String,
    pub data: serde_json::Value,
}

/// A webhook to store. `salt` with the server key gives the signing secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewWebhook {
    pub id: String,
    pub url: String,
    pub events: Vec<String>,
    pub project: Option<String>,
    pub salt: String,
    pub secret_last4: String,
    pub created_by: String,
}

/// Inbox notifications and webhooks kept by the database.
pub trait Notices: Send + Sync {
    fn insert_notifications(&self, notifications: &[NewNotification]) -> Result<(), StoreError>;
    /// Newest first, at most `limit`.
    fn notifications(
        &self,
        user: &str,
        unread_only: bool,
        limit: u32,
    ) -> Result<Vec<board_model::Notification>, StoreError>;
    fn unread_count(&self, user: &str) -> Result<u32, StoreError>;
    /// Marks these (or, with `None`, all) of the user's notifications read.
    fn mark_read(&self, user: &str, ids: Option<&[u64]>) -> Result<(), StoreError>;
    /// Every webhook with its salt.
    fn webhooks(&self) -> Result<Vec<(board_model::Webhook, String)>, StoreError>;
    fn insert_webhook(&self, hook: &NewWebhook) -> Result<board_model::Webhook, StoreError>;
    fn delete_webhook(&self, id: &str) -> Result<bool, StoreError>;
    fn set_webhook_salt(&self, id: &str, salt: &str, last4: &str) -> Result<bool, StoreError>;
    /// The outcome of the latest delivery: an HTTP status, or an error when none came back.
    fn record_delivery(
        &self,
        id: &str,
        status: Option<u16>,
        error: Option<&str>,
    ) -> Result<(), StoreError>;
}

/// Sends one webhook POST and returns the HTTP status (`board-server` over HTTPS).
pub trait WebhookSender: Send + Sync {
    fn post(&self, url: &str, headers: &[(&str, String)], body: &[u8]) -> Result<u16, String>;
}

/// An invite to store; only the token's hash is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewInvite {
    pub id: String,
    pub token_hash: String,
    pub role: board_model::Role,
    pub user: Option<String>,
    pub label: String,
    pub created_by: String,
    pub expires_at: u64,
}

/// One-time invite and sign-in links kept by the database.
pub trait Invites: Send + Sync {
    fn insert_invite(&self, invite: &NewInvite) -> Result<board_model::Invite, StoreError>;
    fn invite_by_token(&self, token_hash: &str) -> Result<Option<board_model::Invite>, StoreError>;
    /// Marks it used by `user`; false when someone used it first.
    fn use_invite(&self, id: &str, user: &str) -> Result<bool, StoreError>;
    /// Invites for new members (not sign-in links), newest first.
    fn invites(&self) -> Result<Vec<board_model::Invite>, StoreError>;
    fn delete_invite(&self, id: &str) -> Result<bool, StoreError>;
}

/// Runs the tunnels that bring people to the remote door (`board-tunnel`): Tailscale serve and
/// Funnel. Calls may take a few seconds.
pub trait Tunnels: Send + Sync {
    /// The ways this machine can share right now, for the sharing page.
    fn modes(&self) -> Vec<TunnelMode>;
    /// Opens `mode` to the remote door; returns the address people open.
    fn open(&self, mode: crate::ShareMode) -> Result<String, String>;
    fn close(&self, mode: crate::ShareMode) -> Result<(), String>;
}

/// One way to share, as this machine sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunnelMode {
    pub mode: crate::ShareMode,
    pub available: bool,
    /// The address it would give, or why it cannot be used.
    pub detail: String,
}
