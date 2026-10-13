use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use tracing::warn;
use writ_core::recovery::UnsavedText;

use crate::errors::StorageResult;

/// A journal row that could not be read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndecodableEntry {
    /// The row's SQLite rowid, which is what names it in the log.
    pub rowid: i64,
    /// Which column did not decode, and why.
    pub reason: String,
}

/// What the journal holds, as far as it could be read.
#[derive(Debug, Default)]
pub struct JournalRead {
    /// Every row that decoded, oldest first.
    pub entries: Vec<UnsavedText>,
    /// Every row that did not. Each was logged and is still in the table.
    pub undecodable: Vec<UndecodableEntry>,
}

/// Persistence facade over the unsaved-text journal (ADR-044).
///
/// An entry is text a note's save could not write. While the tab that typed it
/// is the note's tab, it is that tab's entry, one per note at most, which the
/// tab's next save replaces or clears. When the note's tab stops being that
/// tab, because the note came back from history or the app launched again, the
/// entry is released: no save touches it, and a launch puts its text on disk.
/// A note can hold any number of released entries. An entry keeps its own
/// lifetime: deleting the note's row leaves it, until a launch puts its text
/// on disk or the person discards it.
pub struct UnsavedJournal<'a> {
    conn: &'a Connection,
}

impl<'a> UnsavedJournal<'a> {
    /// Constructs a journal over the given connection.
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Keeps `text` as what note `buffer_id`'s tab holds that no file does,
    /// replacing what the tab's entry held before. Released entries are left
    /// as they are.
    ///
    /// The write is made durable when it commits, rather than at the next
    /// checkpoint ([`with_full_sync`]): this is the record a power cut is
    /// for. Text is compared by its hash: when the tab's entry already holds
    /// text with the same hash, under the same path, nothing is written and
    /// the stored text is not read back, so a save retried with unchanged
    /// text costs one hash and one small read however long the text is.
    ///
    /// # Errors
    ///
    /// [`crate::errors::StorageError::Database`] when the note has no row
    /// (an entry starts from one) or the write fails.
    pub fn record(
        &self,
        buffer_id: &str,
        path: Option<&str>,
        text: &str,
        recorded_at: DateTime<Utc>,
    ) -> StorageResult<()> {
        self.conn.query_row(
            "SELECT 1 FROM buffers WHERE id = ?1",
            params![buffer_id],
            |_| Ok(()),
        )?;
        let hash = text_hash(text);
        if self.tab_entry_holds(buffer_id, path, &hash)? {
            return Ok(());
        }
        with_full_sync(self.conn, || {
            upsert(self.conn, buffer_id, path, text, &hash, recorded_at)
        })
    }

    /// Whether note `buffer_id`'s tab entry holds text whose hash is `hash`,
    /// under `path`.
    fn tab_entry_holds(
        &self,
        buffer_id: &str,
        path: Option<&str>,
        hash: &str,
    ) -> StorageResult<bool> {
        let held: Option<(String, Option<String>)> = self
            .conn
            .query_row(
                "SELECT text_hash, path FROM unsaved_text WHERE buffer_id = ?1 AND in_tab = 1",
                params![buffer_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        Ok(held.is_some_and(|(held_hash, held_path)| {
            held_hash == hash && held_path.as_deref() == path
        }))
    }

    /// Drops the entry of note `buffer_id`'s tab, returning whether there was
    /// one. Released entries stay: no save of the tab carries their text.
    pub fn clear(&self, buffer_id: &str) -> StorageResult<bool> {
        let removed = self.conn.execute(
            "DELETE FROM unsaved_text WHERE buffer_id = ?1 AND in_tab = 1",
            params![buffer_id],
        )?;
        Ok(removed > 0)
    }

    /// Releases the entry of note `buffer_id`'s tab, returning whether there
    /// was one: the note's tab now shows something other than that text, so
    /// none of its saves may clear or replace it.
    pub fn release(&self, buffer_id: &str) -> StorageResult<bool> {
        let released = self.conn.execute(
            "UPDATE unsaved_text SET in_tab = 0 WHERE buffer_id = ?1 AND in_tab = 1",
            params![buffer_id],
        )?;
        Ok(released > 0)
    }

    /// Releases every tab's entry, returning how many there were. A launch
    /// starts here: each tab comes up showing its file.
    pub fn release_all(&self) -> StorageResult<usize> {
        Ok(self
            .conn
            .execute("UPDATE unsaved_text SET in_tab = 0 WHERE in_tab = 1", [])?)
    }

    /// Makes entry `entry_id` its note's tab entry again, for text a launch
    /// hands to the tab, returning whether the entry was there.
    ///
    /// # Errors
    ///
    /// [`crate::errors::StorageError::Database`] when the note's tab already
    /// has an entry (a note has one at most) or the write fails.
    pub fn give_to_tab(&self, entry_id: i64) -> StorageResult<bool> {
        let given = self.conn.execute(
            "UPDATE unsaved_text SET in_tab = 1 WHERE id = ?1",
            params![entry_id],
        )?;
        Ok(given > 0)
    }

    /// Drops entry `entry_id`, whose text a launch has put on disk, returning
    /// whether it was there.
    pub fn remove(&self, entry_id: i64) -> StorageResult<bool> {
        let removed = self
            .conn
            .execute("DELETE FROM unsaved_text WHERE id = ?1", params![entry_id])?;
        Ok(removed > 0)
    }

    /// Whether note `buffer_id` has an entry, decodable or not.
    pub fn holds(&self, buffer_id: &str) -> StorageResult<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM unsaved_text WHERE buffer_id = ?1",
                params![buffer_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// How many entries the journal holds, decodable or not.
    pub fn count(&self) -> StorageResult<usize> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM unsaved_text", [], |row| row.get(0))?;
        Ok(usize::try_from(count).unwrap_or(0))
    }

    /// Reads every entry, oldest first.
    ///
    /// A row that does not decode (text that is not UTF-8, a time that does
    /// not parse) is logged by its rowid and left in the table. It holds text
    /// that reached no file, so removing it is the one thing that cannot be
    /// undone; the rest of the journal is read regardless.
    pub fn read(&self) -> StorageResult<JournalRead> {
        let mut stmt = self.conn.prepare(
            "SELECT id, buffer_id, path, text, recorded_at, in_tab
             FROM unsaved_text
             ORDER BY recorded_at, id",
        )?;
        let mut rows = stmt.query([])?;
        let mut read = JournalRead::default();
        while let Some(row) = rows.next()? {
            let rowid: i64 = row.get(0)?;
            match decode(row) {
                Ok(entry) => read.entries.push(entry),
                Err(reason) => {
                    warn!(
                        row = rowid,
                        reason = %reason,
                        "an unsaved-text journal entry could not be decoded; it was kept"
                    );
                    read.undecodable.push(UndecodableEntry { rowid, reason });
                }
            }
        }
        Ok(read)
    }
}

/// Writes the note's tab entry, or replaces it when its text or path changed,
/// telling the texts apart by `hash` ([`text_hash`]). No durability setting
/// of its own: [`UnsavedJournal::record`] sets one, and a caller inside a
/// transaction has its commit to answer for it.
pub(crate) fn upsert(
    conn: &Connection,
    buffer_id: &str,
    path: Option<&str>,
    text: &str,
    hash: &str,
    recorded_at: DateTime<Utc>,
) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT INTO unsaved_text (buffer_id, path, text, text_hash, recorded_at, in_tab)
         VALUES (?1, ?2, ?3, ?4, ?5, 1)
         ON CONFLICT (buffer_id) WHERE in_tab = 1 DO UPDATE SET
             path = excluded.path,
             text = excluded.text,
             text_hash = excluded.text_hash,
             recorded_at = excluded.recorded_at
         WHERE unsaved_text.text_hash IS NOT excluded.text_hash
            OR unsaved_text.path IS NOT excluded.path",
        params![buffer_id, path, text, hash, recorded_at.to_rfc3339()],
    )
}

