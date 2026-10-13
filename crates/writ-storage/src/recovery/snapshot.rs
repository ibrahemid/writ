use rusqlite::{params, Connection};
use uuid::Uuid;
use writ_core::recovery::MAX_SNAPSHOTS;

use crate::errors::StorageResult;
use crate::maintenance::checkpoint_truncate;

/// A row of `session_snapshots`: whether a session was running or ended
/// cleanly.
///
/// The markers Writ writes now hold no text (ADR-044). A snapshot 0.5 wrote
/// held note text under a `buffers` key, which the first launch after the
/// upgrade moves into the unsaved-text journal and empties out of the row
/// ([`super::legacy::carry_over_legacy_snapshots`]).
pub struct SessionSnapshot {
    /// Snapshot UUID, assigned at creation.
    pub id: String,
    /// Snapshot format version. Bumped when the shape of
    /// `state_json` changes in a breaking way.
    pub format_version: i32,
    /// Encoded state: an empty object for a marker.
    pub state_json: serde_json::Value,
    /// Creation timestamp formatted as SQLite `datetime('now')`.
    pub created_at: String,
    /// `true` when written during a clean shutdown.
    pub is_clean: bool,
}

/// Persistence facade over the session markers.
pub struct SnapshotManager<'a> {
    conn: &'a Connection,
}

impl<'a> SnapshotManager<'a> {
    /// Constructs a manager over the given connection.
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Writes a new snapshot row with `state` as its encoded state, prunes,
    /// and checkpoints the write-ahead log.
    pub fn write_snapshot(&self, state: &serde_json::Value, is_clean: bool) -> StorageResult<()> {
        self.insert(state, is_clean)?;
        // Pruning frees pages inside the write-ahead log, not the database
        // file; without a checkpoint the log keeps every superseded row.
        if let Err(e) = checkpoint_truncate(self.conn) {
            tracing::warn!(error = %e, "wal checkpoint after snapshot prune failed");
        }
        Ok(())
    }

    /// Records that a session is running: an unclean marker with no text.
    ///
    /// No checkpoint follows, unlike [`Self::write_snapshot`]. The heartbeat
    /// writes this while the app runs and holds the store for it, and a
    /// checkpoint copies the whole log into the database file.
    pub fn mark_running(&self) -> StorageResult<()> {
        self.insert(&serde_json::Value::Object(serde_json::Map::new()), false)
    }

    /// Records that a session ended cleanly: a clean marker with no text,
    /// written as the last thing a quit writes.
    pub fn mark_clean(&self) -> StorageResult<()> {
        self.write_snapshot(&serde_json::Value::Object(serde_json::Map::new()), true)
    }

    fn insert(&self, state: &serde_json::Value, is_clean: bool) -> StorageResult<()> {
        let id = Uuid::new_v4().to_string();
        let state_str = serde_json::to_string(state)?;
        let is_clean_int = if is_clean { 1i64 } else { 0i64 };
        self.conn.execute(
            "INSERT INTO session_snapshots (id, format_version, state_json, created_at, is_clean)
             VALUES (?1, 1, ?2, datetime('now'), ?3)",
            params![id, state_str, is_clean_int],
        )?;
        self.prune_old_snapshots()
    }

    /// Removes all but the [`MAX_SNAPSHOTS`] most recent rows.
    ///
    /// A row that still holds text is not pruned: a 0.5 snapshot whose text
    /// could not be decoded, which the upgrade logs and leaves where it is
    /// rather than drop (ADR-044 §7), and a row whose state is not JSON at
    /// all, which is the same thing seen from further away.
    pub fn prune_old_snapshots(&self) -> StorageResult<()> {
        self.conn.execute(
            "DELETE FROM session_snapshots
             WHERE id NOT IN (
                 SELECT id FROM session_snapshots
                 ORDER BY created_at DESC, rowid DESC
                 LIMIT ?1
             )
             AND json_valid(state_json)
             AND json_type(state_json, '$.buffers') IS NULL",
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
}
