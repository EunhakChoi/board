//! Comments and replies in SQLite; each comment's capture snapshot as a file under
//! `<dir>/<project>/c<id>.jpg`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use charet_engine::{Comments, NewComment, NewReply, StoreError};
use charet_model::{Comment, CommentStatus, Person, Reply, ReplyKind};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{Db, err};

/// [`Comments`] over the charet database and a directory for snapshots.
#[derive(Debug, Clone)]
pub struct CommentStore {
    db: Arc<Db>,
    dir: PathBuf,
}

impl CommentStore {
    pub fn new(db: Arc<Db>, dir: impl Into<PathBuf>) -> Self {
        Self {
            db,
            dir: dir.into(),
        }
    }

    fn snapshot_path(&self, project: &str, id: u64) -> PathBuf {
        self.dir.join(project).join(format!("c{id}.jpg"))
    }
}

const COLUMNS: &str =
    "SELECT id, kind, target, theme, anchor, body, author_id, author_name, status,
    created_commit, resolved_commit, rev, created_at, updated_at, branch FROM comments";

/// SQLite integers are signed; ids are always positive.
fn row_id(row: &Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let id: i64 = row.get(index)?;
    u64::try_from(id).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Integer,
            Box::new(e),
        )
    })
}

fn sql_id(id: u64) -> i64 {
    i64::try_from(id).unwrap_or(i64::MAX)
}

fn status_text(status: CommentStatus) -> &'static str {
    match status {
        CommentStatus::Open => "open",
        CommentStatus::Resolved => "resolved",
    }
}

fn kind_text(kind: ReplyKind) -> &'static str {
    match kind {
        ReplyKind::Reply => "reply",
        ReplyKind::Resolve => "resolve",
        ReplyKind::Reopen => "reopen",
    }
}

fn comment_row(row: &Row<'_>) -> rusqlite::Result<(Comment, String)> {
    let status: String = row.get(8)?;
    Ok((
        Comment {
            id: row_id(row, 0)?,
            kind: row.get(1)?,
            target: row.get(2)?,
            theme: row.get(3)?,
            anchor: charet_model::Anchor::default(),
            body: row.get(5)?,
            author: Person {
                id: row.get(6)?,
                name: row.get(7)?,
            },
            status: if status == "resolved" {
                CommentStatus::Resolved
            } else {
                CommentStatus::Open
            },
            created_commit: row.get(9)?,
            resolved_commit: row.get(10)?,
            rev: row.get(11)?,
            created_at: row.get(12)?,
            updated_at: row.get(13)?,
            branch: row.get(14)?,
            replies: Vec::new(),
        },
        row.get(4)?,
    ))
}

fn reply_row(row: &Row<'_>) -> rusqlite::Result<(u64, Reply)> {
    let kind: String = row.get(5)?;
    Ok((
        row_id(row, 0)?,
        Reply {
            id: row_id(row, 1)?,
            author: Person {
                id: row.get(2)?,
                name: row.get(3)?,
            },
            body: row.get(4)?,
            kind: match kind.as_str() {
                "resolve" => ReplyKind::Resolve,
                "reopen" => ReplyKind::Reopen,
                _ => ReplyKind::Reply,
            },
            created_at: row.get(6)?,
        },
    ))
}

/// Comments matching `filter` (SQL after `WHERE`), with their replies.
fn load(
    conn: &Connection,
    filter: &str,
    args: &[&dyn rusqlite::ToSql],
) -> Result<Vec<Comment>, StoreError> {
    let mut statement = conn
        .prepare(&format!("{COLUMNS} WHERE {filter} ORDER BY id"))
        .map_err(err)?;
    let mut comments = Vec::new();
    for row in statement.query_map(args, comment_row).map_err(err)? {
        let (mut comment, anchor) = row.map_err(err)?;
        comment.anchor = serde_json::from_str(&anchor).map_err(err)?;
        comments.push(comment);
    }
    if comments.is_empty() {
        return Ok(comments);
    }
    let ids: Vec<String> = comments.iter().map(|c| c.id.to_string()).collect();
    let mut statement = conn
        .prepare(&format!(
            "SELECT comment, id, author_id, author_name, body, kind, created_at FROM replies
             WHERE comment IN ({}) ORDER BY id",
            ids.join(",")
        ))
        .map_err(err)?;
    let mut replies: HashMap<u64, Vec<Reply>> = HashMap::new();
    for row in statement.query_map([], reply_row).map_err(err)? {
        let (comment, reply) = row.map_err(err)?;
        replies.entry(comment).or_default().push(reply);
    }
    for comment in &mut comments {
        comment.replies = replies.remove(&comment.id).unwrap_or_default();
    }
    Ok(comments)
}

