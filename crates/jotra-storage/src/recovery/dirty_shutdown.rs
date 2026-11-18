use rusqlite::Connection;

use crate::errors::StorageResult;
use crate::recovery::snapshot::SnapshotManager;

/// Returns `true` when the most recent session snapshot lacks a clean
/// marker, indicating the previous run did not shut down gracefully.
///
/// Returns `false` when no snapshot exists (for example, a fresh
/// install).
pub fn check_dirty_shutdown(conn: &Connection) -> StorageResult<bool> {
    let manager = SnapshotManager::new(conn);
    match manager.latest_snapshot()? {
        Some(snapshot) => Ok(!snapshot.is_clean),
        None => Ok(false),
    }
}
