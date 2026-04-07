use crate::errors::StorageResult;
use rusqlite::Connection;
use std::path::Path;

pub fn open_database(path: &Path) -> StorageResult<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "wal")?;
    conn.pragma_update(None, "synchronous", "normal")?;
    conn.pragma_update(None, "foreign_keys", "on")?;
    Ok(conn)
}
