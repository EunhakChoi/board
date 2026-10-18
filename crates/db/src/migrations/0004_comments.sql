-- Comments on screens and stories (line = main in P0) and their threads. The capture snapshot a
-- comment was made on is a file next to the database (`comments/<project>/c<id>.jpg`).
CREATE TABLE comments (
  id              INTEGER PRIMARY KEY,
  project         TEXT NOT NULL REFERENCES projects (key) ON DELETE CASCADE,
  kind            TEXT NOT NULL CHECK (kind IN ('screen', 'story')),
  target          TEXT NOT NULL,
  theme           TEXT NOT NULL CHECK (theme IN ('light', 'dark')),
  anchor          TEXT NOT NULL,
  body            TEXT NOT NULL,
  author_id       TEXT NOT NULL,
  author_name     TEXT NOT NULL,
  status          TEXT NOT NULL CHECK (status IN ('open', 'resolved')),
  created_commit  TEXT NOT NULL,
  resolved_commit TEXT,
  rev             INTEGER NOT NULL,
  created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE INDEX comments_project ON comments (project, id);

CREATE TABLE replies (
  id          INTEGER PRIMARY KEY,
  comment     INTEGER NOT NULL REFERENCES comments (id) ON DELETE CASCADE,
  author_id   TEXT NOT NULL,
  author_name TEXT NOT NULL,
  body        TEXT NOT NULL,
  kind        TEXT NOT NULL CHECK (kind IN ('reply', 'resolve', 'reopen')),
  created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE INDEX replies_comment ON replies (comment, id);
