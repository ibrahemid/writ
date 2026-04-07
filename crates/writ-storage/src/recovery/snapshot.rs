use rusqlite::{params, Connection};
use uuid::Uuid;

use crate::errors::StorageResult;

pub struct SessionSnapshot {
    pub id: String,
    pub format_version: i32,
    pub state_json: serde_json::Value,
    pub created_at: String,
    pub is_clean: bool,
}

pub struct SnapshotManager<'a> {
    conn: &'a Connection,
}

impl<'a> SnapshotManager<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

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
