-- Who may see a private project besides admins.
CREATE TABLE project_members (
  project TEXT NOT NULL REFERENCES projects (key) ON DELETE CASCADE,
  user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
  PRIMARY KEY (project, user_id)
) STRICT;
