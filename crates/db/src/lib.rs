//! SQLite persistence for people, access, branches, comments and jobs.

mod accounts;
mod captures;
mod comments;
mod notices;

use std::path::Path;
use std::sync::Mutex;

use board_engine::{Records, Settings, StoreError};
use board_model::{Origin, Project, Version, VersionKind, Visibility};
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
];

/// The board database: one SQLite file in WAL mode.
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
            "database schema {applied} is newer than this board ({}); upgrade board",
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

    fn add_version(
        &self,
        project: &str,
        commit: &str,
        kind: VersionKind,
        actor: &str,
        note: &str,
    ) -> Result<Version, StoreError> {
        let conn = self.lock()?;
        // One statement picks the next number, so concurrent writers cannot collide.
        conn.query_row(
            &format!(
                "INSERT INTO versions (project, number, git_commit, kind, actor, note)
                 VALUES (?1, (SELECT COALESCE(MAX(number), 0) + 1 FROM versions WHERE project = ?1),
                         ?2, ?3, ?4, ?5)
                 RETURNING {VERSION_FIELDS}"
            ),
            params![project, commit, text(kind), actor, note],
            version_row,
        )
        .map_err(err)
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

const PROJECT_COLUMNS: &str = "SELECT key, title, visibility, origin, created_at FROM projects";
const VERSION_FIELDS: &str = "number, git_commit, kind, actor, note, created_at";

fn project_row(row: &Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        key: row.get(0)?,
        title: row.get(1)?,
        visibility: parse::<Visibility>(row, 2)?,
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
                .contains("newer than this board")
        );
    }

    fn project(key: &str) -> Project {
        Project {
            key: key.into(),
            title: key.to_uppercase(),
            visibility: Visibility::Team,
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

    #[test]
    fn versions_count_per_project() {
        let db = Db::memory().unwrap();
        db.insert_project(&project("a")).unwrap();
        db.insert_project(&project("b")).unwrap();
        let v1 = db
            .add_version("a", "c1", VersionKind::Import, "Me", "first")
            .unwrap();
        let v2 = db
            .add_version("a", "c2", VersionKind::Push, "Me", "second")
            .unwrap();
        let b1 = db
            .add_version("b", "c3", VersionKind::Initial, "Me", "")
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
            db.add_version("nope", "c", VersionKind::Push, "Me", "")
                .is_err()
        );
        assert!(db.delete_project("a").unwrap());
        assert!(!db.delete_project("a").unwrap());
        assert_eq!(db.versions("a").unwrap(), Vec::new());
        assert_eq!(db.versions("b").unwrap().len(), 1);
    }

    #[test]
    fn opens_a_file() {
        let dir = std::env::temp_dir().join(format!("board-db-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("board.db");
        Db::open(&path).unwrap().set("k", "v").unwrap();
        assert_eq!(Db::open(&path).unwrap().get("k").unwrap(), Some("v".into()));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
