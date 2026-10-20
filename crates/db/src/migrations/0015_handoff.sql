-- Where versions meet the product repo: the commit an import came from, and handoff marks.
ALTER TABLE versions ADD COLUMN source_commit TEXT;
-- The store commit holding exactly the repo's files, when the import was joined with other work.
ALTER TABLE versions ADD COLUMN source_tree TEXT;
ALTER TABLE versions ADD COLUMN handoff_commit TEXT;
ALTER TABLE versions ADD COLUMN handoff_actor TEXT;
ALTER TABLE versions ADD COLUMN handoff_at TEXT;
