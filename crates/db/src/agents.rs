//! Web agent runs, their conversations and attached images.

use charet_engine::{AgentRecords, NewRun, RunUpdate, StoreError};
use charet_model::{AgentMessage, AgentRun, Attachment, MessageKind, Person};
use rusqlite::{OptionalExtension, Row, params};

use crate::{Db, err, parse, text};

const RUN_FIELDS: &str = "SELECT id, project, branch, actor_id, actor_name, agent, model, prompt, \
     context, attachments, state, error, tokens_in, tokens_out, cost_usd, created_at, started_at, \
     ended_at, runner_seq FROM agent_runs";

fn run_row(row: &Row<'_>) -> rusqlite::Result<AgentRun> {
    let context: String = row.get(8)?;
    Ok(AgentRun {
        id: u64::try_from(row.get::<_, i64>(0)?).unwrap_or(0),
        project: row.get(1)?,
        branch: row.get(2)?,
        actor: Person {
            id: row.get(3)?,
            name: row.get(4)?,
        },
        agent: parse(row, 5)?,
        model: row.get(6)?,
        prompt: row.get(7)?,
        context: serde_json::from_str(&context).unwrap_or_default(),
        attachments: row.get(9)?,
        state: parse(row, 10)?,
        error: row.get(11)?,
        tokens_in: u64::try_from(row.get::<_, i64>(12)?).unwrap_or(0),
        tokens_out: u64::try_from(row.get::<_, i64>(13)?).unwrap_or(0),
        cost_usd: row.get(14)?,
        created_at: row.get(15)?,
        started_at: row.get(16)?,
        ended_at: row.get(17)?,
        runner_seq: row.get(18)?,
    })
}

fn message_row(row: &Row<'_>) -> rusqlite::Result<AgentMessage> {
    Ok(AgentMessage {
        run: u64::try_from(row.get::<_, i64>(0)?).unwrap_or(0),
        seq: row.get(1)?,
        kind: parse(row, 2)?,
        text: row.get(3)?,
        tool: row.get(4)?,
        created_at: row.get(5)?,
    })
}

fn id(run: u64) -> i64 {
    i64::try_from(run).unwrap_or(i64::MAX)
}

fn count(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

impl Db {
    fn query_runs(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
    ) -> Result<Vec<AgentRun>, StoreError> {
        let conn = self.lock()?;
        let mut statement = conn.prepare(&format!("{RUN_FIELDS} {sql}")).map_err(err)?;
        let rows = statement.query_map(params, run_row).map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }
}

impl AgentRecords for Db {
    fn insert_run(&self, run: &NewRun) -> Result<AgentRun, StoreError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(err)?;
        tx.execute(
            "INSERT INTO agent_runs (project, branch, actor_id, actor_name, agent, model, prompt,
                 context, attachments, state)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'queued')",
            params![
                run.project,
                run.branch,
                run.actor.id,
                run.actor.name,
                text(run.agent),
                run.model,
                run.prompt,
                run.context.to_string(),
                i64::try_from(run.attachments.len()).unwrap_or(i64::MAX),
            ],
        )
        .map_err(err)?;
        let id = tx.last_insert_rowid();
        for (seq, image) in run.attachments.iter().enumerate() {
            tx.execute(
                "INSERT INTO agent_attachments (run, seq, media_type, data) VALUES (?1, ?2, ?3, ?4)",
                params![id, i64::try_from(seq).unwrap_or(i64::MAX), image.media_type, image.data],
            )
            .map_err(err)?;
        }
        tx.execute(
            "INSERT INTO agent_messages (run, seq, kind, text) VALUES (?1, 1, 'user', ?2)",
            params![id, run.prompt],
        )
        .map_err(err)?;
        tx.commit().map_err(err)?;
        drop(conn);
        self.run(u64::try_from(id).unwrap_or(0))?
            .ok_or_else(|| StoreError("run vanished".into()))
    }

    fn run(&self, run: u64) -> Result<Option<AgentRun>, StoreError> {
        self.lock()?
            .query_row(
                &format!("{RUN_FIELDS} WHERE id = ?1"),
                params![id(run)],
                run_row,
            )
            .optional()
            .map_err(err)
    }

    fn runs(&self, project: &str, branch: Option<u32>) -> Result<Vec<AgentRun>, StoreError> {
        match branch {
            Some(branch) => self.query_runs(
                "WHERE project = ?1 AND branch = ?2 ORDER BY id",
                params![project, branch],
            ),
            None => self.query_runs("WHERE project = ?1 ORDER BY id", params![project]),
        }
    }

    fn open_runs(&self) -> Result<Vec<AgentRun>, StoreError> {
        self.query_runs("WHERE state IN ('queued', 'running') ORDER BY id", [])
    }

    fn all_runs(&self) -> Result<Vec<AgentRun>, StoreError> {
        self.query_runs("ORDER BY id DESC", [])
    }

    fn update_run(&self, run: u64, update: &RunUpdate) -> Result<(), StoreError> {
        let conn = self.lock()?;
        let (token_set, token) = match &update.token_hash {
            Some(hash) => (true, hash.clone()),
            None => (false, None),
        };
        conn.execute(
            "UPDATE agent_runs SET
                 state = COALESCE(?2, state),
                 error = COALESCE(?3, error),
                 tokens_in = COALESCE(?4, tokens_in),
                 tokens_out = COALESCE(?5, tokens_out),
                 cost_usd = COALESCE(?6, cost_usd),
                 token_hash = CASE WHEN ?7 THEN ?8 ELSE token_hash END,
                 runner_seq = COALESCE(?9, runner_seq),
                 started_at = CASE WHEN ?10 THEN strftime('%Y-%m-%dT%H:%M:%fZ', 'now') ELSE started_at END,
                 ended_at = CASE WHEN ?11 THEN strftime('%Y-%m-%dT%H:%M:%fZ', 'now') ELSE ended_at END
             WHERE id = ?1",
            params![
                id(run),
                update.state.map(text),
                update.error,
                update.tokens_in.map(count),
                update.tokens_out.map(count),
                update.cost_usd,
                token_set,
                token,
                update.runner_seq,
                update.started,
                update.ended,
            ],
        )
        .map_err(err)?;
        Ok(())
    }

    fn run_by_token(&self, hash: &str) -> Result<Option<AgentRun>, StoreError> {
        self.lock()?
            .query_row(
                &format!("{RUN_FIELDS} WHERE token_hash = ?1 AND state IN ('queued', 'running')"),
                params![hash],
                run_row,
            )
            .optional()
            .map_err(err)
    }

    fn add_message(
        &self,
        run: u64,
        kind: MessageKind,
        text_: &str,
        tool: Option<&str>,
    ) -> Result<AgentMessage, StoreError> {
        self.lock()?
            .query_row(
                "INSERT INTO agent_messages (run, seq, kind, text, tool)
                 VALUES (?1, (SELECT COALESCE(MAX(seq), 0) + 1 FROM agent_messages WHERE run = ?1),
                         ?2, ?3, ?4)
                 RETURNING run, seq, kind, text, tool, created_at",
                params![id(run), text(kind), text_, tool],
                message_row,
            )
            .map_err(err)
    }

    fn messages(&self, run: u64, after: u32) -> Result<Vec<AgentMessage>, StoreError> {
        let conn = self.lock()?;
        let mut statement = conn
            .prepare(
                "SELECT run, seq, kind, text, tool, created_at FROM agent_messages
                 WHERE run = ?1 AND seq > ?2 ORDER BY seq",
            )
            .map_err(err)?;
        let rows = statement
            .query_map(params![id(run), after], message_row)
            .map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }

    fn attachments(&self, run: u64) -> Result<Vec<Attachment>, StoreError> {
        let conn = self.lock()?;
        let mut statement = conn
            .prepare("SELECT media_type, data FROM agent_attachments WHERE run = ?1 ORDER BY seq")
            .map_err(err)?;
        let rows = statement
            .query_map(params![id(run)], |row| {
                Ok(Attachment {
                    media_type: row.get(0)?,
                    data: row.get(1)?,
                })
            })
            .map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }
}

