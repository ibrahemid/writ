# ADR-004: SQLite Over Flat Files

**Status:** Accepted
**Date:** 2026-03-27

## Context

Writ needs to persist several categories of data locally:

- **Buffer metadata**: file path, last cursor position, scroll offset, language override,
  pinned status, open/close timestamps.
- **Session state**: which buffers are open, tab order, split layout, active buffer.
- **Search index**: content of open (and recently opened) buffers for fast across-file search.
- **Recovery journal**: partially-written file changes for crash recovery.

The simplest approach is a collection of JSON or TOML files, one per buffer or one per category.
This is zero-dependency (the `serde_json`/`toml` crates are already present) and easy to
inspect with a text editor.

However, flat files present problems at scale. With hundreds of open buffers, loading all
metadata at startup means reading and parsing hundreds of files. Searching across buffer content
with flat files requires either loading all content into memory or re-reading files on every
query. Concurrent writes from multiple application windows require manual locking.

## Decision

Use SQLite via `rusqlite` as the persistence layer for all structured application state.
Enable WAL (Write-Ahead Logging) mode on the database connection at startup.

Key factors:

- **Structured queries**: Buffer metadata can be queried, sorted, and filtered with SQL without
  loading all records into memory. `SELECT * FROM buffers ORDER BY last_accessed DESC LIMIT 10`
  is more efficient than deserializing a directory of JSON files.
- **FTS5 full-text search**: SQLite's FTS5 extension provides an inverted index over buffer
  content. Across-file search becomes a single SQL query rather than a sequential scan.
- **WAL mode crash safety**: WAL mode ensures that a crash mid-write does not corrupt committed
  data. The recovery journal still exists for in-progress file writes, but session and metadata
  state is always consistent.
- **Schema migrations**: `rusqlite_migration` or hand-rolled migration functions allow the
  schema to evolve across versions without breaking existing installations. Flat files offer no
  equivalent mechanism.
- **Atomic multi-table updates**: Opening a buffer involves writing to `buffers`, `sessions`,
  and `search_index` atomically in one transaction. With flat files, partial writes are
  observable failure states.

The SQLite file lives in the platform data directory (`~/.local/share/writ/` on Linux,
`~/Library/Application Support/writ/` on macOS) as `writ.db`.

## Consequences

**Positive:**
- Startup time is bounded; loading the buffer list is one indexed query regardless of how many
  buffers exist.
- Full-text search across thousands of buffers without loading content into memory.
- WAL mode allows the UI to read metadata (e.g., tab list) while a background task is writing
  (e.g., search index update) without contention.
- Schema versioning supports forward-compatible upgrades.

**Negative / risks:**
- **Binary dependency**: `rusqlite` bundles SQLite (or links to the system SQLite). The
  bundled variant adds ~600KB to the binary but eliminates the system version dependency.
  We use the bundled feature flag.
- **More complex than JSON**: Debugging application state requires `sqlite3` CLI or a GUI
  tool. A JSON file can be opened in any text editor. Mitigated by exposing a
  debug-dump command in development builds.
- **Single-writer constraint**: SQLite allows only one writer at a time. With WAL mode,
  concurrent readers are non-blocking, but two simultaneous write transactions will serialize.
  For a single-user desktop editor this is not a practical constraint.
