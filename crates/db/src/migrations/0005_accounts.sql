-- Accounts that signed in through the identity provider, their sessions, the one-time setup link
-- and the allowlist. Tokens are stored as SHA-256 hashes only.
CREATE TABLE users (
  id         TEXT PRIMARY KEY,
  provider   TEXT NOT NULL,
  subject    TEXT NOT NULL,
  email      TEXT NOT NULL,
  name       TEXT NOT NULL,
  avatar_url TEXT,
  role       TEXT NOT NULL CHECK (role IN ('admin', 'editor', 'commenter', 'viewer')),
  status     TEXT NOT NULL CHECK (status IN ('pending', 'active', 'rejected', 'removed')),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  UNIQUE (provider, subject)
) STRICT;

CREATE TABLE sessions (
  token_hash TEXT PRIMARY KEY,
  user_id    TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
  expires_at INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE INDEX sessions_user ON sessions (user_id);

CREATE TABLE setup_tokens (
  token_hash TEXT PRIMARY KEY,
  used_at    TEXT
) STRICT;

CREATE TABLE allow_rules (
  id         INTEGER PRIMARY KEY,
  kind       TEXT NOT NULL CHECK (kind IN ('domain', 'email')),
  value      TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  UNIQUE (kind, value)
) STRICT;
