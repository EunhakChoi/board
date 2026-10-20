//! SQLite persistence for members, projects, comments and notifications.

mod accounts;
mod branches;
mod captures;
mod comments;
mod notices;

use std::path::Path;
use std::sync::Mutex;

use charet_engine::{BranchEvent, NewBranch, NewVersion, Records, Settings, StoreError};
use charet_model::{Branch, Origin, Project, Version, VersionKind, Visibility, VisitorAccess};
use rusqlite::{Connection, OptionalExtension, Row, params};

pub use captures::CaptureStore;
pub use comments::CommentStore;

/// Schema migrations, applied in order. `PRAGMA user_version` records how many have run.
/// Never edit a released migration; append a new one.
const MIGRATIONS: &[&str] = &[
    include_str!("migrations/0001_settings.sql"),
    include_str!("migrations/0002_projects.sql"),
    include_str!("migrations/0003_captures.sql"),
    include_str!("migrations/0004_comments.sql"),
    include_str!("migrations/0005_accounts.sql"),
    include_str!("migrations/0006_share_links.sql"),
    include_str!("migrations/0007_cli_tokens.sql"),
    include_str!("migrations/0008_project_members.sql"),
    include_str!("migrations/0009_notices.sql"),
    include_str!("migrations/0010_invites.sql"),
    include_str!("migrations/0011_member_tokens.sql"),
    include_str!("migrations/0012_branches.sql"),
    include_str!("migrations/0013_branch_comments.sql"),
    include_str!("migrations/0014_merge_guards.sql"),
    include_str!("migrations/0015_handoff.sql"),
    include_str!("migrations/0016_visitor_access.sql"),
];

/// The charet database: one SQLite file in WAL mode.
#[derive(Debug)]
pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    /// Opens (or creates) the database at `path` and runs pending migrations.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let conn = Connection::open(path).map_err(err)?;
        Self::init(conn)
    }

    /// An in-memory database, for tests.
    pub fn memory() -> Result<Self, StoreError> {
        Self::init(Connection::open_in_memory().map_err(err)?)
    }

    fn init(conn: Connection) -> Result<Self, StoreError> {
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(err)?;
        conn.pragma_update(None, "foreign_keys", true)
            .map_err(err)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(err)?;
        migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// How many migrations have been applied.
    pub fn schema_version(&self) -> Result<usize, StoreError> {
        user_version(&*self.lock()?)
    }

    pub(crate) fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, StoreError> {
        self.conn
            .lock()
            .map_err(|_| StoreError("database lock poisoned".into()))
    }
}

fn user_version(conn: &Connection) -> Result<usize, StoreError> {
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(err)?;
    usize::try_from(version).map_err(err)
}

fn migrate(conn: &Connection) -> Result<(), StoreError> {
    let applied = user_version(conn)?;
    if applied > MIGRATIONS.len() {
        return Err(StoreError(format!(
            "database schema {applied} is newer than this charet ({}); upgrade charet",
            MIGRATIONS.len()
        )));
    }
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(applied) {
        conn.execute_batch(&format!(
            "BEGIN;\n{sql}\nPRAGMA user_version = {};\nCOMMIT;",
            index + 1
        ))
        .map_err(err)?;
    }
    Ok(())
}

impl Settings for Db {
    fn get(&self, key: &str) -> Result<Option<String>, StoreError> {
        self.lock()?
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(err)
    }

    fn set(&self, key: &str, value: &str) -> Result<(), StoreError> {
        self.lock()?
            .execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map(drop)
            .map_err(err)
    }
}

impl Records for Db {
    fn update_project(
        &self,
        key: &str,
        title: &str,
        visibility: Visibility,
    ) -> Result<(), StoreError> {
        self.lock()?
            .execute(
                "UPDATE projects SET title = ?2, visibility = ?3 WHERE key = ?1",
                params![key, title, text(visibility)],
            )
            .map(drop)
            .map_err(err)
    }

    fn set_project_visitors(
        &self,
        key: &str,
        visitors: Option<VisitorAccess>,
    ) -> Result<(), StoreError> {
        self.lock()?
            .execute(
                "UPDATE projects SET visitors = ?2 WHERE key = ?1",
                params![key, visitors.map(text)],
            )
            .map(drop)
            .map_err(err)
    }

    fn members(&self, project: &str) -> Result<Vec<String>, StoreError> {
        let conn = self.lock()?;
        let mut statement = conn
            .prepare("SELECT user_id FROM project_members WHERE project = ?1 ORDER BY user_id")
            .map_err(err)?;
        let rows = statement
            .query_map(params![project], |row| row.get(0))
            .map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }

