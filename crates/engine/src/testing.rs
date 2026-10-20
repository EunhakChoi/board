//! In-memory ports for engine tests.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use charet_model::{Comment, CommentStatus, Project, Reply, Role, User, Version, VersionKind};

use crate::{
    Accounts, Author, Build, BuildState, BuiltFile, Capture, Captures, Change, ChangeKind,
    Comments, Engine, FileChange, NewComment, NewReply, NewUser, Ports, PreviewSettings,
    ProjectStore, Records, RenderJob, Renderer, SessionRow, Settings, Shot, StoreError, Workspace,
    WorkspaceError,
};

/// The fakes behind an engine, for tests that steer or inspect them.
#[derive(Clone)]
pub struct Fakes {
    pub workspace: Arc<MemoryWorkspace>,
    pub renderer: Arc<MemoryRenderer>,
    pub realtime: Arc<MemoryRealtime>,
    pub notices: Arc<MemoryNotices>,
    pub sender: Arc<MemorySender>,
}

pub fn engine() -> Engine {
    engine_with(&fakes())
}

pub fn engine_with_preview(settings: PreviewSettings) -> (Engine, Arc<MemoryWorkspace>) {
    let fakes = fakes();
    (engine_with(&fakes).with_preview(settings), fakes.workspace)
}

/// An engine with previews on (`render_origin` = `http://render.internal`).
pub fn engine_with_fakes() -> (Engine, Fakes) {
    let fakes = fakes();
    let engine = engine_with(&fakes).with_preview(PreviewSettings {
        origin: "http://preview.test".into(),
        render_origin: "http://render.internal".into(),
        secret: [1; 32],
        ttl_secs: 600,
    });
    (engine, fakes)
}

pub fn file(path: &str, content: &str) -> FileChange {
    FileChange {
        path: path.into(),
        content: Some(content.as_bytes().to_vec()),
        executable: false,
    }
}

fn fakes() -> Fakes {
    Fakes {
        workspace: Arc::new(MemoryWorkspace::default()),
        renderer: Arc::new(MemoryRenderer::default()),
        realtime: Arc::new(MemoryRealtime::default()),
        notices: Arc::new(MemoryNotices::default()),
        sender: Arc::new(MemorySender::default()),
    }
}

fn engine_with(fakes: &Fakes) -> Engine {
    Engine::new(
        Ports {
            settings: Arc::new(MemorySettings::default()),
            records: Arc::new(MemoryRecords::default()),
            store: Arc::new(MemoryStore::default()),
            workspace: fakes.workspace.clone(),
            renderer: fakes.renderer.clone(),
            captures: Arc::new(MemoryCaptures::default()),
            comments: Arc::new(MemoryComments::default()),
            accounts: Arc::new(MemoryAccounts::default()),
            realtime: fakes.realtime.clone(),
            notices: fakes.notices.clone(),
            webhooks: fakes.sender.clone(),
        },
        true,
    )
    .with_secret([7; 32])
}

/// Records jobs. Screens render ready; with an image, the picture is `jpeg:<target>` and the
/// element map points at `src/<target>.tsx:3:5`.
#[derive(Default)]
pub struct MemoryRenderer(Mutex<Vec<RenderJob>>, Mutex<Option<serde_json::Value>>);

impl MemoryRenderer {
    pub fn jobs(&self) -> Vec<RenderJob> {
        self.0.lock().unwrap().clone()
    }

    /// The element map of every later render.
    pub fn set_elements(&self, elements: serde_json::Value) {
        *self.1.lock().unwrap() = Some(elements);
    }
}

impl Renderer for MemoryRenderer {
    fn render(&self, job: &RenderJob) -> Result<Shot, WorkspaceError> {
        self.0.lock().unwrap().push(job.clone());
        let target = job
            .url
            .split(['?', '&'])
            .find_map(|p| {
                p.strip_prefix("screen=")
                    .or_else(|| p.strip_prefix("story="))
            })
            .unwrap_or_default()
            .to_owned();
        Ok(Shot {
            ready: Some("true".into()),
            issues: Vec::new(),
            image: job.image.map(|_| format!("jpeg:{target}").into_bytes()),
            elements: job.map.then(|| {
                self.1.lock().unwrap().clone().unwrap_or_else(
                    || serde_json::json!([{ "tag": "main", "src": format!("src/{target}.tsx:3:5") }]),
                )
            }),
        })
    }
}

