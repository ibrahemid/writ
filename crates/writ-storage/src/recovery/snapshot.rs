use rusqlite::{params, Connection};
use uuid::Uuid;

use crate::errors::StorageResult;

/// A point-in-time snapshot of Writ's session state.
pub struct SessionSnapshot {
    /// Snapshot UUID, assigned at creation.
    pub id: String,
    /// Snapshot format version. Bumped when the shape of
    /// `state_json` changes in a breaking way.
    pub format_version: i32,
    /// Opaque session state encoded as JSON.
    pub state_json: serde_json::Value,
    /// Creation timestamp formatted as SQLite `datetime('now')`.
    pub created_at: String,
    /// `true` when written during a clean shutdown.
    pub is_clean: bool,
}

/// Persistence facade over session snapshots.
pub struct SnapshotManager<'a> {
    conn: &'a Connection,
}

impl<'a> SnapshotManager<'a> {
    /// Constructs a manager over the given connection.
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Writes a new snapshot row with `state` as its encoded state.
    pub fn write_snapshot(&self, state: &serde_json::Value, is_clean: bool) -> StorageResult<()> {
        let id = Uuid::new_v4().to_string();
        let state_str = serde_json::to_string(state)?;
        let is_clean_int = if is_clean { 1i64 } else { 0i64 };
        self.conn.execute(
            "INSERT INTO session_snapshots (id, format_version, state_json, created_at, is_clean)
             VALUES (?1, 1, ?2, datetime('now'), ?3)",
            params![id, state_str, is_clean_int],
        )?;
        Ok(())
    }

    /// Returns the most recently written snapshot, or `None` when the
    /// table is empty.
    pub fn latest_snapshot(&self) -> StorageResult<Option<SessionSnapshot>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, format_version, state_json, created_at, is_clean
             FROM session_snapshots
             ORDER BY created_at DESC, rowid DESC
             LIMIT 1",
        )?;

        let result = stmt.query_row([], |row| {
            let state_str: String = row.get(2)?;
            let is_clean_int: i64 = row.get(4)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i32>(1)?,
                state_str,
                row.get::<_, String>(3)?,
                is_clean_int,
            ))
        });

        match result {
            Ok((id, format_version, state_str, created_at, is_clean_int)) => {
                let state_json = serde_json::from_str(&state_str)?;
                Ok(Some(SessionSnapshot {
                    id,
                    format_version,
                    state_json,
                    created_at,
                    is_clean: is_clean_int != 0,
                }))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}