    fn is_member(&self, project: &str, user: &str) -> Result<bool, StoreError> {
        self.lock()?
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM project_members WHERE project = ?1 AND user_id = ?2)",
                params![project, user],
                |row| row.get(0),
            )
            .map_err(err)
    }

    fn set_member(&self, project: &str, user: &str, member: bool) -> Result<(), StoreError> {
        let sql = if member {
            "INSERT INTO project_members (project, user_id) VALUES (?1, ?2) ON CONFLICT DO NOTHING"
        } else {
            "DELETE FROM project_members WHERE project = ?1 AND user_id = ?2"
        };
        self.lock()?
            .execute(sql, params![project, user])
            .map(drop)
            .map_err(err)
    }

    fn delete_project(&self, key: &str) -> Result<bool, StoreError> {
        // Versions, captures, comments and replies go with it (ON DELETE CASCADE).
        let deleted = self
            .lock()?
            .execute("DELETE FROM projects WHERE key = ?1", params![key])
            .map_err(err)?;
        Ok(deleted == 1)
    }

    fn insert_project(&self, project: &Project) -> Result<bool, StoreError> {
        let inserted = self
            .lock()?
            .execute(
                "INSERT INTO projects (key, title, visibility, origin) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (key) DO NOTHING",
                params![
                    project.key,
                    project.title,
                    text(project.visibility),
                    text(project.origin)
                ],
            )
            .map_err(err)?;
        Ok(inserted == 1)
    }

    fn project(&self, key: &str) -> Result<Option<Project>, StoreError> {
        self.lock()?
            .query_row(
                &format!("{PROJECT_COLUMNS} WHERE key = ?1"),
                params![key],
                project_row,
            )
            .optional()
            .map_err(err)
    }

    fn projects(&self) -> Result<Vec<Project>, StoreError> {
        let conn = self.lock()?;
        let mut statement = conn
            .prepare(&format!("{PROJECT_COLUMNS} ORDER BY key"))
            .map_err(err)?;
        let rows = statement.query_map([], project_row).map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }

    fn add_version(&self, project: &str, version: &NewVersion) -> Result<Version, StoreError> {
        let conn = self.lock()?;
        // One statement picks the next number, so concurrent writers cannot collide.
        conn.query_row(
            &format!(
                "INSERT INTO versions
                   (project, number, git_commit, kind, actor, note, branch, restored, source_commit,
                    source_tree)
                 VALUES (?1, (SELECT COALESCE(MAX(number), 0) + 1 FROM versions WHERE project = ?1),
                         ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 RETURNING {VERSION_FIELDS}"
            ),
            params![
                project,
                version.commit,
                text(version.kind),
                version.actor,
                version.note,
                version.branch,
                version.restored,
                version.source_commit,
                version.source_tree
            ],
            version_row,
        )
        .map_err(err)
    }

    fn insert_branch(&self, project: &str, branch: &NewBranch) -> Result<Branch, StoreError> {
        self.insert_branch_row(project, branch)
    }

    fn branch(&self, project: &str, number: u32) -> Result<Option<Branch>, StoreError> {
        self.branch_row(project, number)
    }

    fn branches(&self, project: &str) -> Result<Vec<Branch>, StoreError> {
        self.branch_rows(project)
    }

    fn update_branch(&self, project: &str, branch: &Branch) -> Result<(), StoreError> {
        self.update_branch_row(project, branch)
    }

    fn add_branch_event(
        &self,
        project: &str,
        number: u32,
        event: &BranchEvent,
    ) -> Result<(), StoreError> {
        self.add_branch_event_row(project, number, event)
    }

    fn branch_events(&self, project: &str, number: u32) -> Result<Vec<BranchEvent>, StoreError> {
        self.branch_event_rows(project, number)
    }

    fn mark_handoff(
        &self,
        project: &str,
        number: u32,
        commit: &str,
        actor: &str,
    ) -> Result<bool, StoreError> {
        let changed = self
            .lock()?
            .execute(
                "UPDATE versions SET handoff_commit = ?3, handoff_actor = ?4,
                   handoff_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE project = ?1 AND number = ?2",
                params![project, number, commit, actor],
            )
            .map_err(err)?;
        Ok(changed > 0)
    }

    fn versions(&self, project: &str) -> Result<Vec<Version>, StoreError> {
        let conn = self.lock()?;
        let mut statement = conn
            .prepare(&format!(
                "SELECT {VERSION_FIELDS} FROM versions WHERE project = ?1 ORDER BY number DESC"
            ))
            .map_err(err)?;
        let rows = statement
            .query_map(params![project], version_row)
            .map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }
}

