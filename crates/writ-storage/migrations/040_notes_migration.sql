-- Files are the only copy of the text (ADR-028). This migration adds the
-- columns the one-time notes migration records its progress in, the meta
-- table that records the rollback copy, and the path-keyed index the search
-- and link work is built on. It moves no data: the pass that writes files
-- runs in Rust, after the read path has moved to source_path, so no row is
-- ever left as a mirror the editor cannot open.

ALTER TABLE buffers ADD COLUMN migrated_path TEXT;
ALTER TABLE buffers ADD COLUMN migrated_at INTEGER;

CREATE TABLE schema_meta (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

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

CREATE TABLE links (
    from_path TEXT NOT NULL REFERENCES files(path) ON DELETE CASCADE,
    to_target TEXT NOT NULL,
    to_path   TEXT,
    kind      TEXT NOT NULL,
    line      INTEGER NOT NULL DEFAULT 0,
    col       INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_links_from ON links(from_path);
CREATE INDEX idx_links_to ON links(to_path);

CREATE TABLE properties (
    path       TEXT NOT NULL REFERENCES files(path) ON DELETE CASCADE,
    key        TEXT NOT NULL,
    value_json TEXT NOT NULL
);
CREATE INDEX idx_properties_path ON properties(path);
CREATE INDEX idx_properties_key ON properties(key);

CREATE TABLE tags (
    path TEXT NOT NULL REFERENCES files(path) ON DELETE CASCADE,
    tag  TEXT NOT NULL,
    line INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_tags_path ON tags(path);
CREATE INDEX idx_tags_tag ON tags(tag);

CREATE TABLE headings (
    path  TEXT NOT NULL REFERENCES files(path) ON DELETE CASCADE,
    level INTEGER NOT NULL,
    text  TEXT NOT NULL,
    line  INTEGER NOT NULL DEFAULT 0,
    slug  TEXT NOT NULL
);
CREATE INDEX idx_headings_path ON headings(path);

-- Same tokenizer and prefix set as buffer_fts after migration 030, so search
-- behaviour does not change when the index is re-keyed to paths.
CREATE VIRTUAL TABLE files_fts USING fts5(
    name,
    content,
    prefix='2 3 4',
    tokenize='unicode61 remove_diacritics 2'
);
