-- Per-buffer preview layout persistence (ADR-009 §"writ-storage").
--
-- Keyed by absolute source path: scratch buffers (no path on disk) never
-- key into this table and always open in their content-type default. When
-- a scratch buffer is later saved it acquires a path and persists from
-- that point on.
--
-- Columns mirror the ADR commitment exactly: layout_mode discriminant,
-- nullable split_ratio, last_view_mode. Split orientation is a per-session
-- UI preference and is not persisted in v1 (restores to vertical).

CREATE TABLE layout_state (
    path            TEXT PRIMARY KEY,
    layout_mode     TEXT NOT NULL CHECK(layout_mode IN ('source', 'preview', 'split', 'detached')),
    split_ratio     REAL,
    last_view_mode  TEXT NOT NULL CHECK(last_view_mode IN ('source', 'preview')),
    updated_at      TEXT NOT NULL
);