const PROJECT_COLUMNS: &str =
    "SELECT key, title, visibility, origin, created_at, visitors FROM projects";
const VERSION_FIELDS: &str = "number, git_commit, kind, actor, note, created_at, branch, restored, \
     source_commit, handoff_commit, handoff_actor, handoff_at, source_tree";

fn project_row(row: &Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        key: row.get(0)?,
        title: row.get(1)?,
        visibility: parse::<Visibility>(row, 2)?,
        visitors: row
            .get::<_, Option<String>>(5)?
            .and_then(|text| serde_json::from_value(serde_json::Value::String(text)).ok()),
        visitor_access: VisitorAccess::View,
        origin: parse::<Origin>(row, 3)?,
        created_at: row.get(4)?,
    })
}

fn version_row(row: &Row<'_>) -> rusqlite::Result<Version> {
    Ok(Version {
        number: row.get(0)?,
        commit: row.get(1)?,
        kind: parse::<VersionKind>(row, 2)?,
        actor: row.get(3)?,
        note: row.get(4)?,
        created_at: row.get(5)?,
        branch: row.get(6)?,
        restored: row.get(7)?,
        source_commit: row.get(8)?,
        source_tree: row.get(12)?,
        handoff: match row.get::<_, Option<String>>(9)? {
            Some(commit) => Some(charet_model::Handoff {
                commit,
                actor: row.get::<_, Option<String>>(10)?.unwrap_or_default(),
                at: row.get::<_, Option<String>>(11)?.unwrap_or_default(),
            }),
            None => None,
        },
    })
}

/// Stores a model enum as its serde name (`"team"`, `"import"`, ...).
fn text(value: impl serde::Serialize) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(text)) => text,
        _ => String::new(),
    }
}

fn parse<T: serde::de::DeserializeOwned>(row: &Row<'_>, index: usize) -> rusqlite::Result<T> {
    let text: String = row.get(index)?;
    serde_json::from_value(serde_json::Value::String(text)).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

/// Deletes a directory tree; a missing one is fine.
pub(crate) fn remove_dir(dir: &Path) -> Result<(), StoreError> {
    match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(err(e)),
        _ => Ok(()),
    }
}

