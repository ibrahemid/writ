use rusqlite::Connection;

use crate::errors::StorageResult;
use crate::recovery::snapshot::SnapshotManager;

pub fn check_dirty_shutdown(conn: &Connection) -> StorageResult<bool> {
    let manager = SnapshotManager::new(conn);
    match manager.latest_snapshot()? {
        Some(snapshot) => Ok(!snapshot.is_clean),
        None => Ok(false),
    }
}