type CaptureKey = (String, String, String, String);
type StoredCapture = (Capture, Vec<u8>, serde_json::Value);

#[derive(Default)]
struct MemoryCaptures(Mutex<BTreeMap<CaptureKey, StoredCapture>>);

impl Captures for MemoryCaptures {
    fn remove_project(&self, project: &str) -> Result<(), StoreError> {
        self.0.lock().unwrap().retain(|key, _| key.0 != project);
        Ok(())
    }
    fn put(
        &self,
        project: &str,
        capture: &Capture,
        image: &[u8],
        elements: &serde_json::Value,
    ) -> Result<(), StoreError> {
        let key = (
            project.to_owned(),
            capture.kind.clone(),
            capture.target.clone(),
            capture.theme.clone(),
        );
        self.0
            .lock()
            .unwrap()
            .insert(key, (capture.clone(), image.to_vec(), elements.clone()));
        Ok(())
    }
    fn list(&self, project: &str) -> Result<Vec<Capture>, StoreError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|(k, _)| k.0 == project)
            .map(|(_, v)| v.0.clone())
            .collect())
    }
    fn image(
        &self,
        project: &str,
        kind: &str,
        target: &str,
        theme: &str,
    ) -> Result<Option<Vec<u8>>, StoreError> {
        let key = (project.into(), kind.into(), target.into(), theme.into());
        Ok(self.0.lock().unwrap().get(&key).map(|v| v.1.clone()))
    }
    fn elements(
        &self,
        project: &str,
        kind: &str,
        target: &str,
        theme: &str,
    ) -> Result<Option<serde_json::Value>, StoreError> {
        let key = (project.into(), kind.into(), target.into(), theme.into());
        Ok(self.0.lock().unwrap().get(&key).map(|v| v.2.clone()))
    }
}

/// (project, comment, snapshot).
type StoredComment = (String, Comment, Option<Vec<u8>>);

#[derive(Default)]
struct MemoryComments(Mutex<Vec<StoredComment>>);

impl Comments for MemoryComments {
    fn remove_project(&self, project: &str) -> Result<(), StoreError> {
        self.0.lock().unwrap().retain(|(p, _, _)| p != project);
        Ok(())
    }
    fn add(
        &self,
        project: &str,
        new: &NewComment,
        snapshot: Option<&[u8]>,
    ) -> Result<Comment, StoreError> {
        let mut all = self.0.lock().unwrap();
        let comment = Comment {
            id: all.len() as u64 + 1,
            kind: new.kind.clone(),
            target: new.target.clone(),
            theme: new.theme.clone(),
            anchor: new.anchor.clone(),
            body: new.body.clone(),
            author: new.author.clone(),
            status: CommentStatus::Open,
            created_commit: new.commit.clone(),
            resolved_commit: None,
            rev: 1,
            created_at: "now".into(),
            updated_at: "now".into(),
            replies: Vec::new(),
        };
        all.push((
            project.into(),
            comment.clone(),
            snapshot.map(<[u8]>::to_vec),
        ));
        Ok(comment)
    }
    fn list(&self, project: &str) -> Result<Vec<Comment>, StoreError> {
        let all = self.0.lock().unwrap();
        Ok(all
            .iter()
            .filter(|c| c.0 == project)
            .map(|c| c.1.clone())
            .collect())
    }
    fn get(&self, project: &str, id: u64) -> Result<Option<Comment>, StoreError> {
        Ok(self.list(project)?.into_iter().find(|c| c.id == id))
    }
    fn reply(
        &self,
        project: &str,
        id: u64,
        rev: u32,
        reply: &NewReply,
    ) -> Result<Option<Comment>, StoreError> {
        let mut all = self.0.lock().unwrap();
        let Some((_, comment, _)) = all.iter_mut().find(|c| c.0 == project && c.1.id == id) else {
            return Ok(None);
        };
        if comment.rev != rev {
            return Ok(None);
        }
        comment.rev += 1;
        if let Some(status) = reply.status {
            comment.status = status;
            comment.resolved_commit.clone_from(&reply.resolved_commit);
        }
        comment.replies.push(Reply {
            id: comment.replies.len() as u64 + 1,
            author: reply.author.clone(),
            body: reply.body.clone(),
            kind: reply.kind,
            created_at: "now".into(),
        });
        Ok(Some(comment.clone()))
    }
    fn move_anchor(
        &self,
        project: &str,
        id: u64,
        anchor: &charet_model::Anchor,
    ) -> Result<(), StoreError> {
        let mut all = self.0.lock().unwrap();
        if let Some((_, comment, _)) = all.iter_mut().find(|c| c.0 == project && c.1.id == id) {
            comment.anchor = anchor.clone();
        }
        Ok(())
    }
    fn snapshot(&self, project: &str, id: u64) -> Result<Option<Vec<u8>>, StoreError> {
        let all = self.0.lock().unwrap();
        Ok(all
            .iter()
            .find(|c| c.0 == project && c.1.id == id)
            .and_then(|c| c.2.clone()))
    }
}

