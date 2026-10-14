//! What the index records beside its rows: the folded name every link row
//! carries (migration 045), the folded search text of the notes in Han, kana,
//! Hangul or Arabic script (migration 051), the version of the set of paths it
//! holds, and the census a complete walk leaves for the next one.

use std::collections::{HashMap, HashSet};

use rusqlite::{params, Connection};
use writ_core::notes::{facts, links};
use writ_core::search::{fold_for_search, needs_folded_index};

use super::IndexedBy;
use crate::errors::StorageResult;
use crate::schema_meta;

/// The value `links.name_key` holds for a link stored with `to_target`: the
/// folded name of the note it names.
///
/// A path answers to the keys [`links::candidate_name_keys`] gives it, and a
/// link names exactly one of those, so a note that arrives or leaves finds the
/// links naming it with one indexed lookup per key instead of reading every
/// link in the folder.
pub(crate) fn link_name_key(to_target: &str) -> String {
    links::name_key(&links::stored_target(to_target).name)
}

/// Fills `links.name_key` for every row that has none, which is every row a
/// database held when migration 045 added the column. Returns the rows filled.
///
/// Runs inside the migration's own transaction, so the column and its values
/// arrive together: a launch that stops halfway leaves the migration
/// unrecorded and the next one does all of it again. Nothing is read from the
/// notes folder; the name is already in `to_target`.
pub(crate) fn backfill_link_name_keys(conn: &Connection) -> StorageResult<usize> {
    let rows: Vec<(i64, String)> = {
        let mut stmt = conn.prepare("SELECT rowid, to_target FROM links WHERE name_key IS NULL")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    let mut keys: HashMap<String, String> = HashMap::new();
    let mut stmt = conn.prepare("UPDATE links SET name_key = ?2 WHERE rowid = ?1")?;
    for (rowid, target) in &rows {
        let key: &String = keys
            .entry(target.clone())
            .or_insert_with(|| link_name_key(target));
        stmt.execute(params![rowid, key])?;
    }
    Ok(rows.len())
}

/// Writes `rowid`'s row of `files_folded` from the note's `name` and `content`
/// when either holds a script that index is for
/// ([`writ_core::search::needs_folded_index`]), and leaves none otherwise.
///
/// Runs inside the caller's transaction, after its `files_fts` row: the two
/// indexes describe one note at one moment.
pub(crate) fn write_folded(
    conn: &Connection,
    rowid: i64,
    name: &str,
    content: &str,
) -> rusqlite::Result<()> {
    conn.prepare_cached("DELETE FROM files_folded WHERE rowid = ?1")?
        .execute(params![rowid])?;
    if !needs_folded_index(name) && !needs_folded_index(content) {
        return Ok(());
    }
    conn.prepare_cached("INSERT INTO files_folded (rowid, name, content) VALUES (?1, ?2, ?3)")?
        .execute(params![
            rowid,
            fold_for_search(name).as_ref(),
            fold_for_search(content).as_ref()
        ])?;
    Ok(())
}

/// Fills `files_folded` from the text `files_fts` already holds, for every
/// note that needs a row there. Returns the rows written.
///
/// Migration 051 runs this inside its own transaction, and the repair runs it
/// when the table was lost, so no note is read off disk to fill it: a note
/// the index holds by name alone has empty text here as in `files_fts`.
pub(crate) fn backfill_folded(conn: &Connection) -> StorageResult<usize> {
    let mut written = 0usize;
    let mut stmt = conn.prepare("SELECT rowid, name, content FROM files_fts")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let rowid: i64 = row.get(0)?;
        let name: String = row.get(1)?;
        let content: String = row.get(2)?;
        if needs_folded_index(&name) || needs_folded_index(&content) {
            write_folded(conn, rowid, &name, &content)?;
            written += 1;
        }
    }
    Ok(written)
}

/// The version of the set of paths in `files`
/// ([`schema_meta::KEY_NOTES_NAMES_VERSION`]), `0` before any write recorded
/// one.
pub(crate) fn names_version(conn: &Connection) -> StorageResult<i64> {
    Ok(
        schema_meta::get(conn, schema_meta::KEY_NOTES_NAMES_VERSION)?
            .and_then(|value| value.parse().ok())
            .unwrap_or(0),
    )
}

