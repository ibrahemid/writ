use crate::errors::StorageResult;
use rusqlite::Connection;
use std::path::Path;

/// Upper bound, in bytes, that the write-ahead log is truncated to on
/// checkpoint.
const JOURNAL_SIZE_LIMIT_BYTES: i64 = 64 * 1024 * 1024;

/// Opens a SQLite database at `path` using Writ's pragma defaults.
///
/// The returned connection has:
///
/// - `journal_mode = WAL` for concurrent read/write performance,
/// - `synchronous = NORMAL` for durability within a transaction while
///   avoiding the fsync cost of `FULL` on every commit,
/// - `foreign_keys = ON` so referential constraints are enforced,
/// - `journal_size_limit = 64 MiB` so a checkpoint truncates the
///   write-ahead log back to that cap instead of leaving it at its
///   high-water mark.
pub fn open_database(path: &Path) -> StorageResult<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "wal")?;
    conn.pragma_update(None, "synchronous", "normal")?;
    conn.pragma_update(None, "foreign_keys", "on")?;
    conn.pragma_update(None, "journal_size_limit", JOURNAL_SIZE_LIMIT_BYTES)?;
    Ok(conn)
}