/// Builds stay queued until a test calls [`MemoryWorkspace::finish`].
#[derive(Default)]
pub struct MemoryWorkspace {
    builds: Mutex<HashMap<(String, String), Build>>,
    files: Mutex<HashMap<String, Vec<u8>>>,
    starts: Mutex<usize>,
}

impl MemoryWorkspace {
    pub fn starts(&self) -> usize {
        *self.starts.lock().unwrap()
    }

    /// Makes `path` part of every build.
    pub fn serve(&self, path: &str, content: &str) {
        self.files
            .lock()
            .unwrap()
            .insert(path.into(), content.as_bytes().to_vec());
    }

    pub fn finish(&self, state: BuildState) {
        for build in self.builds.lock().unwrap().values_mut() {
            build.state = state;
        }
    }
}

impl Workspace for MemoryWorkspace {
    fn build(&self, project: &str, commit: &str) -> Result<Option<Build>, WorkspaceError> {
        Ok(self
            .builds
            .lock()
            .unwrap()
            .get(&(project.into(), commit.into()))
            .cloned())
    }
    fn start_build(
        &self,
        project: &str,
        commit: &str,
        base: &str,
        _source: Vec<u8>,
        retry: bool,
    ) -> Result<Build, WorkspaceError> {
        let mut builds = self.builds.lock().unwrap();
        let key = (project.to_owned(), commit.to_owned());
        if let Some(build) = builds.get(&key)
            && build.base == base
            && !(retry && build.state == BuildState::Failed)
        {
            return Ok(build.clone());
        }
        *self.starts.lock().unwrap() += 1;
        let build = Build {
            base: base.to_owned(),
            state: BuildState::Queued,
            steps: Vec::new(),
            error: String::new(),
        };
        builds.insert(key, build.clone());
        Ok(build)
    }
    fn file(
        &self,
        _project: &str,
        _commit: &str,
        path: &str,
    ) -> Result<Option<BuiltFile>, WorkspaceError> {
        Ok(self.files.lock().unwrap().get(path).map(|body| BuiltFile {
            content_type: "application/octet-stream".into(),
            cache_control: "no-cache".into(),
            body: body.clone(),
        }))
    }
}

#[derive(Default)]
struct MemorySettings(Mutex<HashMap<String, String>>);

impl Settings for MemorySettings {
    fn get(&self, key: &str) -> Result<Option<String>, StoreError> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }
    fn set(&self, key: &str, value: &str) -> Result<(), StoreError> {
        self.0.lock().unwrap().insert(key.into(), value.into());
        Ok(())
    }
}

#[derive(Default)]
struct MemoryRecords {
    projects: Mutex<Vec<Project>>,
    versions: Mutex<Vec<(String, Version)>>,
    members: Mutex<Vec<(String, String)>>,
}