/// Moves the names version on by one, inside the caller's transaction, and
/// returns the value it now holds. Called by every write that adds a path to
/// `files`, removes one or re-keys them.
pub(crate) fn bump_names_version(conn: &Connection) -> StorageResult<i64> {
    let version = names_version(conn)? + 1;
    schema_meta::set(
        conn,
        schema_meta::KEY_NOTES_NAMES_VERSION,
        &version.to_string(),
    )?;
    Ok(version)
}

/// Every indexed path with the size and mtime it was indexed at.
pub(crate) fn snapshot(conn: &Connection) -> StorageResult<Vec<(String, u64, i64)>> {
    let mut stmt = conn.prepare("SELECT path, size, mtime FROM files")?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)? as u64,
                row.get::<_, i64>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// The paths whose row holds a name and no text ([`IndexedBy::Name`]).
///
/// Materialising a placeholder leaves its size and mtime where they were, so
/// the walk's unchanged shortcut cannot tell a downloaded note from the
/// placeholder it replaced. This is how it tells, and it reads a column of its
/// own so that a writer recording a digest cannot disturb it.
pub(crate) fn name_only_paths(conn: &Connection) -> StorageResult<HashSet<String>> {
    let mut stmt = conn.prepare("SELECT path FROM files WHERE indexed_by = ?1")?;
    let rows = stmt
        .query_map(params![IndexedBy::Name.as_str()], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<Result<HashSet<_>, _>>()?;
    Ok(rows)
}

/// How many rows the four derived tables hold, and how many files they were
/// derived from.
///
/// The pair a complete walk records at its end and the next walk compares
/// against at its start, which is how a walk notices that the derived tables
/// were emptied out from under it.
pub(crate) fn facts_census(conn: &Connection) -> StorageResult<(i64, i64)> {
    let census = conn.query_row(
        "SELECT (SELECT count(*) FROM links)
              + (SELECT count(*) FROM properties)
              + (SELECT count(*) FROM tags)
              + (SELECT count(*) FROM headings),
                (SELECT count(*) FROM files)",
        [],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
    )?;
    Ok(census)
}

/// The reading the last complete walk derived its rows under, or `None` when
/// nothing is recorded or the row is not a number.
pub(crate) fn read_facts_reading(conn: &Connection) -> StorageResult<Option<u32>> {
    Ok(
        schema_meta::get(conn, schema_meta::KEY_NOTES_FACTS_READING)?
            .and_then(|value| value.parse().ok()),
    )
}

/// The derived-row census the last complete walk recorded, or `None` when
/// there is none or the row is not the pair it should be.
pub(crate) fn read_facts_census(conn: &Connection) -> StorageResult<Option<(i64, i64)>> {
    let Some(value) = schema_meta::get(conn, schema_meta::KEY_NOTES_FACTS_CENSUS)? else {
        return Ok(None);
    };
    let Some((rows, files)) = value.split_once(':') else {
        return Ok(None);
    };
    Ok(match (rows.parse(), files.parse()) {
        (Ok(rows), Ok(files)) => Some((rows, files)),
        _ => None,
    })
}

/// Records what a complete walk left: the census and the reading its rows
/// were derived under. A cancelled walk records nothing, because it has
/// written fewer derived rows than the folder holds and the next walk would
/// read that as tables emptied out from under it.
pub(crate) fn record_complete_pass(conn: &Connection) -> StorageResult<()> {
    let after = facts_census(conn)?;
    let tx = conn.unchecked_transaction()?;
    schema_meta::set(
        &tx,
        schema_meta::KEY_NOTES_FACTS_CENSUS,
        &format!("{}:{}", after.0, after.1),
    )?;
    schema_meta::set(
        &tx,
        schema_meta::KEY_NOTES_FACTS_READING,
        &facts::READING.to_string(),
    )?;
    tx.commit()?;
    Ok(())
}
