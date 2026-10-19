-- One-time links that let someone in without an identity provider: a new member joins with the
-- role the link fixes, or (with `user_id`) an existing member signs in on another device.
CREATE TABLE invites (
  id         TEXT PRIMARY KEY,
  token_hash TEXT NOT NULL UNIQUE,
  role       TEXT NOT NULL CHECK (role IN ('admin', 'editor', 'commenter', 'viewer')),
  user_id    TEXT REFERENCES users (id) ON DELETE CASCADE,
  label      TEXT NOT NULL,
  created_by TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  expires_at INTEGER NOT NULL,
  used_at    TEXT,
  used_by    TEXT
) STRICT;