impl Records for MemoryRecords {
    fn update_project(
        &self,
        key: &str,
        title: &str,
        visibility: charet_model::Visibility,
    ) -> Result<(), StoreError> {
        for p in self
            .projects
            .lock()
            .unwrap()
            .iter_mut()
            .filter(|p| p.key == key)
        {
            p.title = title.into();
            p.visibility = visibility;
        }
        Ok(())
    }
    fn members(&self, project: &str) -> Result<Vec<String>, StoreError> {
        Ok(self
            .members
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p == project)
            .map(|(_, u)| u.clone())
            .collect())
    }
    fn is_member(&self, project: &str, user: &str) -> Result<bool, StoreError> {
        Ok(self
            .members
            .lock()
            .unwrap()
            .iter()
            .any(|(p, u)| p == project && u == user))
    }
    fn set_member(&self, project: &str, user: &str, member: bool) -> Result<(), StoreError> {
        let mut members = self.members.lock().unwrap();
        members.retain(|(p, u)| !(p == project && u == user));
        if member {
            members.push((project.into(), user.into()));
        }
        Ok(())
    }
    fn delete_project(&self, key: &str) -> Result<bool, StoreError> {
        let mut projects = self.projects.lock().unwrap();
        let before = projects.len();
        projects.retain(|p| p.key != key);
        self.versions.lock().unwrap().retain(|(p, _)| p != key);
        Ok(projects.len() < before)
    }
    fn insert_project(&self, project: &Project) -> Result<bool, StoreError> {
        let mut projects = self.projects.lock().unwrap();
        if projects.iter().any(|p| p.key == project.key) {
            return Ok(false);
        }
        projects.push(project.clone());
        Ok(true)
    }
    fn project(&self, key: &str) -> Result<Option<Project>, StoreError> {
        Ok(self
            .projects
            .lock()
            .unwrap()
            .iter()
            .find(|p| p.key == key)
            .cloned())
    }
    fn projects(&self) -> Result<Vec<Project>, StoreError> {
        Ok(self.projects.lock().unwrap().clone())
    }
    fn add_version(
        &self,
        project: &str,
        commit: &str,
        kind: VersionKind,
        actor: &str,
        note: &str,
    ) -> Result<Version, StoreError> {
        let mut versions = self.versions.lock().unwrap();
        let number =
            u32::try_from(versions.iter().filter(|(p, _)| p == project).count() + 1).unwrap();
        let version = Version {
            number,
            commit: commit.into(),
            kind,
            actor: actor.into(),
            note: note.into(),
            created_at: String::new(),
        };
        versions.push((project.into(), version.clone()));
        Ok(version)
    }
    fn versions(&self, project: &str) -> Result<Vec<Version>, StoreError> {
        let mut found: Vec<_> = self
            .versions
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p == project)
            .map(|(_, v)| v.clone())
            .collect();
        found.reverse();
        Ok(found)
    }
}

type Files = BTreeMap<String, Vec<u8>>;

#[derive(Default)]
struct Repo {
    commits: HashMap<String, Files>,
    main: Option<String>,
    tags: HashMap<u32, String>,
}

#[derive(Default)]
struct MemoryStore(Mutex<HashMap<String, Repo>>);

