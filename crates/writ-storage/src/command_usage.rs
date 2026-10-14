//! How often each palette command ran, in the `command_usage` table
//! (migration 049, ADR-048).
//!
//! The store holds its own connection, like [`crate::layout_state`], so a
//! command run never queues behind a save on the buffer store's.

use std::collections::{HashMap, HashSet};
use std::sync::{LockResult, Mutex, MutexGuard};

use rusqlite::{params, Connection};
use writ_core::command::usage::{CommandUsage, CommandUse};

use crate::errors::StorageResult;
use crate::schema_meta;

fn recover_conn(result: LockResult<MutexGuard<'_, Connection>>) -> MutexGuard<'_, Connection> {
    result.unwrap_or_else(|poisoned| {
        tracing::error!(location = "command_usage", "recovered poisoned mutex");
        poisoned.into_inner()
    })
}

/// SQLite stores integers as `i64`; a count or a time past that range is held
/// at its top rather than refused.
fn to_sql(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn count_from_sql(value: i64) -> u32 {
    u32::try_from(value.max(0)).unwrap_or(u32::MAX)
}

fn ms_from_sql(value: i64) -> u64 {
    u64::try_from(value.max(0)).unwrap_or(0)
}

const UPSERT_ADDING: &str =
    "INSERT INTO command_usage (id, count, last_used_ms) VALUES (?1, ?2, ?3)
     ON CONFLICT(id) DO UPDATE SET
         count = min(command_usage.count + excluded.count, 4294967295),
         last_used_ms = max(command_usage.last_used_ms, excluded.last_used_ms)";

const UPSERT_KEEPING_LARGER: &str =
    "INSERT INTO command_usage (id, count, last_used_ms) VALUES (?1, ?2, ?3)
     ON CONFLICT(id) DO UPDATE SET
         count = max(command_usage.count, excluded.count),
         last_used_ms = max(command_usage.last_used_ms, excluded.last_used_ms)";

/// Repository for the `command_usage` table.
pub struct CommandUsageStore {
    conn: Mutex<Connection>,
}

impl CommandUsageStore {
    /// Wraps an open connection to a database migrations have run on.
    pub fn new(conn: Connection) -> Self {
        Self {
            conn: Mutex::new(conn),
        }
    }

    /// Every command's usage, keyed by command id.
    pub fn all(&self) -> StorageResult<HashMap<String, CommandUsage>> {
        let conn = recover_conn(self.conn.lock());
        let mut statement = conn.prepare("SELECT id, count, last_used_ms FROM command_usage")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                CommandUsage {
                    count: count_from_sql(row.get(1)?),
                    last_used_ms: ms_from_sql(row.get(2)?),
                },
            ))
        })?;
        let mut usage = HashMap::new();
        for row in rows {
            let (id, entry) = row?;
            usage.insert(id, entry);
        }
        Ok(usage)
    }

    /// Adds `uses` to what is stored, in one transaction. A command's count
    /// grows by each use's count and its time moves forward only.
    pub fn record(&self, uses: &[CommandUse]) -> StorageResult<()> {
        if uses.is_empty() {
            return Ok(());
        }
        let conn = recover_conn(self.conn.lock());
        let tx = conn.unchecked_transaction()?;
        {
            let mut statement = tx.prepare(UPSERT_ADDING)?;
            for item in uses {
                statement.execute(params![
                    item.id,
                    to_sql(u64::from(item.count)),
                    to_sql(item.last_used_ms)
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Forgets every command's usage.
    pub fn clear(&self) -> StorageResult<()> {
        let conn = recover_conn(self.conn.lock());
        conn.execute("DELETE FROM command_usage", [])?;
        Ok(())
    }

    /// Drops the rows of commands that no longer exist. Returns how many went.
    pub fn prune(&self, known: &HashSet<String>) -> StorageResult<usize> {
        let conn = recover_conn(self.conn.lock());
        let tx = conn.unchecked_transaction()?;
        let stale: Vec<String> = {
            let mut statement = tx.prepare("SELECT id FROM command_usage")?;
            let ids = statement.query_map([], |row| row.get::<_, String>(0))?;
            let mut stale = Vec::new();
            for id in ids {
                let id = id?;
                if !known.contains(&id) {
                    stale.push(id);
                }
            }
            stale
        };
        for id in &stale {
            tx.execute("DELETE FROM command_usage WHERE id = ?1", params![id])?;
        }
        tx.commit()?;
        Ok(stale.len())
    }

    /// Moves the usage a `config.toml` written before ADR-048 carried into the
    /// table, once.
    ///
    /// The rows and the mark that the move happened commit together, so a
    /// launch that stops half way moves everything on the next one, and a
    /// launch after the move does nothing even if the file still carries the
    /// old table. A command already in the table keeps the larger count and the
    /// later time, so a move that does repeat never counts a run twice.
    /// Returns whether this call did the move.
    pub fn import_once(&self, usage: &HashMap<String, CommandUsage>) -> StorageResult<bool> {
        let conn = recover_conn(self.conn.lock());
        let tx = conn.unchecked_transaction()?;
        if schema_meta::get(&tx, schema_meta::KEY_COMMAND_USAGE_IMPORTED_AT)?.is_some() {
            return Ok(false);
        }
        {
            let mut statement = tx.prepare(UPSERT_KEEPING_LARGER)?;
            for (id, entry) in usage {
                statement.execute(params![
                    id,
                    to_sql(u64::from(entry.count)),
                    to_sql(entry.last_used_ms)
                ])?;
            }
        }
        schema_meta::set(
            &tx,
            schema_meta::KEY_COMMAND_USAGE_IMPORTED_AT,
            &chrono::Utc::now().to_rfc3339(),
        )?;
        tx.commit()?;
        Ok(true)
    }
}
