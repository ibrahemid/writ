-- Files are the only copy of the text (ADR-028). This migration adds the
-- columns the one-time notes migration records its progress in, the meta
-- table that records the rollback copy, and the path-keyed index the search
-- and link work is built on. It moves no data: the pass that writes files
-- runs in Rust, after the read path has moved to source_path, so no row is
-- ever left as a mirror the editor cannot open.

ALTER TABLE buffers ADD COLUMN migrated_path TEXT;
ALTER TABLE buffers ADD COLUMN migrated_at INTEGER;

-- files is a rowid table: files_fts joins it on rowid, exactly as buffer_fts
-- joins buffers. An index write must therefore be
-- INSERT ... ON CONFLICT(path) DO UPDATE, never INSERT OR REPLACE, which
-- deletes the row and reassigns the rowid: that orphans the files_fts row and
-- cascades links, properties, tags and headings away.
CREATE TABLE files (
    path       TEXT PRIMARY KEY,
    size       INTEGER NOT NULL DEFAULT 0,
    mtime      INTEGER NOT NULL DEFAULT 0,
    hash       TEXT,
    indexed_at TEXT NOT NULL
);

-- schema_meta, links, properties, tags, headings and files_fts are created
-- from src/database/schema_meta.sql, notes_index_derived.sql and
-- notes_index_fts.sql, appended to this migration. The repair that recreates
-- them when a database loses them reads the same three files.
