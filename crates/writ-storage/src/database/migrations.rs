use crate::errors::{StorageError, StorageResult};
use rusqlite::Connection;
use tracing::info;

const MIGRATIONS: &[(i32, &str)] = &[(1, include_str!("../../migrations/001_initial.sql"))];

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
