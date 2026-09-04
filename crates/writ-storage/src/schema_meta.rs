//! Key/value rows recording what a one-time schema pass did.
//!
//! `schema_meta` (migration 040) holds the small facts that outlive a single
//! launch and have no home in a typed table: when the notes migration ran,
//! the report it produced, and where the pre-migration copy of the database
//! sits. Each row carries an RFC 3339 `updated_at` stamp written on every
//! set.

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};

use crate::errors::StorageResult;

/// When the notes migration last completed, as an RFC 3339 timestamp.
pub const KEY_NOTES_MIGRATION_RAN_AT: &str = "notes_migration_ran_at";
/// The JSON report of the last notes migration run.
pub const KEY_NOTES_MIGRATION_REPORT: &str = "notes_migration_report";
/// When the user dismissed the notes-migration report, as an RFC 3339
/// timestamp. Present means dismissed; the report is shown once (ADR-028
/// section 4 step 5).
pub const KEY_NOTES_MIGRATION_REPORT_DISMISSED: &str = "notes_migration_report_dismissed";
/// Absolute path of the pre-migration copy of the database.
pub const KEY_ROLLBACK_COPY_PATH: &str = "notes_migration_rollback_path";
/// How many launches the pre-migration copy has survived.
pub const KEY_ROLLBACK_COPY_LAUNCHES: &str = "notes_migration_rollback_launches";

/// Reads a `schema_meta` value, or `None` when the key has no row.
pub fn get(conn: &Connection, key: &str) -> StorageResult<Option<String>> {
    let value = conn
        .query_row(
            "SELECT value FROM schema_meta WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()?;
    Ok(value)
}

/// Writes a `schema_meta` value, replacing any existing one and stamping
/// `updated_at` with the current RFC 3339 time.
pub fn set(conn: &Connection, key: &str, value: &str) -> StorageResult<()> {
    conn.execute(
        "INSERT INTO schema_meta (key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![key, value, Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

/// Deletes a `schema_meta` row. Clearing a key that has no row succeeds.
pub fn clear(conn: &Connection, key: &str) -> StorageResult<()> {
    conn.execute("DELETE FROM schema_meta WHERE key = ?1", params![key])?;
    Ok(())
}
