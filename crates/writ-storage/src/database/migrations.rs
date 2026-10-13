use crate::database::index_repair::{object_exists, repair_notes_index};
use crate::errors::{StorageError, StorageResult};
use rusqlite::Connection;
use tracing::info;

const MIGRATIONS: &[(i32, &str)] = &[
    (1, include_str!("../../migrations/001_initial.sql")),
    (10, include_str!("../../migrations/010_layout_state.sql")),
    (
        20,
        include_str!("../../migrations/020_buffer_open_mode.sql"),
    ),
    (30, include_str!("../../migrations/030_fts_prefix.sql")),
    (
        40,
        concat!(
            include_str!("../../migrations/040_notes_migration.sql"),
            include_str!("schema_meta.sql"),
            include_str!("notes_index_derived.sql"),
            include_str!("notes_index_fts.sql"),
        ),
    ),
    (41, include_str!("../../migrations/041_drop_buffer_fts.sql")),
    (
        42,
        include_str!("../../migrations/042_notes_indexed_by.sql"),
    ),
    (
        43,
        include_str!("../../migrations/043_buffer_line_ending.sql"),
    ),
    // A column on a derived table, added by `migrate_rows` (045_links_name_key.sql).
    (45, ""),
];

/// Work a migration needs that its SQL cannot do, run inside the same
/// transaction as the SQL so the two are applied together or not at all.
fn migrate_rows(version: i32, conn: &Connection) -> StorageResult<()> {
    match version {
        45 => add_link_name_keys(conn),
        _ => Ok(()),
    }
}

/// Migration 45: `links.name_key`, its index, and the key of every link
/// already stored.
///
/// The key is a Unicode fold of each stored target, which SQL has no function
/// for, so the rows are filled from here. A database that lost `links` has
/// nothing to add the column to; the repair that ends the run recreates the
/// table with it ([`repair_notes_index`]), and the next walk fills it. A
/// `links` that already carries the column, written by a build that numbered
/// this migration 44, only has its keys filled.
fn add_link_name_keys(conn: &Connection) -> StorageResult<()> {
    if !object_exists(conn, "links")? {
        return Ok(());
    }
    if !column_exists(conn, "links", "name_key")? {
        conn.execute_batch(include_str!("../../migrations/045_links_name_key.sql"))?;
    }
    crate::notes_index::backfill_link_name_keys(conn)?;
    Ok(())
}

/// Whether `table` has a column called `column`.
fn column_exists(conn: &Connection, table: &str, column: &str) -> StorageResult<bool> {
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM pragma_table_info(?1) WHERE name = ?2",
        [table, column],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// Highest migration version embedded in this binary.
pub fn binary_schema_version() -> i32 {
    MIGRATIONS.iter().map(|(v, _)| *v).max().unwrap_or(0)
}

/// Highest migration version the database at `conn` has recorded, or `0` when
/// the table is there and empty.
///
/// Reads only, so a read-only connection can ask. A second process compares the
/// answer against [`binary_schema_version`] to find out whether the database it
/// opened was written by the build it is part of.
///
/// A failure is returned rather than reported as version `0`. The two are
/// different things to tell a reader: a database at an older version is one
/// Writ brings up to date on its next run, while a file that cannot be queried
/// at all is not a database Writ can do anything with, and the caller that
/// says so is the only one that can tell them apart.
pub fn applied_schema_version(conn: &Connection) -> StorageResult<i32> {
    Ok(conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_version",
        [],
        |row| row.get(0),
    )?)
}

/// Applies every pending schema migration to `conn`.
///
/// Migrations are embedded at compile time and tracked in a
/// `schema_version` table. A migration is applied when its version is
/// strictly greater than the highest previously applied version. The
/// function is idempotent: calling it on an up-to-date database is a
/// no-op.
///
/// Before applying anything, the runner enforces a downgrade guard: if
/// the database records a `schema_version` strictly greater than the
/// highest version this binary embeds, it was written by a newer build
/// and is refused with [`StorageError::SchemaTooNew`] rather than read
/// through a stale column layout (audit blocker #53.8).
///
/// A version-gated runner recreates nothing in a database that records a
/// current version, so the pass ends with
/// [`repair_notes_index`](crate::database::index_repair::repair_notes_index),
/// which puts back the notes-index tables a database lost some other way.
pub fn run_migrations(conn: &Connection) -> StorageResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        );",
    )?;

    let current_version = applied_schema_version(conn)?;

    let binary_version = binary_schema_version();
    if current_version > binary_version {
        return Err(StorageError::SchemaTooNew {
            db_version: current_version,
            binary_version,
        });
    }

    for (version, sql) in MIGRATIONS {
        if *version > current_version {
            // Apply the migration body and stamp its version in one transaction
            // so a mid-batch failure rolls back completely. Without this a
            // multi-statement migration (e.g. the FTS rebuild in v30: create,
            // copy, drop, rename) could partially apply, leave the version
            // unrecorded, and brick the next launch when it re-creates an
            // already-existing object. SQLite FTS5 DDL is transaction-safe.
            let tx = conn
                .unchecked_transaction()
                .map_err(|e| StorageError::Migration {
                    message: format!("migration v{} begin failed: {}", version, e),
                })?;
            tx.execute_batch(sql).map_err(|e| StorageError::Migration {
                message: format!("migration v{} failed: {}", version, e),
            })?;
            migrate_rows(*version, &tx).map_err(|e| StorageError::Migration {
                message: format!("migration v{} failed: {}", version, e),
            })?;
            tx.execute(
                "INSERT INTO schema_version (version, applied_at) VALUES (?1, datetime('now'))",
                [version],
            )?;
            tx.commit().map_err(|e| StorageError::Migration {
                message: format!("migration v{} commit failed: {}", version, e),
            })?;
            info!(version = version, "applied migration");
        }
    }

    repair_notes_index(conn)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::MIGRATIONS;

    /// The runner applies the pending migrations in the order this list holds
    /// them, so a version listed above a lower one would run before it on a
    /// database that needs both.
    #[test]
    fn the_migrations_are_listed_in_the_order_of_their_versions() {
        let versions: Vec<i32> = MIGRATIONS.iter().map(|(version, _)| *version).collect();
        assert!(
            versions.windows(2).all(|pair| pair[0] < pair[1]),
            "{versions:?}"
        );
    }
}