impl ProjectStore for MemoryStore {
    fn remove(&self, project: &str) -> Result<(), StoreError> {
        self.0.lock().unwrap().remove(project);
        Ok(())
    }
    fn create(&self, project: &str) -> Result<(), StoreError> {
        self.0
            .lock()
            .unwrap()
            .insert(project.into(), Repo::default());
        Ok(())
    }
    fn head(&self, project: &str) -> Result<Option<String>, StoreError> {
        Ok(self.0.lock().unwrap()[project].main.clone())
    }
    fn commit_main(
        &self,
        project: &str,
        parent: Option<&str>,
        changes: &[FileChange],
        _author: &Author,
        _message: &str,
    ) -> Result<Option<String>, StoreError> {
        let mut repos = self.0.lock().unwrap();
        let repo = repos.get_mut(project).unwrap();
        if repo.main.as_deref() != parent {
            return Ok(None);
        }
        let mut files = parent.map(|p| repo.commits[p].clone()).unwrap_or_default();
        for change in changes {
            match &change.content {
                Some(content) => files.insert(change.path.clone(), content.clone()),
                None => files.remove(&change.path),
            };
        }
        let id = format!("c{}", repo.commits.len() + 1);
        repo.commits.insert(id.clone(), files);
        repo.main = Some(id.clone());
        Ok(Some(id))
    }
    fn tag_version(&self, project: &str, number: u32, commit: &str) -> Result<(), StoreError> {
        self.0
            .lock()
            .unwrap()
            .get_mut(project)
            .unwrap()
            .tags
            .insert(number, commit.into());
        Ok(())
    }
    fn resolve(&self, project: &str, rev: &str) -> Result<Option<String>, StoreError> {
        let repos = self.0.lock().unwrap();
        let repo = &repos[project];
        if rev == "main" {
            return Ok(repo.main.clone());
        }
        if let Some(number) = rev.strip_prefix('v').and_then(|n| n.parse().ok()) {
            return Ok(repo.tags.get(&number).cloned());
        }
        Ok(repo.commits.contains_key(rev).then(|| rev.to_owned()))
    }
    fn list(&self, project: &str, commit: &str) -> Result<Vec<String>, StoreError> {
        let repos = self.0.lock().unwrap();
        Ok(repos[project].commits[commit].keys().cloned().collect())
    }
    fn read(&self, project: &str, commit: &str, path: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let repos = self.0.lock().unwrap();
        Ok(repos[project].commits[commit].get(path).cloned())
    }
    fn archive(&self, _project: &str, commit: &str) -> Result<Vec<u8>, StoreError> {
        Ok(commit.as_bytes().to_vec())
    }
    fn diff(&self, project: &str, from: &str, to: &str) -> Result<Vec<Change>, StoreError> {
        let repos = self.0.lock().unwrap();
        let (a, b) = (&repos[project].commits[from], &repos[project].commits[to]);
        let mut changes = Vec::new();
        for (path, content) in b {
            match a.get(path) {
                None => changes.push(Change {
                    path: path.clone(),
                    kind: ChangeKind::Added,
                }),
                Some(old) if old != content => {
                    changes.push(Change {
                        path: path.clone(),
                        kind: ChangeKind::Modified,
                    });
                }
                Some(_) => {}
            }
        }
        for path in a.keys().filter(|p| !b.contains_key(*p)) {
            changes.push(Change {
                path: path.clone(),
                kind: ChangeKind::Deleted,
            });
        }
        changes.sort_by(|x, y| x.path.cmp(&y.path));
        Ok(changes)
    }
}

/// A public engine (no local door).
pub fn public_engine() -> Engine {
    let mut engine = engine_with(&fakes());
    engine.local_mode = false;
    engine
}

/// Members in memory: (token, user), sessions by hash: (user, expires, replaced).
#[derive(Default)]
pub struct MemoryAccounts {
    users: Mutex<Vec<(String, User)>>,
    sessions: Mutex<HashMap<String, (String, u64, bool)>>,
}

impl Accounts for MemoryAccounts {
    fn user(&self, id: &str) -> Result<Option<User>, StoreError> {
        Ok(self
            .users
            .lock()
            .unwrap()
            .iter()
            .find(|(_, u)| u.id == id)
            .map(|(_, u)| u.clone()))
    }
    fn users(&self) -> Result<Vec<User>, StoreError> {
        Ok(self
            .users
            .lock()
            .unwrap()
            .iter()
            .map(|(_, u)| u.clone())
            .collect())
    }
    fn insert_user(&self, new: &NewUser) -> Result<User, StoreError> {
        let user = User {
            id: new.id.clone(),
            name: new.name.clone(),
            role: new.role,
            created_at: "2026-01-01T00:00:00Z".into(),
            last_used_at: None,
        };
        self.users
            .lock()
            .unwrap()
            .push((new.token.clone(), user.clone()));
        Ok(user)
    }
    fn update_user(&self, id: &str, name: &str, role: Role) -> Result<bool, StoreError> {
        let mut users = self.users.lock().unwrap();
        let Some((_, user)) = users.iter_mut().find(|(_, u)| u.id == id) else {
            return Ok(false);
        };
        user.name = name.to_owned();
        user.role = role;
        Ok(true)
    }
    fn delete_user(&self, id: &str) -> Result<bool, StoreError> {
        let mut users = self.users.lock().unwrap();
        let before = users.len();
        users.retain(|(_, u)| u.id != id);
        self.sessions.lock().unwrap().retain(|_, (u, ..)| u != id);
        Ok(users.len() != before)
    }
    fn user_token(&self, id: &str) -> Result<Option<String>, StoreError> {
        Ok(self
            .users
            .lock()
            .unwrap()
            .iter()
            .find(|(_, u)| u.id == id)
            .map(|(t, _)| t.clone()))
    }
    fn set_user_token(&self, id: &str, token: &str) -> Result<bool, StoreError> {
        let mut users = self.users.lock().unwrap();
        let Some((old, _)) = users.iter_mut().find(|(_, u)| u.id == id) else {
            return Ok(false);
        };
        token.clone_into(old);
        Ok(true)
    }
    fn user_by_token(&self, token: &str) -> Result<Option<User>, StoreError> {
        let mut users = self.users.lock().unwrap();
        Ok(users.iter_mut().find(|(t, _)| t == token).map(|(_, u)| {
            u.last_used_at = Some("now".into());
            u.clone()
        }))
    }
    fn insert_session(
        &self,
        token_hash: &str,
        user: &str,
        expires_at: u64,
    ) -> Result<(), StoreError> {
        self.sessions
            .lock()
            .unwrap()
            .insert(token_hash.into(), (user.into(), expires_at, false));
        Ok(())
    }
    fn session(&self, token_hash: &str, now: u64) -> Result<Option<SessionRow>, StoreError> {
        let Some((user, expires, replaced)) =
            self.sessions.lock().unwrap().get(token_hash).cloned()
        else {
            return Ok(None);
        };
        if expires <= now {
            return Ok(None);
        }
        Ok(self.user(&user)?.map(|user| SessionRow { user, replaced }))
    }
    fn replace_sessions(&self, user: &str, except_hash: &str) -> Result<(), StoreError> {
        for (hash, (u, _, replaced)) in self.sessions.lock().unwrap().iter_mut() {
            if u == user && hash != except_hash {
                *replaced = true;
            }
        }
        Ok(())
    }
    fn delete_session(&self, token_hash: &str) -> Result<(), StoreError> {
        self.sessions.lock().unwrap().remove(token_hash);
        Ok(())
    }
    fn delete_sessions(&self, user: &str) -> Result<(), StoreError> {
        self.sessions.lock().unwrap().retain(|_, (u, ..)| u != user);
        Ok(())
    }
}

