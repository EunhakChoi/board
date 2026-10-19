-- Personal CLI tokens and device logins waiting for approval in the browser.
CREATE TABLE cli_tokens (
  id           TEXT PRIMARY KEY,
  user_id      TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
  name         TEXT NOT NULL,
  token_hash   TEXT NOT NULL UNIQUE,
  via          TEXT NOT NULL CHECK (via IN ('device', 'manual')),
  created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  last_used_at TEXT
) STRICT;

CREATE INDEX cli_tokens_user ON cli_tokens (user_id);

CREATE TABLE device_codes (
  code_hash  TEXT PRIMARY KEY,
  user_code  TEXT NOT NULL UNIQUE,
  name       TEXT NOT NULL,
  status     TEXT NOT NULL CHECK (status IN ('pending', 'approved', 'denied')),
  user_id    TEXT REFERENCES users (id) ON DELETE CASCADE,
  expires_at INTEGER NOT NULL
) STRICT;
