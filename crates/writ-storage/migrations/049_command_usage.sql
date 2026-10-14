-- How often each palette command ran and when it last did, which ranks the
-- palette (ADR-048). Derived from use, so it lives here rather than in
-- config.toml, which a person edits and syncs. A config written before this
-- table carries the counts under [commands.usage]; the launch that first sees
-- this table moves them here once.

CREATE TABLE command_usage (
    id            TEXT PRIMARY KEY,
    count         INTEGER NOT NULL CHECK(count >= 0),
    last_used_ms  INTEGER NOT NULL CHECK(last_used_ms >= 0)
);
