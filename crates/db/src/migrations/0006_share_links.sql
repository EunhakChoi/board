-- Share links into one project, the guests who came in through them and their sessions.
CREATE TABLE share_links (
  id         TEXT PRIMARY KEY,
  project    TEXT NOT NULL REFERENCES projects (key) ON DELETE CASCADE,
  token_hash TEXT NOT NULL UNIQUE,
  role       TEXT NOT NULL CHECK (role IN ('editor', 'commenter', 'viewer')),
  label      TEXT NOT NULL,
  created_by TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  revoked_at TEXT
) STRICT;

CREATE INDEX share_links_project ON share_links (project, created_at);

CREATE TABLE guests (
  id         TEXT PRIMARY KEY,
  link       TEXT NOT NULL REFERENCES share_links (id) ON DELETE CASCADE,
  name       TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE TABLE guest_sessions (
  token_hash TEXT PRIMARY KEY,
  guest      TEXT NOT NULL REFERENCES guests (id) ON DELETE CASCADE,
  expires_at INTEGER NOT NULL
) STRICT;
