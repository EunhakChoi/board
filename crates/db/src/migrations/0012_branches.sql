-- Branches (drafts and changes under review) and what versions came from.
ALTER TABLE versions ADD COLUMN branch INTEGER;
ALTER TABLE versions ADD COLUMN restored INTEGER;

CREATE TABLE branches (
  project        TEXT NOT NULL REFERENCES projects (key) ON DELETE CASCADE,
  number         INTEGER NOT NULL,
  title          TEXT NOT NULL,
  author_id      TEXT NOT NULL,
  author_name    TEXT NOT NULL,
  status         TEXT NOT NULL CHECK (status IN ('open', 'in_review', 'merged', 'archived')),
  base_commit    TEXT NOT NULL,
  head_commit    TEXT NOT NULL,
  conflicts      TEXT NOT NULL DEFAULT '[]',
  merged_version INTEGER,
  created_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  PRIMARY KEY (project, number)
) STRICT;

-- Who did what on a branch: push, review request, catch-up, merge, archive, duplicate.
CREATE TABLE branch_events (
  project    TEXT NOT NULL,
  number     INTEGER NOT NULL,
  actor_id   TEXT NOT NULL,
  actor_name TEXT NOT NULL,
  kind       TEXT NOT NULL,
  detail     TEXT NOT NULL DEFAULT '',
  at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  FOREIGN KEY (project, number) REFERENCES branches (project, number) ON DELETE CASCADE
) STRICT;