#[cfg(test)]
mod tests {
    use charet_engine::Records;
    use charet_model::{AgentKind, RunState};

    use super::*;

    #[test]
    fn runs_keep_their_conversation_images_and_token() {
        let db = Db::memory().unwrap();
        db.insert_project(&crate::tests::project("shop")).unwrap();
        let run = db
            .insert_run(&NewRun {
                project: "shop".into(),
                branch: 3,
                actor: Person {
                    id: "u1".into(),
                    name: "Ann".into(),
                },
                agent: AgentKind::Claude,
                model: String::new(),
                prompt: "bigger".into(),
                context: serde_json::json!({ "screen": "home" }),
                attachments: vec![Attachment {
                    media_type: "image/png".into(),
                    data: vec![1, 2, 3],
                }],
            })
            .unwrap();
        assert_eq!(
            (run.state, run.attachments, run.branch),
            (RunState::Queued, 1, 3)
        );
        assert_eq!(run.context["screen"], "home");
        assert_eq!(db.open_runs().unwrap().len(), 1);

        db.update_run(
            run.id,
            &RunUpdate {
                state: Some(RunState::Running),
                token_hash: Some(Some("h".into())),
                started: true,
                ..RunUpdate::default()
            },
        )
        .unwrap();
        assert_eq!(db.run_by_token("h").unwrap().unwrap().id, run.id);
        db.add_message(run.id, MessageKind::Tool, "Edit a.tsx", Some("Edit"))
            .unwrap();
        db.update_run(
            run.id,
            &RunUpdate {
                state: Some(RunState::Done),
                tokens_in: Some(10),
                cost_usd: Some(0.25),
                token_hash: Some(None),
                runner_seq: Some(4),
                ended: true,
                ..RunUpdate::default()
            },
        )
        .unwrap();
        assert_eq!(db.run_by_token("h").unwrap(), None);
        let run = db.run(run.id).unwrap().unwrap();
        assert_eq!(
            (run.state, run.tokens_in, run.cost_usd, run.runner_seq),
            (RunState::Done, 10, Some(0.25), 4)
        );
        assert!(run.started_at.is_some() && run.ended_at.is_some());
        let messages = db.messages(run.id, 1).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(
            (messages[0].seq, messages[0].tool.as_deref()),
            (2, Some("Edit"))
        );
        assert_eq!(db.attachments(run.id).unwrap()[0].data, [1, 2, 3]);
        assert_eq!(db.runs("shop", Some(3)).unwrap().len(), 1);
        assert_eq!(db.runs("shop", Some(4)).unwrap().len(), 0);
        assert_eq!(db.open_runs().unwrap().len(), 0);
    }
}
