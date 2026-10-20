-- Approval of protected-file changes and the check of what a merge would give.
ALTER TABLE branches ADD COLUMN approved TEXT;
ALTER TABLE branches ADD COLUMN merge_check TEXT;
