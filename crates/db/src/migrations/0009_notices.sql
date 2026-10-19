-- Inbox notifications per member, and webhooks the admin set up.
CREATE TABLE notifications (
  id         INTEGER PRIMARY KEY,
  user_id    TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
  kind       TEXT NOT NULL,
  project    TEXT REFERENCES projects (key) ON DELETE CASCADE,
  actor      TEXT NOT NULL,
  text       TEXT NOT NULL,
  link       TEXT NOT NULL,
  data       TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  read_at    TEXT
) STRICT;

CREATE INDEX notifications_user ON notifications (user_id, id);

-- The signing secret is derived from the server key and `salt`; only its last 4 are kept.
CREATE TABLE webhooks (
  id               TEXT PRIMARY KEY,
  url              TEXT NOT NULL,
  events           TEXT NOT NULL,
  project          TEXT REFERENCES projects (key) ON DELETE CASCADE,
  salt             TEXT NOT NULL,
  secret_last4     TEXT NOT NULL,
  created_by       TEXT NOT NULL,
  created_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  last_status      INTEGER,
  last_error       TEXT,
  last_delivery_at TEXT
) STRICT;
