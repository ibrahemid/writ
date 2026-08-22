use rusqlite::Connection;
use tracing::{info, warn};
use writ_core::maintenance::needs_vacuum;

use crate::errors::StorageResult;

/// Page accounting for a database file, read from SQLite's pragmas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DatabaseStats {
    /// Total pages in the main database file.
    pub page_count: u64,
    /// Pages on the freelist, holding no live data.
    pub freelist_count: u64,
    /// Size of one page in bytes.
    pub page_size: u64,
}

impl DatabaseStats {
    /// File size in bytes implied by the page count.
    pub fn file_bytes(&self) -> u64 {
        self.page_count * self.page_size
    }

    /// Bytes held by freelist pages.
    pub fn free_bytes(&self) -> u64 {
        self.freelist_count * self.page_size
    }
}

/// What a maintenance pass did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaintenanceOutcome {
    /// Page accounting before the pass.
    pub before: DatabaseStats,
    /// Page accounting after the pass.
    pub after: DatabaseStats,
    /// Whether a `VACUUM` ran.
    pub vacuumed: bool,
}

/// Reads page counts and page size from `conn`.
pub fn read_stats(conn: &Connection) -> StorageResult<DatabaseStats> {
    let page_count: i64 = conn.query_row("PRAGMA page_count", [], |row| row.get(0))?;
    let freelist_count: i64 = conn.query_row("PRAGMA freelist_count", [], |row| row.get(0))?;
    let page_size: i64 = conn.query_row("PRAGMA page_size", [], |row| row.get(0))?;
    Ok(DatabaseStats {
        page_count: page_count.max(0) as u64,
        freelist_count: freelist_count.max(0) as u64,
        page_size: page_size.max(0) as u64,
    })
}

/// Folds the write-ahead log back into the database and truncates it to zero.
///
/// Returns `false` when SQLite reported the checkpoint as busy, which leaves
/// the log in place until the next attempt.
pub fn checkpoint_truncate(conn: &Connection) -> StorageResult<bool> {
    let busy: i64 = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))?;
    Ok(busy == 0)
}

/// Reclaims free pages when they dominate the file, then truncates the log.
///
/// `VACUUM` rewrites the database, so it needs free disk space of roughly the
/// live data size and exclusive access. A failure is logged and reported, not
/// propagated as fatal: an unvacuumed database is still fully usable.
pub fn run_maintenance(conn: &Connection) -> StorageResult<MaintenanceOutcome> {
    if let Err(e) = checkpoint_truncate(conn) {
        warn!(error = %e, "wal checkpoint before maintenance failed");
    }

    let before = read_stats(conn)?;
    let mut vacuumed = false;

    if needs_vacuum(before.page_count, before.freelist_count) {
        info!(
            file_bytes = before.file_bytes(),
            free_bytes = before.free_bytes(),
            "database is mostly free pages; vacuuming"
        );
        match conn.execute_batch("VACUUM") {
            Ok(()) => vacuumed = true,
            Err(e) => warn!(error = %e, "vacuum failed; leaving the database as is"),
        }
        if let Err(e) = checkpoint_truncate(conn) {
            warn!(error = %e, "wal checkpoint after vacuum failed");
        }
    }

    let after = read_stats(conn)?;
    if vacuumed {
        info!(
            before_bytes = before.file_bytes(),
            after_bytes = after.file_bytes(),
            "database vacuumed"
        );
    }

    Ok(MaintenanceOutcome {
        before,
        after,
        vacuumed,
    })
}
