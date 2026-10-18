CREATE TABLE projects (
  key        TEXT PRIMARY KEY,
  title      TEXT NOT NULL,
  visibility TEXT NOT NULL CHECK (visibility IN ('team', 'private')),
  origin     TEXT NOT NULL CHECK (origin IN ('empty', 'template', 'import')),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE TABLE versions (
  project    TEXT NOT NULL REFERENCES projects (key) ON DELETE CASCADE,
  number     INTEGER NOT NULL,
  git_commit TEXT NOT NULL,
  kind       TEXT NOT NULL CHECK (kind IN ('initial', 'import', 'push', 'merge', 'restore')),
  actor      TEXT NOT NULL,
  note       TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  PRIMARY KEY (project, number)
) STRICT;