impl Comments for CommentStore {
    fn remove_project(&self, project: &str) -> Result<(), StoreError> {
        if !charet_model::Project::valid_key(project) {
            return Err(StoreError(format!("bad project key {project:?}")));
        }
        crate::remove_dir(&self.dir.join(project))
    }

    fn add(
        &self,
        project: &str,
        new: &NewComment,
        snapshot: Option<&[u8]>,
    ) -> Result<Comment, StoreError> {
        let anchor = serde_json::to_string(&new.anchor).map_err(err)?;
        let id: i64 = self
            .db
            .lock()?
            .query_row(
                "INSERT INTO comments (project, kind, target, theme, anchor, body, author_id, author_name,
                   status, created_commit, rev, branch)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'open', ?9, 1, ?10) RETURNING id",
                params![
                    project,
                    new.kind,
                    new.target,
                    new.theme,
                    anchor,
                    new.body,
                    new.author.id,
                    new.author.name,
                    new.commit,
                    new.branch
                ],
                |row| row.get(0),
            )
            .map_err(err)?;
        let id = u64::try_from(id).map_err(err)?;
        if let Some(bytes) = snapshot {
            let path = self.snapshot_path(project, id);
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(err)?;
            }
            std::fs::write(&path, bytes).map_err(err)?;
        }
        self.get(project, id)?
            .ok_or_else(|| StoreError("comment vanished after insert".into()))
    }

    fn list(&self, project: &str) -> Result<Vec<Comment>, StoreError> {
        load(&*self.db.lock()?, "project = ?1", &[&project])
    }

    fn get(&self, project: &str, id: u64) -> Result<Option<Comment>, StoreError> {
        Ok(load(
            &*self.db.lock()?,
            "project = ?1 AND id = ?2",
            &[&project, &sql_id(id)],
        )?
        .into_iter()
        .next())
    }

    fn reply(
        &self,
        project: &str,
        id: u64,
        rev: u32,
        reply: &NewReply,
    ) -> Result<Option<Comment>, StoreError> {
        {
            let mut conn = self.db.lock()?;
            let tx = conn.transaction().map_err(err)?;
            // Compare-and-swap on rev: a writer who saw an older thread changes nothing.
            let moved = tx
                .execute(
                    "UPDATE comments SET rev = rev + 1,
                       status = COALESCE(?4, status),
                       resolved_commit = CASE WHEN ?4 IS NULL THEN resolved_commit ELSE ?5 END,
                       updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                     WHERE project = ?1 AND id = ?2 AND rev = ?3",
                    params![
                        project,
                        sql_id(id),
                        rev,
                        reply.status.map(status_text),
                        reply.resolved_commit
                    ],
                )
                .map_err(err)?;
            if moved == 0 {
                return Ok(None);
            }
            tx.execute(
                "INSERT INTO replies (comment, author_id, author_name, body, kind)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    sql_id(id),
                    reply.author.id,
                    reply.author.name,
                    reply.body,
                    kind_text(reply.kind)
                ],
            )
            .map_err(err)?;
            tx.commit().map_err(err)?;
        }
        self.get(project, id)
    }

    fn move_anchor(
        &self,
        project: &str,
        id: u64,
        anchor: &charet_model::Anchor,
    ) -> Result<(), StoreError> {
        let anchor = serde_json::to_string(anchor).map_err(err)?;
        self.db
            .lock()?
            .execute(
                "UPDATE comments SET anchor = ?3 WHERE project = ?1 AND id = ?2",
                params![project, sql_id(id), anchor],
            )
            .map(drop)
            .map_err(err)
    }

    fn snapshot(&self, project: &str, id: u64) -> Result<Option<Vec<u8>>, StoreError> {
        let exists = self
            .db
            .lock()?
            .query_row(
                "SELECT 1 FROM comments WHERE project = ?1 AND id = ?2",
                params![project, sql_id(id)],
                |_| Ok(()),
            )
            .optional()
            .map_err(err)?
            .is_some();
        if !exists {
            return Ok(None);
        }
        match std::fs::read(self.snapshot_path(project, id)) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(err(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use charet_engine::Records;
    use charet_model::{Anchor, Origin, Project, Visibility};

    fn store(case: &str) -> (CommentStore, PathBuf) {
        let db = Arc::new(Db::memory().unwrap());
        db.insert_project(&Project {
            key: "web".into(),
            title: "Web".into(),
            visibility: Visibility::Team,
            visitors: None,
            visitor_access: charet_model::VisitorAccess::View,
            origin: Origin::Empty,
            created_at: String::new(),
        })
        .unwrap();
        let dir =
            std::env::temp_dir().join(format!("charet-comments-{}-{case}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        (CommentStore::new(db, &dir), dir)
    }

    fn me() -> Person {
        Person {
            id: "local".into(),
            name: "Local admin".into(),
        }
    }

    #[test]
    fn thread_with_compare_and_swap() {
        let (store, dir) = store("thread");
        let new = NewComment {
            kind: "screen".into(),
            target: "list".into(),
            theme: "light".into(),
            anchor: Anchor {
                x: 1,
                y: 2,
                w: 3,
                h: 4,
                text: "Add".into(),
                src: Some("src/a.tsx:1:1".into()),
                ..Anchor::default()
            },
            body: "Bigger".into(),
            author: me(),
            commit: "c1".into(),
            branch: None,
        };
        let comment = store.add("web", &new, Some(b"jpeg")).unwrap();
        assert_eq!((comment.rev, comment.status), (1, CommentStatus::Open));
        assert_eq!(comment.anchor, new.anchor);
        assert_eq!(
            store.snapshot("web", comment.id).unwrap().as_deref(),
            Some(&b"jpeg"[..])
        );

        let reply = |kind, status, resolved: Option<&str>| NewReply {
            author: me(),
            body: "ok".into(),
            kind,
            status,
            resolved_commit: resolved.map(str::to_owned),
        };
        let replied = store
            .reply("web", comment.id, 1, &reply(ReplyKind::Reply, None, None))
            .unwrap()
            .unwrap();
        assert_eq!(replied.rev, 2);
        assert_eq!(
            store
                .reply("web", comment.id, 1, &reply(ReplyKind::Reply, None, None))
                .unwrap(),
            None,
            "stale rev"
        );
        let resolved = store
            .reply(
                "web",
                comment.id,
                2,
                &reply(
                    ReplyKind::Resolve,
                    Some(CommentStatus::Resolved),
                    Some("c2"),
                ),
            )
            .unwrap()
            .unwrap();
        assert_eq!(resolved.status, CommentStatus::Resolved);
        assert_eq!(resolved.resolved_commit.as_deref(), Some("c2"));
        assert_eq!(
            resolved.replies.iter().map(|r| r.kind).collect::<Vec<_>>(),
            [ReplyKind::Reply, ReplyKind::Resolve]
        );

        let moved = Anchor {
            x: 50,
            detached: true,
            ..new.anchor.clone()
        };
        store.move_anchor("web", comment.id, &moved).unwrap();
        let after = store.get("web", comment.id).unwrap().unwrap();
        assert_eq!(after.anchor, moved);
        assert_eq!(after.rev, 3, "moving the anchor is not a revision");
        assert_eq!(store.list("web").unwrap().len(), 1);
        assert_eq!(store.get("web", 999).unwrap(), None);
        assert_eq!(store.snapshot("web", 999).unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
