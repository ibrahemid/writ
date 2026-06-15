use crate::errors::{StorageError, StorageResult};
use rusqlite::Connection;
use tracing::info;

const MIGRATIONS: &[(i32, &str)] = &[
    (1, include_str!("../../migrations/001_initial.sql")),
    (10, include_str!("../../migrations/010_layout_state.sql")),
    (20, include_str!("../../migrations/020_buffer_open_mode.sql")),
];

/// Highest migration version embedded in this binary.
fn binary_schema_version() -> i32 {
    MIGRATIONS.iter().map(|(v, _)| *v).max().unwrap_or(0)
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
pub fn run_migrations(conn: &Connection) -> StorageResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        );",
    )?;

    let current_version: i32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let binary_version = binary_schema_version();
    if current_version > binary_version {
        return Err(StorageError::SchemaTooNew {
            db_version: current_version,
            binary_version,
        });
    }

    for (version, sql) in MIGRATIONS {
        if *version > current_version {
            conn.execute_batch(sql)
                .map_err(|e| StorageError::Migration {
                    message: format!("migration v{} failed: {}", version, e),
                })?;
            conn.execute(
                "INSERT INTO schema_version (version, applied_at) VALUES (?1, datetime('now'))",
                [version],
            )?;
            info!(version = version, "applied migration");
        }
    }

    Ok(())
}
