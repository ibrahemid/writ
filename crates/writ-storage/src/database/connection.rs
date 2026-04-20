use crate::errors::StorageResult;
use rusqlite::Connection;
use std::path::Path;

/// Opens a SQLite database at `path` using Writ's pragma defaults.
///
/// The returned connection has:
///
/// - `journal_mode = WAL` for concurrent read/write performance,
/// - `synchronous = NORMAL` for durability within a transaction while
///   avoiding the fsync cost of `FULL` on every commit,
/// - `foreign_keys = ON` so referential constraints are enforced.
pub fn open_database(path: &Path) -> StorageResult<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "wal")?;
    conn.pragma_update(None, "synchronous", "normal")?;
    conn.pragma_update(None, "foreign_keys", "on")?;
    Ok(conn)
}
