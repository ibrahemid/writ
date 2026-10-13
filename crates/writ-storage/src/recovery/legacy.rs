use std::collections::HashMap;

use chrono::{DateTime, NaiveDateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use tracing::warn;
use writ_core::recovery::{resolve_recovery, RecoveryResolution};

use crate::errors::StorageResult;
use crate::recovery::journal;

/// The key a 0.5 snapshot kept its map of note text under, by buffer id.
const LEGACY_TEXT_KEY: &str = "buffers";

/// What the carry-over of 0.5's snapshots did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LegacyCarryOver {
    /// Entries moved into the unsaved-text journal.
    pub journaled: usize,
    /// Snapshot rows whose text was emptied out.
    pub emptied: usize,
    /// Snapshot rows whose text did not decode, logged and left as they were.
    pub undecodable: usize,
}

/// One `session_snapshots` row as it was read.
struct Row {
    rowid: i64,
    id: String,
    state_json: Result<String, rusqlite::Error>,
    created_at: String,
    is_clean: bool,
}

/// Moves the text a 0.5 snapshot was holding into the journal, then empties
/// every snapshot row of note text (ADR-044 §7).
///
/// 0.5 restored from its latest snapshot when that snapshot was unclean,
/// taking each entry newer than its note's row
/// ([`writ_core::recovery::resolve_recovery`]). That snapshot can hold text a
/// quit was handed after a failed save, which nothing else holds, so the same
/// entries go into the journal under the same rule and the launch restores
/// them from there. Every other `buffers` map is a copy of what the files held
/// and is dropped, which leaves `writ.db` holding no copy of a file's text
/// (ADR-028 §1).
///
/// A row whose state or `buffers` map does not decode is logged and left
/// exactly as it is; it is never emptied and never pruned
/// ([`super::snapshot::SnapshotManager::prune_old_snapshots`]). An entry for a
/// note whose row is gone is dropped, as 0.5 dropped it, and so is one for a
/// read-only row.
///
/// Runs on every launch and does nothing once no row holds text. The journal
/// writes and the emptying commit together, so a launch that stops halfway
/// leaves the snapshots as they were for the next one.
pub fn carry_over_legacy_snapshots(conn: &Connection) -> StorageResult<LegacyCarryOver> {
    let rows = read_rows(conn)?;
    let mut outcome = LegacyCarryOver::default();
    if rows.is_empty() {
        return Ok(outcome);
    }

    let tx = conn.unchecked_transaction()?;
    for (position, row) in rows.iter().enumerate() {
        let state = match decode_state(row) {
            Ok(Some(state)) => state,
            Ok(None) => continue,
            Err(reason) => {
                warn!(
                    snapshot = %row.id,
                    reason = %reason,
                    "a recovery snapshot could not be decoded; it was kept"
                );
                outcome.undecodable += 1;
                continue;
            }
        };
        let (mut object, entries) = state;

        if position == 0 && !row.is_clean {
            for (buffer_id, text) in entries {
                if journal_entry(&tx, row, &buffer_id, &text)? {
                    outcome.journaled += 1;
                }
            }
        }

        object.remove(LEGACY_TEXT_KEY);
        tx.execute(
            "UPDATE session_snapshots SET state_json = ?1 WHERE rowid = ?2",
            params![
                serde_json::to_string(&serde_json::Value::Object(object))?,
                row.rowid
            ],
        )?;
        outcome.emptied += 1;
    }
    tx.commit()?;
    Ok(outcome)
}

/// Every row, newest first: the order 0.5 read its latest snapshot in.
fn read_rows(conn: &Connection) -> StorageResult<Vec<Row>> {
    let mut stmt = conn.prepare(
        "SELECT rowid, id, state_json, created_at, is_clean
         FROM session_snapshots
         ORDER BY created_at DESC, rowid DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Row {
            rowid: row.get(0)?,
            id: row.get(1)?,
            state_json: row.get::<_, String>(2),
            created_at: row.get(3)?,
            is_clean: row.get::<_, i64>(4)? != 0,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

type DecodedState = (
    serde_json::Map<String, serde_json::Value>,
    HashMap<String, String>,
);

/// The row's state and its `buffers` map, `None` for a row with no text in
/// it, or why it could not be read.
fn decode_state(row: &Row) -> Result<Option<DecodedState>, String> {
    let raw = row
        .state_json
        .as_ref()
        .map_err(|e| format!("state_json: {e}"))?;
    let state: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("state_json: {e}"))?;
    let serde_json::Value::Object(object) = state else {
        return Err("state_json: not an object".to_string());
    };
    let Some(texts) = object.get(LEGACY_TEXT_KEY) else {
        return Ok(None);
    };
    let entries: HashMap<String, String> =
        serde_json::from_value(texts.clone()).map_err(|e| format!("{LEGACY_TEXT_KEY}: {e}"))?;
    Ok(Some((object, entries)))
}

/// Journals one entry of the latest unclean snapshot when 0.5 would have
/// restored it, returning whether it did.
///
/// A read-only row is never journaled, as
/// [`crate::buffer_store::BufferStore::record_unsaved_text`] never journals
/// one: nothing in its tab was typed, and the launch refuses to write into it,
/// so its entry would never clear.
fn journal_entry(conn: &Connection, row: &Row, buffer_id: &str, text: &str) -> StorageResult<bool> {
    let note: Option<(Option<String>, String, bool)> = conn
        .query_row(
            "SELECT source_path, updated_at, read_only FROM buffers WHERE id = ?1",
            params![buffer_id],
            |found| Ok((found.get(0)?, found.get(1)?, found.get::<_, i64>(2)? != 0)),
        )
        .optional()?;
    let Some((source_path, updated_at, read_only)) = note else {
        return Ok(false);
    };
    if read_only {
        return Ok(false);
    }
    // The row's stamp in the shape 0.5 compared it in. A stamp that does not
    // parse is no reason to drop text, so the entry is kept.
    if let Ok(updated_at) = DateTime::parse_from_rfc3339(&updated_at) {
        let updated_at = updated_at
            .with_timezone(&Utc)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        if resolve_recovery(&row.created_at, &updated_at) != RecoveryResolution::Restore {
            return Ok(false);
        }
    }
    let recorded_at = NaiveDateTime::parse_from_str(&row.created_at, "%Y-%m-%d %H:%M:%S")
        .map(|at| at.and_utc())
        .unwrap_or_else(|_| Utc::now());
    let hash = journal::text_hash(text);
    journal::upsert(
        conn,
        buffer_id,
        source_path.as_deref(),
        text,
        &hash,
        recorded_at,
    )?;
    Ok(true)
}
