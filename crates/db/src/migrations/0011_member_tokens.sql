-- Members come in with a token an admin hands over (and visitors without one). Everything of the
-- earlier ways in goes: identity-provider accounts, invites, share links, CLI tokens and device
-- logins. Existing accounts are dropped with their sessions, memberships and notifications.
DROP TABLE device_codes;
DROP TABLE cli_tokens;
DROP TABLE guest_sessions;
DROP TABLE guests;
DROP TABLE share_links;
DROP TABLE invites;
DROP TABLE allow_rules;
DROP TABLE setup_tokens;
DROP TABLE sessions;
DELETE FROM project_members;
DELETE FROM notifications;
DROP TABLE users;
DELETE FROM settings WHERE key = 'sharing.owner';

-- The token is kept as it is: admins hand it over again from the Team page.
CREATE TABLE users (
  id           TEXT PRIMARY KEY,
  name         TEXT NOT NULL,
  role         TEXT NOT NULL CHECK (role IN ('admin', 'editor', 'commenter', 'viewer')),
  token        TEXT NOT NULL UNIQUE,
  created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  last_used_at TEXT
) STRICT;

-- Browser sessions. A newer browser signing in with the same token marks the others replaced.
CREATE TABLE sessions (
  token_hash TEXT PRIMARY KEY,
  user_id    TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
  expires_at INTEGER NOT NULL,
  replaced   INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE INDEX sessions_user ON sessions (user_id);
