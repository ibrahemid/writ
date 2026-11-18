-- How much of a file the index holds: 'content' for a row written from the
-- file's text, 'name' for one written without reading the file at all, which
-- is what a sync placeholder with no local data gets (ADR-028 section 7).
--
-- reconcile needs this to tell a downloaded note from the placeholder it
-- replaced: materialising a placeholder leaves size and mtime untouched, so
-- the row's other columns say nothing changed. It is a column of its own
-- rather than a marker in hash, which means a digest and nothing else.
--
-- Every existing row was written from content, so the default is the whole
-- migration.

ALTER TABLE files ADD COLUMN indexed_by TEXT NOT NULL DEFAULT 'content';
