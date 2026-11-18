-- What a one-time schema pass did, and the derived-row census the notes index
-- walk keeps. The census is what tells the walk to read every file again, so
-- the repair that recreates the index tables recreates this one with them.
CREATE TABLE schema_meta (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
