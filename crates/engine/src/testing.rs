//! In-memory ports for engine tests.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use board_model::{
    CliToken, Comment, CommentStatus, Project, Reply, Role, ShareLink, User, UserStatus, Version,
    VersionKind,
};

use crate::{
    Accounts, AllowRule, AuthRequest, Author, Build, BuildState, BuiltFile, Capture, Captures,
    Change, ChangeKind, Claims, Comments, Device, Engine, FileChange, Guest, Identity, NewComment,
    NewReply, NewShareLink, NewUser, Ports, PreviewSettings, ProjectStore, Records, RenderJob,
    Renderer, Settings, Shot, SignInSettings, StoreError, Workspace, WorkspaceError as IdpError,
    WorkspaceError,
};

/// The fakes behind an engine, for tests that steer or inspect them.
#[derive(Clone)]
pub struct Fakes {
    pub workspace: Arc<MemoryWorkspace>,
    pub renderer: Arc<MemoryRenderer>,
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
        },
        true,
    )
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
        anchor: &board_model::Anchor,
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
        _base: &str,
        _source: Vec<u8>,
        retry: bool,
    ) -> Result<Build, WorkspaceError> {
        let mut builds = self.builds.lock().unwrap();
        let key = (project.to_owned(), commit.to_owned());
        if let Some(build) = builds.get(&key)
            && !(retry && build.state == BuildState::Failed)
        {
            return Ok(build.clone());
        }
        *self.starts.lock().unwrap() += 1;
        let build = Build {
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
        visibility: board_model::Visibility,
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

/// An engine with sign-in on (not local mode), behind a scripted identity provider.
pub fn engine_with_sign_in(admin_emails: &[&str]) -> (Engine, Arc<FakeIdentity>) {
    let idp = Arc::new(FakeIdentity::default());
    let mut engine = engine_with(&fakes()).with_sign_in(SignInSettings {
        identity: idp.clone(),
        admin_emails: admin_emails.iter().map(|e| (*e).to_owned()).collect(),
    });
    engine.local_mode = false;
    (engine, idp)
}

/// Adds an allowlist rule straight to the accounts.
pub fn allow(engine: &Engine, kind: &str, value: &str) {
    engine.ports.accounts.add_allow_rule(kind, value).unwrap();
}

/// Accounts in memory.
#[derive(Default)]
pub struct MemoryAccounts {
    users: Mutex<Vec<(String, String, User)>>,
    sessions: Mutex<HashMap<String, (String, u64)>>,
    setup: Mutex<Option<String>>,
    rules: Mutex<Vec<AllowRule>>,
    links: Mutex<Vec<(String, ShareLink)>>,
    guests: Mutex<Vec<(Guest, String)>>,
    guest_sessions: Mutex<HashMap<String, (String, u64)>>,
    tokens: Mutex<Vec<(String, String, CliToken)>>,
    devices: Mutex<HashMap<String, Device>>,
}

impl Accounts for MemoryAccounts {
    fn user(&self, id: &str) -> Result<Option<User>, StoreError> {
        Ok(self
            .users
            .lock()
            .unwrap()
            .iter()
            .find(|u| u.2.id == id)
            .map(|u| u.2.clone()))
    }

    fn user_by_subject(&self, provider: &str, subject: &str) -> Result<Option<User>, StoreError> {
        Ok(self
            .users
            .lock()
            .unwrap()
            .iter()
            .find(|u| u.0 == provider && u.1 == subject)
            .map(|u| u.2.clone()))
    }

    fn insert_user(&self, new: &NewUser) -> Result<User, StoreError> {
        let user = User {
            id: new.id.clone(),
            email: new.email.clone(),
            name: new.name.clone(),
            avatar_url: new.avatar_url.clone(),
            role: new.role,
            status: new.status,
            created_at: "2026-10-19T00:00:00Z".into(),
        };
        self.users
            .lock()
            .unwrap()
            .push((new.provider.clone(), new.subject.clone(), user.clone()));
        Ok(user)
    }

    fn update_profile(
        &self,
        id: &str,
        email: &str,
        name: &str,
        avatar_url: Option<&str>,
    ) -> Result<(), StoreError> {
        for u in self
            .users
            .lock()
            .unwrap()
            .iter_mut()
            .filter(|u| u.2.id == id)
        {
            u.2.email = email.into();
            u.2.name = name.into();
            u.2.avatar_url = avatar_url.map(Into::into);
        }
        Ok(())
    }

    fn set_access(&self, id: &str, role: Role, status: UserStatus) -> Result<(), StoreError> {
        for u in self
            .users
            .lock()
            .unwrap()
            .iter_mut()
            .filter(|u| u.2.id == id)
        {
            u.2.role = role;
            u.2.status = status;
        }
        Ok(())
    }

    fn has_admin(&self) -> Result<bool, StoreError> {
        Ok(self
            .users
            .lock()
            .unwrap()
            .iter()
            .any(|u| u.2.role == Role::Admin && u.2.status == UserStatus::Active))
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
            .insert(token_hash.into(), (user.into(), expires_at));
        Ok(())
    }

    fn session_user(&self, token_hash: &str, now: u64) -> Result<Option<User>, StoreError> {
        let user = self
            .sessions
            .lock()
            .unwrap()
            .get(token_hash)
            .filter(|(_, expires)| *expires > now)
            .map(|(user, _)| user.clone());
        match user {
            Some(id) => self.user(&id),
            None => Ok(None),
        }
    }

    fn delete_session(&self, token_hash: &str) -> Result<(), StoreError> {
        self.sessions.lock().unwrap().remove(token_hash);
        Ok(())
    }

    fn set_setup_token(&self, token_hash: &str) -> Result<(), StoreError> {
        *self.setup.lock().unwrap() = Some(token_hash.into());
        Ok(())
    }

    fn take_setup_token(&self, token_hash: &str) -> Result<bool, StoreError> {
        let mut setup = self.setup.lock().unwrap();
        if setup.as_deref() == Some(token_hash) {
            *setup = None;
            return Ok(true);
        }
        Ok(false)
    }

    fn allow_rules(&self) -> Result<Vec<AllowRule>, StoreError> {
        Ok(self.rules.lock().unwrap().clone())
    }

    fn remove_allow_rule(&self, id: i64) -> Result<bool, StoreError> {
        let mut rules = self.rules.lock().unwrap();
        let before = rules.len();
        rules.retain(|r| r.id != id);
        Ok(rules.len() < before)
    }

    fn insert_cli_token(
        &self,
        id: &str,
        user: &str,
        name: &str,
        token_hash: &str,
        via: &str,
    ) -> Result<CliToken, StoreError> {
        let token = CliToken {
            id: id.into(),
            name: name.into(),
            via: via.into(),
            created_at: "2026-10-19T00:00:00Z".into(),
            last_used_at: None,
        };
        self.tokens
            .lock()
            .unwrap()
            .push((token_hash.into(), user.into(), token.clone()));
        Ok(token)
    }

    fn cli_tokens(&self, user: &str) -> Result<Vec<CliToken>, StoreError> {
        Ok(self
            .tokens
            .lock()
            .unwrap()
            .iter()
            .filter(|t| t.1 == user)
            .map(|t| t.2.clone())
            .collect())
    }

    fn delete_cli_token(&self, user: &str, id: &str) -> Result<bool, StoreError> {
        let mut tokens = self.tokens.lock().unwrap();
        let before = tokens.len();
        tokens.retain(|t| !(t.1 == user && t.2.id == id));
        Ok(tokens.len() < before)
    }

    fn delete_cli_token_by_hash(&self, token_hash: &str) -> Result<(), StoreError> {
        self.tokens.lock().unwrap().retain(|t| t.0 != token_hash);
        Ok(())
    }

    fn token_user(&self, token_hash: &str) -> Result<Option<User>, StoreError> {
        let user = self
            .tokens
            .lock()
            .unwrap()
            .iter()
            .find(|t| t.0 == token_hash)
            .map(|t| t.1.clone());
        match user {
            Some(id) => self.user(&id),
            None => Ok(None),
        }
    }

    fn insert_device(
        &self,
        code_hash: &str,
        user_code: &str,
        name: &str,
        expires_at: u64,
    ) -> Result<(), StoreError> {
        self.devices.lock().unwrap().insert(
            code_hash.into(),
            Device {
                user_code: user_code.into(),
                name: name.into(),
                status: "pending".into(),
                user: None,
                expires_at,
            },
        );
        Ok(())
    }

    fn device_by_user_code(&self, user_code: &str) -> Result<Option<Device>, StoreError> {
        Ok(self
            .devices
            .lock()
            .unwrap()
            .values()
            .find(|d| d.user_code == user_code)
            .cloned())
    }

    fn device_by_code(&self, code_hash: &str) -> Result<Option<Device>, StoreError> {
        Ok(self.devices.lock().unwrap().get(code_hash).cloned())
    }

    fn decide_device(
        &self,
        user_code: &str,
        user: &str,
        approve: bool,
    ) -> Result<bool, StoreError> {
        let mut devices = self.devices.lock().unwrap();
        let Some(device) = devices
            .values_mut()
            .find(|d| d.user_code == user_code && d.status == "pending")
        else {
            return Ok(false);
        };
        device.status = if approve { "approved" } else { "denied" }.into();
        device.user = Some(user.into());
        Ok(true)
    }

    fn delete_device(&self, code_hash: &str) -> Result<(), StoreError> {
        self.devices.lock().unwrap().remove(code_hash);
        Ok(())
    }

    fn insert_share_link(&self, link: &NewShareLink) -> Result<ShareLink, StoreError> {
        let created = ShareLink {
            id: link.id.clone(),
            project: link.project.clone(),
            role: link.role,
            label: link.label.clone(),
            created_by: link.created_by.clone(),
            created_at: "2026-10-19T00:00:00Z".into(),
            revoked_at: None,
            guests: 0,
        };
        self.links
            .lock()
            .unwrap()
            .push((link.token_hash.clone(), created.clone()));
        Ok(created)
    }

    fn share_links(&self, project: &str) -> Result<Vec<ShareLink>, StoreError> {
        let guests = self.guests.lock().unwrap();
        Ok(self
            .links
            .lock()
            .unwrap()
            .iter()
            .rev()
            .filter(|(_, l)| l.project == project)
            .map(|(_, l)| ShareLink {
                guests: u32::try_from(guests.iter().filter(|(_, link)| *link == l.id).count())
                    .unwrap(),
                ..l.clone()
            })
            .collect())
    }

    fn share_link_by_token(&self, token_hash: &str) -> Result<Option<ShareLink>, StoreError> {
        Ok(self
            .links
            .lock()
            .unwrap()
            .iter()
            .find(|(h, _)| h == token_hash)
            .map(|(_, l)| l.clone()))
    }

    fn revoke_share_link(&self, project: &str, id: &str) -> Result<bool, StoreError> {
        let mut links = self.links.lock().unwrap();
        let Some((_, link)) = links
            .iter_mut()
            .find(|(_, l)| l.project == project && l.id == id && l.revoked_at.is_none())
        else {
            return Ok(false);
        };
        link.revoked_at = Some("2026-10-19T01:00:00Z".into());
        let gone: Vec<String> = self
            .guests
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, l)| l == id)
            .map(|(g, _)| g.id.clone())
            .collect();
        self.guest_sessions
            .lock()
            .unwrap()
            .retain(|_, (guest, _)| !gone.contains(guest));
        Ok(true)
    }

    fn insert_guest(&self, id: &str, link: &str, name: &str) -> Result<(), StoreError> {
        self.guests.lock().unwrap().push((
            Guest {
                id: id.into(),
                name: name.into(),
            },
            link.into(),
        ));
        Ok(())
    }

    fn insert_guest_session(
        &self,
        token_hash: &str,
        guest: &str,
        expires_at: u64,
    ) -> Result<(), StoreError> {
        self.guest_sessions
            .lock()
            .unwrap()
            .insert(token_hash.into(), (guest.into(), expires_at));
        Ok(())
    }

    fn guest_session(
        &self,
        token_hash: &str,
        now: u64,
    ) -> Result<Option<(Guest, ShareLink)>, StoreError> {
        let Some((guest_id, _)) = self
            .guest_sessions
            .lock()
            .unwrap()
            .get(token_hash)
            .filter(|(_, expires)| *expires > now)
            .cloned()
        else {
            return Ok(None);
        };
        let Some((guest, link_id)) = self
            .guests
            .lock()
            .unwrap()
            .iter()
            .find(|(g, _)| g.id == guest_id)
            .cloned()
        else {
            return Ok(None);
        };
        let link = self
            .links
            .lock()
            .unwrap()
            .iter()
            .find(|(_, l)| l.id == link_id && l.revoked_at.is_none())
            .map(|(_, l)| l.clone());
        Ok(link.map(|link| (guest, link)))
    }

    fn users(&self) -> Result<Vec<User>, StoreError> {
        Ok(self
            .users
            .lock()
            .unwrap()
            .iter()
            .map(|u| u.2.clone())
            .collect())
    }

    fn add_allow_rule(&self, kind: &str, value: &str) -> Result<AllowRule, StoreError> {
        let mut rules = self.rules.lock().unwrap();
        if let Some(rule) = rules.iter().find(|r| r.kind == kind && r.value == value) {
            return Ok(rule.clone());
        }
        let rule = AllowRule {
            id: rules.iter().map(|r| r.id).max().unwrap_or(0) + 1,
            kind: kind.into(),
            value: value.into(),
        };
        rules.push(rule.clone());
        Ok(rule)
    }
}

/// An identity provider that answers what the test told it to.
#[derive(Default)]
pub struct FakeIdentity {
    requests: Mutex<Vec<AuthRequest>>,
    claims: Mutex<Option<Claims>>,
}

impl FakeIdentity {
    /// The `state` of the last sign-in the engine started.
    pub fn last_state(&self) -> String {
        self.requests.lock().unwrap().last().unwrap().state.clone()
    }

    /// The claims the next exchange returns.
    pub fn next(&self, subject: &str, email: &str, verified: bool) {
        *self.claims.lock().unwrap() = Some(Claims {
            subject: subject.into(),
            email: email.into(),
            email_verified: verified,
            name: None,
            picture: None,
        });
    }
}

impl Identity for FakeIdentity {
    fn provider(&self) -> &'static str {
        "fake"
    }

    fn authorize_url(&self, request: &AuthRequest) -> Result<String, IdpError> {
        self.requests.lock().unwrap().push(request.clone());
        Ok(format!("https://idp.test/auth?state={}", request.state))
    }

    fn exchange(&self, _code: &str, _verifier: &str, _nonce: &str) -> Result<Claims, IdpError> {
        self.claims
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| IdpError::Failed("no claims scripted".into()))
    }
}
