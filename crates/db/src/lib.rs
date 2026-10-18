//! SQLite persistence for people, access, branches, comments and jobs.

use std::path::Path;
use std::sync::Mutex;

use board_engine::{Settings, StoreError};
use rusqlite::{Connection, OptionalExtension, params};

/// Schema migrations, applied in order. `PRAGMA user_version` records how many have run.
/// Never edit a released migration; append a new one.
const MIGRATIONS: &[&str] = &[include_str!("migrations/0001_settings.sql")];

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

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, StoreError> {
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
