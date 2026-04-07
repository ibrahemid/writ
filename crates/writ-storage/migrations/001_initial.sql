CREATE TABLE buffers (
    id          TEXT PRIMARY KEY,
    title       TEXT NOT NULL,
    filename    TEXT NOT NULL,
    status      TEXT NOT NULL CHECK(status IN ('active', 'history')),
    language    TEXT,
    source_path TEXT,
    cursor_pos  INTEGER DEFAULT 0,
    scroll_pos  INTEGER DEFAULT 0,
    tab_order   INTEGER DEFAULT 0,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    closed_at   TEXT
);

CREATE INDEX idx_buffers_status ON buffers(status);
CREATE INDEX idx_buffers_updated ON buffers(updated_at);

CREATE VIRTUAL TABLE buffer_fts USING fts5(title, content);

CREATE TABLE session_snapshots (
    id              TEXT PRIMARY KEY,
    format_version  INTEGER NOT NULL DEFAULT 1,
    state_json      TEXT NOT NULL,
    created_at      TEXT NOT NULL,
    is_clean        INTEGER DEFAULT 0
);
