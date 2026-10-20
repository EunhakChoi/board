//! Branches and what happened on them.

use charet_engine::{BranchEvent, NewBranch, StoreError};
use charet_model::{Branch, BranchStatus, Person};
use rusqlite::{OptionalExtension, Row, params};

use crate::{Db, err, parse, text};

const BRANCH_FIELDS: &str = "number, title, author_id, author_name, status, base_commit, \
     head_commit, conflicts, merged_version, created_at, updated_at";

fn branch_row(row: &Row<'_>) -> rusqlite::Result<Branch> {
    let conflicts: String = row.get(7)?;
    Ok(Branch {
        number: row.get(0)?,
        title: row.get(1)?,
        author: Person {
            id: row.get(2)?,
            name: row.get(3)?,
        },
        status: parse::<BranchStatus>(row, 4)?,
        base: row.get(5)?,
        head: row.get(6)?,
        conflicts: serde_json::from_str(&conflicts).unwrap_or_default(),
        merged_version: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

impl Db {
    pub(crate) fn insert_branch_row(
        &self,
        project: &str,
        branch: &NewBranch,
    ) -> Result<Branch, StoreError> {
        // One statement picks the next number, so concurrent writers cannot collide.
        self.lock()?
            .query_row(
                &format!(
                    "INSERT INTO branches
                       (project, number, title, author_id, author_name, status, base_commit,
                        head_commit)
                     VALUES (?1,
                             (SELECT COALESCE(MAX(number), 0) + 1 FROM branches WHERE project = ?1),
                             ?2, ?3, ?4, 'open', ?5, ?6)
                     RETURNING {BRANCH_FIELDS}"
                ),
                params![
                    project,
                    branch.title,
                    branch.author.id,
                    branch.author.name,
                    branch.base,
                    branch.head
                ],
                branch_row,
            )
            .map_err(err)
    }

    pub(crate) fn branch_row(
        &self,
        project: &str,
        number: u32,
    ) -> Result<Option<Branch>, StoreError> {
        self.lock()?
            .query_row(
                &format!("SELECT {BRANCH_FIELDS} FROM branches WHERE project = ?1 AND number = ?2"),
                params![project, number],
                branch_row,
            )
            .optional()
            .map_err(err)
    }

    pub(crate) fn branch_rows(&self, project: &str) -> Result<Vec<Branch>, StoreError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {BRANCH_FIELDS} FROM branches WHERE project = ?1 ORDER BY number DESC"
            ))
            .map_err(err)?;
        stmt.query_map(params![project], branch_row)
            .map_err(err)?
            .collect::<Result<_, _>>()
            .map_err(err)
    }

    pub(crate) fn update_branch_row(
        &self,
        project: &str,
        branch: &Branch,
    ) -> Result<(), StoreError> {
        let conflicts = serde_json::to_string(&branch.conflicts).map_err(err)?;
        self.lock()?
            .execute(
                "UPDATE branches SET title = ?3, status = ?4, base_commit = ?5, head_commit = ?6,
                   conflicts = ?7, merged_version = ?8,
                   updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE project = ?1 AND number = ?2",
                params![
                    project,
                    branch.number,
                    branch.title,
                    text(branch.status),
                    branch.base,
                    branch.head,
                    conflicts,
                    branch.merged_version
                ],
            )
            .map_err(err)?;
        Ok(())
    }

    pub(crate) fn add_branch_event_row(
        &self,
        project: &str,
        number: u32,
        event: &BranchEvent,
    ) -> Result<(), StoreError> {
        self.lock()?
            .execute(
                "INSERT INTO branch_events (project, number, actor_id, actor_name, kind, detail)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    project,
                    number,
                    event.actor.id,
                    event.actor.name,
                    event.kind,
                    event.detail
                ],
            )
            .map_err(err)?;
        Ok(())
    }

    pub(crate) fn branch_event_rows(
        &self,
        project: &str,
        number: u32,
    ) -> Result<Vec<BranchEvent>, StoreError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT actor_id, actor_name, kind, detail, at FROM branch_events
                 WHERE project = ?1 AND number = ?2 ORDER BY rowid",
            )
            .map_err(err)?;
        stmt.query_map(params![project, number], |row| {
            Ok(BranchEvent {
                actor: Person {
                    id: row.get(0)?,
                    name: row.get(1)?,
                },
                kind: row.get(2)?,
                detail: row.get(3)?,
                at: row.get(4)?,
            })
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)
    }
}