/// Records what commands publish; rooms are not simulated.
#[derive(Default)]
pub struct MemoryRealtime {
    pub events: Mutex<Vec<(String, String)>>,
    kicked: Mutex<Vec<crate::Kick>>,
}

impl MemoryRealtime {
    pub fn kicks(&self) -> Vec<crate::Kick> {
        self.kicked.lock().unwrap().clone()
    }
}

impl crate::Realtime for MemoryRealtime {
    fn join(&self, _: &str, _: crate::Peer, _: Option<u64>) -> crate::Joined {
        let (_, receiver) = tokio::sync::mpsc::unbounded_channel();
        crate::Joined {
            client: 1,
            hello: "{}".into(),
            receiver,
        }
    }
    fn presence(&self, _: &str, _: u64, _: serde_json::Value) {}
    fn leave(&self, _: &str, _: u64) {}
    fn publish(&self, project: &str, kind: &str, _: serde_json::Value) {
        self.events
            .lock()
            .unwrap()
            .push((project.into(), kind.into()));
    }
    fn publish_admins(&self, kind: &str, _: serde_json::Value) {
        self.events.lock().unwrap().push(("*".into(), kind.into()));
    }
    fn notify(&self, people: &[String]) {
        self.events
            .lock()
            .unwrap()
            .push(("*".into(), format!("notify {}", people.join(","))));
    }
    fn kick(&self, who: crate::Kick) {
        self.kicked.lock().unwrap().push(who.clone());
        self.events
            .lock()
            .unwrap()
            .push(("*".into(), format!("kick {who:?}")));
    }
    fn working(&self, project: &str, person: &charet_model::Person, on: bool) {
        self.events
            .lock()
            .unwrap()
            .push((project.into(), format!("working {} {on}", person.id)));
    }
}

/// Notifications and webhooks in memory.
#[derive(Default)]
pub struct MemoryNotices {
    notifications: Mutex<Vec<(String, charet_model::Notification)>>,
    hooks: Mutex<Vec<(charet_model::Webhook, String)>>,
}

