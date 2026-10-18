//! In-memory ports for engine tests.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use board_model::{Comment, CommentStatus, Project, Reply, Version, VersionKind};

use crate::{
    Author, Build, BuildState, BuiltFile, Capture, Captures, Change, ChangeKind, Comments, Engine,
    FileChange, NewComment, NewReply, Ports, PreviewSettings, ProjectStore, Records, RenderJob,
    Renderer, Settings, Shot, StoreError, Workspace, WorkspaceError,
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
}

impl Records for MemoryRecords {
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