fn err(error: impl std::fmt::Display) -> StoreError {
    StoreError(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_run_once() {
        let db = Db::memory().unwrap();
        assert_eq!(db.schema_version().unwrap(), MIGRATIONS.len());
        migrate(&db.lock().unwrap()).unwrap();
        assert_eq!(db.schema_version().unwrap(), MIGRATIONS.len());
    }

    #[test]
    fn settings_upsert() {
        let db = Db::memory().unwrap();
        assert_eq!(db.get("a").unwrap(), None);
        db.set("a", "1").unwrap();
        db.set("a", "2").unwrap();
        assert_eq!(db.get("a").unwrap(), Some("2".into()));
    }

    #[test]
    fn refuses_a_newer_schema() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", 999).unwrap();
        assert!(
            Db::init(conn)
                .unwrap_err()
                .0
                .contains("newer than this charet")
        );
    }

    fn project(key: &str) -> Project {
        Project {
            key: key.into(),
            title: key.to_uppercase(),
            visibility: Visibility::Team,
            visitors: None,
            visitor_access: VisitorAccess::View,
            origin: Origin::Import,
            created_at: String::new(),
        }
    }

    #[test]
    fn projects_are_unique_and_listed() {
        let db = Db::memory().unwrap();
        assert!(db.insert_project(&project("b")).unwrap());
        assert!(db.insert_project(&project("a")).unwrap());
        assert!(!db.insert_project(&project("a")).unwrap());
        let found = db.project("a").unwrap().unwrap();
        assert_eq!((found.title.as_str(), found.origin), ("A", Origin::Import));
        assert!(found.created_at.ends_with('Z'));
        let keys: Vec<_> = db.projects().unwrap().into_iter().map(|p| p.key).collect();
        assert_eq!(keys, ["a", "b"]);
        assert_eq!(db.project("zzz").unwrap(), None);
    }

    fn nv(commit: &str, kind: VersionKind, note: &str) -> NewVersion {
        NewVersion {
            commit: commit.into(),
            kind,
            actor: "Me".into(),
            note: note.into(),
            branch: None,
            restored: None,
            source_commit: None,
            source_tree: None,
        }
    }

    #[test]
    fn handoff_marks_and_import_sources() {
        let db = Db::memory().unwrap();
        db.insert_project(&project("a")).unwrap();
        let v1 = db
            .add_version(
                "a",
                &NewVersion {
                    source_commit: Some("abc123".into()),
                    ..nv("c1", VersionKind::Import, "first")
                },
            )
            .unwrap();
        assert_eq!(v1.source_commit.as_deref(), Some("abc123"));
        assert_eq!(v1.handoff, None);
        assert!(db.mark_handoff("a", 1, "def456", "Ann").unwrap());
        assert!(!db.mark_handoff("a", 9, "def456", "Ann").unwrap());
        let marked = db.versions("a").unwrap().remove(0).handoff.unwrap();
        assert_eq!(
            (marked.commit.as_str(), marked.actor.as_str()),
            ("def456", "Ann")
        );
        assert!(marked.at.starts_with("20"), "{}", marked.at);
    }

    #[test]
    fn branches_number_update_and_log() {
        let db = Db::memory().unwrap();
        db.insert_project(&project("a")).unwrap();
        let me = charet_model::Person {
            id: "u1".into(),
            name: "Ann".into(),
        };
        let new = NewBranch {
            title: "Bigger buttons".into(),
            author: me.clone(),
            base: "c1".into(),
            head: "c1".into(),
        };
        let b1 = db.insert_branch("a", &new).unwrap();
        let b2 = db.insert_branch("a", &new).unwrap();
        assert_eq!((b1.number, b2.number), (1, 2));
        assert_eq!(b1.status, charet_model::BranchStatus::Open);
        let mut changed = b1.clone();
        changed.status = charet_model::BranchStatus::Merged;
        changed.head = "c9".into();
        changed.conflicts = vec!["x.ts".into()];
        changed.merged_version = Some(4);
        db.update_branch("a", &changed).unwrap();
        let found = db.branch("a", 1).unwrap().unwrap();
        assert_eq!(
            (
                found.status,
                found.head.as_str(),
                found.conflicts.len(),
                found.merged_version
            ),
            (charet_model::BranchStatus::Merged, "c9", 1, Some(4))
        );
        assert_eq!(
            db.branches("a")
                .unwrap()
                .iter()
                .map(|b| b.number)
                .collect::<Vec<_>>(),
            [2, 1]
        );
        let event = BranchEvent {
            actor: me,
            kind: "push".into(),
            detail: "c9".into(),
            at: String::new(),
        };
        db.add_branch_event("a", 1, &event).unwrap();
        assert_eq!(db.branch_events("a", 1).unwrap()[0].kind, "push");
        let merged = db
            .add_version(
                "a",
                &NewVersion {
                    branch: Some(1),
                    ..nv("c9", VersionKind::Merge, "Bigger buttons")
                },
            )
            .unwrap();
        assert_eq!(merged.branch, Some(1));
        assert!(db.delete_project("a").unwrap());
        assert_eq!(db.branch("a", 1).unwrap(), None);
    }

    #[test]
    fn versions_count_per_project() {
        let db = Db::memory().unwrap();
        db.insert_project(&project("a")).unwrap();
        db.insert_project(&project("b")).unwrap();
        let v1 = db
            .add_version("a", &nv("c1", VersionKind::Import, "first"))
            .unwrap();
        let v2 = db
            .add_version("a", &nv("c2", VersionKind::Push, "second"))
            .unwrap();
        let b1 = db
            .add_version("b", &nv("c3", VersionKind::Initial, ""))
            .unwrap();
        assert_eq!((v1.number, v2.number, b1.number), (1, 2, 1));
        let listed: Vec<_> = db
            .versions("a")
            .unwrap()
            .into_iter()
            .map(|v| (v.number, v.kind))
            .collect();
        assert_eq!(listed, [(2, VersionKind::Push), (1, VersionKind::Import)]);
        assert!(
            db.add_version("nope", &nv("c", VersionKind::Push, ""))
                .is_err()
        );
        assert!(db.delete_project("a").unwrap());
        assert!(!db.delete_project("a").unwrap());
        assert_eq!(db.versions("a").unwrap(), Vec::new());
        assert_eq!(db.versions("b").unwrap().len(), 1);
    }

    #[test]
    fn opens_a_file() {
        let dir = std::env::temp_dir().join(format!("charet-db-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("charet.db");
        Db::open(&path).unwrap().set("k", "v").unwrap();
        assert_eq!(Db::open(&path).unwrap().get("k").unwrap(), Some("v".into()));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
