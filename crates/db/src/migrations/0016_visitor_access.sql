-- What visitors (no token) may do on a project: none, view or comment. NULL follows the server's setting.
ALTER TABLE projects ADD COLUMN visitors TEXT;
