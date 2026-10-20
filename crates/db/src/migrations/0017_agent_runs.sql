-- Web agent runs (story 04): an instruction, the branch it works on, how it went and what it cost.
CREATE TABLE agent_runs (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  project     TEXT NOT NULL REFERENCES projects (key) ON DELETE CASCADE,
  branch      INTEGER NOT NULL,
  actor_id    TEXT NOT NULL,
  actor_name  TEXT NOT NULL,
  agent       TEXT NOT NULL CHECK (agent IN ('claude', 'codex', 'script')),
  model       TEXT NOT NULL DEFAULT '',
  prompt      TEXT NOT NULL,
  context     TEXT NOT NULL DEFAULT 'null',
  attachments INTEGER NOT NULL DEFAULT 0,
  state       TEXT NOT NULL CHECK (state IN ('queued', 'running', 'done', 'failed', 'canceled')),
  error       TEXT,
  tokens_in   INTEGER NOT NULL DEFAULT 0,
  tokens_out  INTEGER NOT NULL DEFAULT 0,
  cost_usd    REAL,
  -- Hash of the token the run's container uses to reach the agent door; cleared when it ends.
  token_hash  TEXT,
  -- How many events the runner has handed over (where the next poll starts).
  runner_seq  INTEGER NOT NULL DEFAULT 0,
  created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  started_at  TEXT,
  ended_at    TEXT
) STRICT;
CREATE INDEX agent_runs_branch ON agent_runs (project, branch, id);
CREATE UNIQUE INDEX agent_runs_token ON agent_runs (token_hash) WHERE token_hash IS NOT NULL;

-- The conversation, as people read it (same visibility as the branch).
CREATE TABLE agent_messages (
  run        INTEGER NOT NULL REFERENCES agent_runs (id) ON DELETE CASCADE,
  seq        INTEGER NOT NULL,
  kind       TEXT NOT NULL CHECK (kind IN ('user', 'assistant', 'tool', 'status')),
  text       TEXT NOT NULL,
  tool       TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  PRIMARY KEY (run, seq)
) STRICT;

-- Images attached to a prompt.
CREATE TABLE agent_attachments (
  run        INTEGER NOT NULL REFERENCES agent_runs (id) ON DELETE CASCADE,
  seq        INTEGER NOT NULL,
  media_type TEXT NOT NULL,
  data       BLOB NOT NULL,
  PRIMARY KEY (run, seq)
) STRICT;
