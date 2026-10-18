-- The latest capture of each screen or story per theme. Images and element maps are files next to
-- the database (`captures/<project>/`).
CREATE TABLE captures (
  project     TEXT NOT NULL REFERENCES projects (key) ON DELETE CASCADE,
  kind        TEXT NOT NULL CHECK (kind IN ('screen', 'story')),
  target      TEXT NOT NULL,
  theme       TEXT NOT NULL CHECK (theme IN ('light', 'dark')),
  git_commit  TEXT NOT NULL,
  width       INTEGER NOT NULL,
  height      INTEGER NOT NULL,
  issues      TEXT NOT NULL,
  sources     TEXT NOT NULL,
  captured_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  PRIMARY KEY (project, kind, target, theme)
) STRICT;
