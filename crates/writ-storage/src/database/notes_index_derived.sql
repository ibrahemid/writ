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
