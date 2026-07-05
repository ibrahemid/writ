use std::collections::HashMap;

use rusqlite::{params, Connection};
use uuid::Uuid;
use writ_core::recovery::{resolve_recovery, RecoveredBuffer, RecoveryResolution, MAX_SNAPSHOTS};

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
        self.prune_old_snapshots()?;
        Ok(())
    }

    /// Writes a session snapshot that embeds buffer contents for crash recovery.
    ///
    /// `buffer_contents` maps buffer id to current content. The snapshot
    /// encodes them under a `buffers` key alongside the rest of `extra_state`.
    /// After writing, old snapshots beyond [`MAX_SNAPSHOTS`] are pruned.
    pub fn write_session_snapshot(
        &self,
        buffer_contents: &HashMap<String, String>,
        extra_state: &serde_json::Value,
        is_clean: bool,
    ) -> StorageResult<()> {
        let mut state = extra_state.clone();
        let obj = state.as_object_mut().cloned().unwrap_or_default();
        let mut merged = obj;
        merged.insert(
            "buffers".to_string(),
            serde_json::to_value(buffer_contents)?,
        );
        let final_state = serde_json::Value::Object(merged);
        self.write_snapshot(&final_state, is_clean)
    }

    /// Removes all but the [`MAX_SNAPSHOTS`] most recent snapshot rows.
    pub fn prune_old_snapshots(&self) -> StorageResult<()> {
        self.conn.execute(
            "DELETE FROM session_snapshots
             WHERE id NOT IN (
                 SELECT id FROM session_snapshots
                 ORDER BY created_at DESC, rowid DESC
                 LIMIT ?1
             )",
            params![MAX_SNAPSHOTS as i64],
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

    /// Resolves which buffers from the latest dirty snapshot should be
    /// restored, given a map of buffer id to the buffer's current
    /// `updated_at` timestamp string.
    ///
    /// Returns an empty list when there is no dirty snapshot or when no
    /// snapshot entry is newer than the stored buffer.
    pub fn recover_buffers(
        &self,
        buffer_updated_at: &HashMap<String, String>,
    ) -> StorageResult<Vec<RecoveredBuffer>> {
        let snapshot = match self.latest_snapshot()? {
            Some(s) if !s.is_clean => s,
            _ => return Ok(Vec::new()),
        };

        let snap_buffers = match snapshot.state_json.get("buffers") {
            Some(v) => {
                let map: HashMap<String, String> =
                    serde_json::from_value(v.clone()).unwrap_or_default();
                map
            }
            None => return Ok(Vec::new()),
        };

        let mut recovered = Vec::new();
        for (id, content) in snap_buffers {
            let resolution = match buffer_updated_at.get(&id) {
                Some(updated_at) => resolve_recovery(&snapshot.created_at, updated_at),
                None => RecoveryResolution::NoSnapshot,
            };
            if resolution == RecoveryResolution::Restore {
                recovered.push(RecoveredBuffer { id, content });
            }
        }
        Ok(recovered)
    }
}