/// The hash an entry's text is compared by: the SHA-256 of its UTF-8 bytes,
/// in hex.
pub(crate) fn text_hash(text: &str) -> String {
    writ_core::hash::sha256_hex(text.as_bytes())
}

fn decode(row: &rusqlite::Row<'_>) -> Result<UnsavedText, String> {
    let entry_id: i64 = row.get(0).map_err(|e| format!("id: {e}"))?;
    let buffer_id: String = row.get(1).map_err(|e| format!("buffer_id: {e}"))?;
    let path: Option<String> = row.get(2).map_err(|e| format!("path: {e}"))?;
    let text: String = row.get(3).map_err(|e| format!("text: {e}"))?;
    let recorded_at: String = row.get(4).map_err(|e| format!("recorded_at: {e}"))?;
    let recorded_at = DateTime::parse_from_rfc3339(&recorded_at)
        .map(|at| at.with_timezone(&Utc))
        .map_err(|e| format!("recorded_at: {e}"))?;
    let in_tab: i64 = row.get(5).map_err(|e| format!("in_tab: {e}"))?;
    Ok(UnsavedText {
        entry_id,
        buffer_id,
        path,
        text,
        recorded_at,
        in_tab: in_tab != 0,
    })
}

/// Runs `write` with the connection's `synchronous` level at FULL, then puts
/// the level back whatever the write did.
///
/// Writ's connections run WAL at NORMAL (`database::connection`), where a
/// commit survives the process being killed but a power cut or an OS crash
/// can roll the last commits back, because the log is synced at checkpoints
/// rather than at every commit. FULL syncs the log as the write commits.
fn with_full_sync(
    conn: &Connection,
    write: impl FnOnce() -> rusqlite::Result<usize>,
) -> StorageResult<()> {
    let level: i64 = conn.pragma_query_value(None, "synchronous", |row| row.get(0))?;
    conn.pragma_update(None, "synchronous", "full")?;
    let written = write();
    let restored = conn.pragma_update(None, "synchronous", level);
    written?;
    restored?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::with_full_sync;

    fn synchronous(conn: &Connection) -> i64 {
        conn.pragma_query_value(None, "synchronous", |row| row.get(0))
            .expect("level")
    }

    #[test]
    fn the_write_runs_at_full_and_the_level_comes_back() {
        let conn = Connection::open_in_memory().expect("open");
        conn.pragma_update(None, "synchronous", "normal")
            .expect("normal");
        let mut during = None;
        with_full_sync(&conn, || {
            during = Some(synchronous(&conn));
            Ok(0)
        })
        .expect("write");
        assert_eq!(during, Some(2), "FULL is 2");
        assert_eq!(synchronous(&conn), 1, "NORMAL is 1");
    }
}
