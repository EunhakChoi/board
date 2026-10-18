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
}
