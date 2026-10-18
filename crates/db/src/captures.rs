//! Captures: one row per (project, kind, target, theme) in SQLite, the image and element map as
//! files under `<dir>/<project>/`.

use std::path::PathBuf;
use std::sync::Arc;

use board_engine::{Capture, Captures, StoreError};
use rusqlite::{OptionalExtension, params};

use crate::{Db, err, remove_dir};

/// [`Captures`] over the board database and a directory for the files.
#[derive(Debug, Clone)]
pub struct CaptureStore {
    db: Arc<Db>,
    dir: PathBuf,
}

impl CaptureStore {
    pub fn new(db: Arc<Db>, dir: impl Into<PathBuf>) -> Self {
        Self {
            db,
            dir: dir.into(),
        }
    }

    /// `<dir>/<project>/<kind>-<target>.<theme>.<ext>`. Every part is a checked id.
    fn file(
        &self,
        project: &str,
        kind: &str,
        target: &str,
        theme: &str,
        ext: &str,
    ) -> Result<PathBuf, StoreError> {
        for part in [project, kind, target, theme] {
            if !id(part) {
                return Err(StoreError(format!("bad capture name part {part:?}")));
            }
        }
        Ok(self
            .dir
            .join(project)
            .join(format!("{kind}-{target}.{theme}.{ext}")))
    }
}

fn id(text: &str) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 100
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// Writes through a temporary file so readers never see half a file.
fn write(path: &PathBuf, bytes: &[u8]) -> Result<(), StoreError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(err)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(err)?;
    std::fs::rename(&tmp, path).map_err(err)
}

fn read(path: &PathBuf) -> Result<Option<Vec<u8>>, StoreError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(err(error)),
    }
}

impl Captures for CaptureStore {
    fn remove_project(&self, project: &str) -> Result<(), StoreError> {
        if !id(project) {
            return Err(StoreError(format!("bad project key {project:?}")));
        }
        remove_dir(&self.dir.join(project))
    }

    fn put(
        &self,
        project: &str,
        capture: &Capture,
        image: &[u8],
        elements: &serde_json::Value,
    ) -> Result<(), StoreError> {
        let (kind, target, theme) = (&capture.kind, &capture.target, &capture.theme);
        write(&self.file(project, kind, target, theme, "jpg")?, image)?;
        let map = serde_json::to_vec(elements).map_err(err)?;
        write(&self.file(project, kind, target, theme, "json")?, &map)?;
        self.db
            .lock()?
            .execute(
                "INSERT INTO captures (project, kind, target, theme, git_commit, width, height, issues, sources)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT (project, kind, target, theme) DO UPDATE SET
                   git_commit = excluded.git_commit, width = excluded.width, height = excluded.height,
                   issues = excluded.issues, sources = excluded.sources,
                   captured_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
                params![
                    project,
                    kind,
                    target,
                    theme,
                    capture.commit,
                    capture.width,
                    capture.height,
                    serde_json::to_string(&capture.issues).map_err(err)?,
                    serde_json::to_string(&capture.sources).map_err(err)?,
                ],
            )
            .map(drop)
            .map_err(err)
    }

    fn list(&self, project: &str) -> Result<Vec<Capture>, StoreError> {
        let conn = self.db.lock()?;
        let mut statement = conn
            .prepare(
                "SELECT kind, target, theme, git_commit, width, height, issues, sources, captured_at
                 FROM captures WHERE project = ?1 ORDER BY kind, target, theme",
            )
            .map_err(err)?;
        let rows = statement
            .query_map(params![project], |row| {
                Ok((
                    Capture {
                        kind: row.get(0)?,
                        target: row.get(1)?,
                        theme: row.get(2)?,
                        commit: row.get(3)?,
                        width: row.get(4)?,
                        height: row.get(5)?,
                        issues: Vec::new(),
                        sources: Vec::new(),
                        captured_at: row.get(8)?,
                    },
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                ))
            })
            .map_err(err)?;
        rows.map(|row| {
            let (mut capture, issues, sources) = row.map_err(err)?;
            capture.issues = serde_json::from_str(&issues).map_err(err)?;
            capture.sources = serde_json::from_str(&sources).map_err(err)?;
            Ok(capture)
        })
        .collect()
    }

    fn image(
        &self,
        project: &str,
        kind: &str,
        target: &str,
        theme: &str,
    ) -> Result<Option<Vec<u8>>, StoreError> {
        if !self.exists(project, kind, target, theme)? {
            return Ok(None);
        }
        read(&self.file(project, kind, target, theme, "jpg")?)
    }

    fn elements(
        &self,
        project: &str,
        kind: &str,
        target: &str,
        theme: &str,
    ) -> Result<Option<serde_json::Value>, StoreError> {
        if !self.exists(project, kind, target, theme)? {
            return Ok(None);
        }
        read(&self.file(project, kind, target, theme, "json")?)?
            .map(|bytes| serde_json::from_slice(&bytes).map_err(err))
            .transpose()
    }
}

impl CaptureStore {
    fn exists(
        &self,
        project: &str,
        kind: &str,
        target: &str,
        theme: &str,
    ) -> Result<bool, StoreError> {
        self.db
            .lock()?
            .query_row(
                "SELECT 1 FROM captures WHERE project = ?1 AND kind = ?2 AND target = ?3 AND theme = ?4",
                params![project, kind, target, theme],
                |_| Ok(()),
            )
            .optional()
            .map(|row| row.is_some())
            .map_err(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use board_engine::Records;
    use board_model::{Origin, Project, Visibility};

    #[test]
    fn stores_replaces_and_reads_back() {
        let db = Arc::new(Db::memory().unwrap());
        db.insert_project(&Project {
            key: "web".into(),
            title: "Web".into(),
            visibility: Visibility::Team,
            origin: Origin::Empty,
            created_at: String::new(),
        })
        .unwrap();
        let dir = std::env::temp_dir().join(format!("board-captures-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = CaptureStore::new(db, &dir);
        let mut capture = Capture {
            kind: "screen".into(),
            target: "list".into(),
            theme: "light".into(),
            commit: "c1".into(),
            width: 800,
            height: 600,
            issues: vec!["error: boom".into()],
            sources: vec!["src/a.tsx".into()],
            captured_at: String::new(),
        };
        let map = serde_json::json!([{ "tag": "main" }]);
        store.put("web", &capture, b"one", &map).unwrap();
        capture.commit = "c2".into();
        capture.issues.clear();
        store.put("web", &capture, b"two", &map).unwrap();

        let list = store.list("web").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].commit, "c2");
        assert_eq!(list[0].issues, Vec::<String>::new());
        assert_eq!(list[0].sources, ["src/a.tsx"]);
        assert_ne!(list[0].captured_at, "");
        assert_eq!(
            store
                .image("web", "screen", "list", "light")
                .unwrap()
                .as_deref(),
            Some(&b"two"[..])
        );
        assert_eq!(
            store.elements("web", "screen", "list", "light").unwrap(),
            Some(map)
        );
        assert_eq!(store.image("web", "screen", "list", "dark").unwrap(), None);
        assert!(
            store
                .image("web", "screen", "../x", "light")
                .unwrap()
                .is_none()
        );
        capture.target = "../escape".into();
        assert!(
            store
                .put("web", &capture, b"x", &serde_json::Value::Null)
                .is_err()
        );

        // Deleting the project drops its rows (cascade) and its files.
        store.db.delete_project("web").unwrap();
        store.remove_project("web").unwrap();
        assert_eq!(store.list("web").unwrap(), Vec::new());
        assert!(!dir.join("web").exists());
        assert!(store.remove_project("../x").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