impl crate::Notices for MemoryNotices {
    fn insert_notifications(&self, rows: &[crate::NewNotification]) -> Result<(), StoreError> {
        let mut all = self.notifications.lock().unwrap();
        for row in rows {
            let id = all.len() as u64 + 1;
            all.push((
                row.user.clone(),
                charet_model::Notification {
                    id,
                    kind: row.kind.clone(),
                    project: row.project.clone(),
                    actor: row.actor.clone(),
                    text: row.text.clone(),
                    link: row.link.clone(),
                    data: row.data.clone(),
                    created_at: String::new(),
                    read_at: None,
                },
            ));
        }
        Ok(())
    }
    fn notifications(
        &self,
        user: &str,
        unread_only: bool,
        limit: u32,
    ) -> Result<Vec<charet_model::Notification>, StoreError> {
        Ok(self
            .notifications
            .lock()
            .unwrap()
            .iter()
            .rev()
            .filter(|(u, n)| u == user && (!unread_only || n.read_at.is_none()))
            .take(limit as usize)
            .map(|(_, n)| n.clone())
            .collect())
    }
    fn unread_count(&self, user: &str) -> Result<u32, StoreError> {
        Ok(u32::try_from(self.notifications(user, true, u32::MAX)?.len()).unwrap_or(u32::MAX))
    }
    fn mark_read(&self, user: &str, ids: Option<&[u64]>) -> Result<(), StoreError> {
        for (u, n) in self.notifications.lock().unwrap().iter_mut() {
            if u == user && ids.is_none_or(|ids| ids.contains(&n.id)) {
                n.read_at = Some("now".into());
            }
        }
        Ok(())
    }
    fn webhooks(&self) -> Result<Vec<(charet_model::Webhook, String)>, StoreError> {
        Ok(self.hooks.lock().unwrap().clone())
    }
    fn insert_webhook(
        &self,
        hook: &crate::NewWebhook,
    ) -> Result<charet_model::Webhook, StoreError> {
        let webhook = charet_model::Webhook {
            id: hook.id.clone(),
            url: hook.url.clone(),
            events: hook.events.clone(),
            project: hook.project.clone(),
            secret_last4: hook.secret_last4.clone(),
            created_by: hook.created_by.clone(),
            created_at: String::new(),
            last_status: None,
            last_error: None,
            last_delivery_at: None,
        };
        self.hooks
            .lock()
            .unwrap()
            .push((webhook.clone(), hook.salt.clone()));
        Ok(webhook)
    }
    fn delete_webhook(&self, id: &str) -> Result<bool, StoreError> {
        let mut hooks = self.hooks.lock().unwrap();
        let before = hooks.len();
        hooks.retain(|(h, _)| h.id != id);
        Ok(hooks.len() != before)
    }
    fn set_webhook_salt(&self, id: &str, salt: &str, last4: &str) -> Result<bool, StoreError> {
        let mut hooks = self.hooks.lock().unwrap();
        let Some((hook, old)) = hooks.iter_mut().find(|(h, _)| h.id == id) else {
            return Ok(false);
        };
        salt.clone_into(old);
        last4.clone_into(&mut hook.secret_last4);
        Ok(true)
    }
    fn record_delivery(
        &self,
        id: &str,
        status: Option<u16>,
        error: Option<&str>,
    ) -> Result<(), StoreError> {
        if let Some((hook, _)) = self
            .hooks
            .lock()
            .unwrap()
            .iter_mut()
            .find(|(h, _)| h.id == id)
        {
            hook.last_status = status;
            hook.last_error = error.map(str::to_owned);
            hook.last_delivery_at = Some("now".into());
        }
        Ok(())
    }
}

/// One recorded POST: url, headers, body.
pub type Post = (String, Vec<(String, String)>, String);

/// Records webhook POSTs and answers 200.
#[derive(Default)]
pub struct MemorySender(pub Mutex<Vec<Post>>);

impl MemorySender {
    /// Waits up to two seconds for `n` posts (deliveries run on their own threads).
    pub fn wait_for(&self, n: usize) -> Vec<Post> {
        for _ in 0..200 {
            let posts = self.0.lock().unwrap().clone();
            if posts.len() >= n {
                return posts;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        self.0.lock().unwrap().clone()
    }
}

impl crate::WebhookSender for MemorySender {
    fn post(&self, url: &str, headers: &[(&str, String)], body: &[u8]) -> Result<u16, String> {
        self.0.lock().unwrap().push((
            url.to_owned(),
            headers
                .iter()
                .map(|(k, v)| ((*k).to_owned(), v.clone()))
                .collect(),
            String::from_utf8_lossy(body).into_owned(),
        ));
        Ok(200)
    }
}
