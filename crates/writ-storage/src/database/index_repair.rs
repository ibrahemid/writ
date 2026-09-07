//! Recreates the notes-index tables a database lost outside the migration
//! runner.
//!
//! `run_migrations` is version-gated: it applies a migration only when its
//! version is above the one the database records. A database whose
//! `schema_version` is current but whose derived tables are gone — partial
//! corruption, a truncated write, a third-party sqlite tool — therefore has
//! nothing to bring them back, and the first
//! [`reconcile`](crate::notes_index::reconcile) fails with
//! `no such table: links`.
//!
//! What this repair may recreate is bounded by what the notes folder can
//! rebuild. `links`, `properties`, `tags` and `headings` hold nothing but
//! facts read out of the files, and `files_fts` holds their text, so all of
//! them come back from one walk. `files` does not: its columns are spread over
//! two migrations, and it is the walk's own record rather than something
//! derived from it, so a database missing it is left alone here.
//!
//! Recreating the tables empty is only half the repair. `reconcile` skips a
//! file whose size and mtime match its row, so the walk that would refill them
//! has to be told to read everything: clearing the derived-row census
//! (`schema_meta::KEY_NOTES_FACTS_CENSUS`) is what tells it, through the
//! rebuild path ADR-034 already gave it.

use crate::errors::{StorageError, StorageResult};
use crate::schema_meta;
use rusqlite::Connection;
use tracing::{info, warn};

/// The four derived tables and their indexes, the DDL migration 40 applies.
const DERIVED_DDL: &str = include_str!("notes_index_derived.sql");

/// The full-text shadow over `files`, the DDL migration 40 applies.
const FTS_DDL: &str = include_str!("notes_index_fts.sql");

/// The tables [`DERIVED_DDL`] creates, in the order a drop may take them.
const DERIVED_TABLES: &[&str] = &["links", "properties", "tags", "headings"];

/// Every object [`DERIVED_DDL`] creates. All of them have to be present for
/// the derived set to count as intact: an index missing on its own is as much
/// a hole as a table missing.
pub const DERIVED_OBJECTS: &[&str] = &[
    "links",
    "properties",
    "tags",
    "headings",
    "idx_links_from",
    "idx_links_to",
    "idx_properties_path",
    "idx_properties_key",
    "idx_tags_path",
    "idx_tags_tag",
    "idx_headings_path",
];

/// The object [`FTS_DDL`] creates. Its own shadow tables are SQLite's to keep.
pub const FTS_OBJECT: &str = "files_fts";

/// What [`repair_notes_index`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexRepairOutcome {
    /// Every object was there, and nothing was written.
    Intact,
    /// Something was missing and the tables it belongs to were recreated. The
    /// next reconcile refills them.
    Repaired,
}

/// Recreates the notes-index tables that are missing from `conn`.
///
/// Returns [`IndexRepairOutcome::Intact`] when there is nothing to do, which
/// is every launch of a healthy database: the check is a handful of
/// `sqlite_master` lookups and writes nothing.
///
/// A hole in the derived set is filled by dropping and recreating all four
/// tables together rather than the missing one alone, so the set always comes
/// from one DDL and one order. Their rows are lost in the process and the
/// walk that follows writes them back. `files_fts` is created when it is
/// missing and never dropped when it is there: it shadows `files`, whose rows
/// this repair does not touch, and dropping it would take the folder's
/// searchable text out for the length of a walk.
///
/// # Errors
///
/// [`StorageError::IndexRepair`] when a table that is missing cannot be
/// created.
pub fn repair_notes_index(conn: &Connection) -> StorageResult<IndexRepairOutcome> {
    if !object_exists(conn, "files")? {
        // Below what a repair from the notes folder can reach: files is the
        // walk's record, not a table derived from it. Recreating the tables
        // that hang off it would build an index around a hole.
        warn!("notes index has no files table; leaving the derived tables alone");
        return Ok(IndexRepairOutcome::Intact);
    }

    let mut missing_derived = Vec::new();
    for name in DERIVED_OBJECTS {
        if !object_exists(conn, name)? {
            missing_derived.push(*name);
        }
    }
    let missing_fts = !object_exists(conn, FTS_OBJECT)?;

    if missing_derived.is_empty() && !missing_fts {
        return Ok(IndexRepairOutcome::Intact);
    }

    let tx = conn
        .unchecked_transaction()
        .map_err(|e| StorageError::IndexRepair {
            message: format!("begin failed: {}", e),
        })?;

    if !missing_derived.is_empty() {
        for table in DERIVED_TABLES {
            tx.execute_batch(&format!("DROP TABLE IF EXISTS {};", table))
                .map_err(|e| StorageError::IndexRepair {
                    message: format!("dropping {} failed: {}", table, e),
                })?;
        }
        tx.execute_batch(DERIVED_DDL)
            .map_err(|e| StorageError::IndexRepair {
                message: format!(
                    "recreating the derived tables failed after {} was found missing: {}",
                    missing_derived.join(", "),
                    e
                ),
            })?;
    }

    if missing_fts {
        tx.execute_batch(FTS_DDL)
            .map_err(|e| StorageError::IndexRepair {
                message: format!("recreating {} failed: {}", FTS_OBJECT, e),
            })?;
    }

    // Inside the transaction: the tables and the census that sends the walk
    // over them again are one change.
    schema_meta::clear(conn, schema_meta::KEY_NOTES_FACTS_CENSUS)?;

    tx.commit().map_err(|e| StorageError::IndexRepair {
        message: format!("commit failed: {}", e),
    })?;

    info!(
        derived = missing_derived.join(", "),
        files_fts = missing_fts,
        "recreated missing notes index tables"
    );
    Ok(IndexRepairOutcome::Repaired)
}

/// Whether `name` is a table, virtual table or index the database holds.
fn object_exists(conn: &Connection, name: &str) -> StorageResult<bool> {
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE name = ?1",
        [name],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}
