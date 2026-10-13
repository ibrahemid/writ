//! The writes: one file per transaction.
//!
//! Every write goes through [`write_file`] or [`forget_file`], whether it
//! comes from a save, the notes watcher or the walk, so the index changes the
//! same way whoever changes it. Each takes one file's rows and nothing else,
//! which is what lets [`super::NotesIndexStore`] hold its writer for one file
//! at a time rather than for a whole walk.

use std::cell::RefCell;
use std::path::Path;

use rusqlite::{params, Connection, Transaction, TransactionBehavior};
use writ_core::notes::{facts, links};

use super::resolve::{self, NameIndex, NameSource};
use super::schema::{self, link_name_key};
use super::{is_note_file, root_prefix, IndexedNote, NotesIndex};
use crate::errors::StorageResult;

/// What a write expects the `files` row it replaces to hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Expect {
    /// Whatever it holds. A save and the watcher read the file a moment ago,
    /// so what they write is the newest thing anyone knows about it.
    Anything,
    /// No row. The walk found none when it decided to read the file.
    NoRow,
    /// The row the walk decided from: this size and this mtime.
    Row(u64, i64),
}

/// Opens a write transaction that holds the database's write lock from its
/// first statement, with `names` brought in line with what it sees.
///
/// Taking the lock up front is what makes the check airtight: no other
/// connection can commit between the moment the names are compared with the
/// database and the moment this transaction commits, so a note another
/// connection added is in the names before anything here resolves against
/// them.
pub(crate) fn begin_write<'c, N: NameSource>(
    conn: &'c Connection,
    names: &N,
) -> StorageResult<Transaction<'c>> {
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    names.sync(&tx)?;
    Ok(tx)
}

/// Commits `tx`, then tells `names` what the write left: the names version it
/// moved to, or, when the commit failed, that the names may hold a change the
/// database does not.
fn commit_names<N: NameSource>(
    tx: Transaction<'_>,
    names: &N,
    version: Option<i64>,
) -> StorageResult<()> {
    tx.commit()?;
    if let Some(version) = version {
        names.committed(version);
    }
    Ok(())
}

/// Records `note` and its text, replacing what the index held for that path
/// when the row still holds what `expect` says. `false` says the write was
/// declined because something newer got there first.
///
/// The `files` row is updated in place rather than replaced. `files_fts` joins
/// `files` on rowid and `links`, `properties`, `tags` and `headings` cascade
/// from `files(path)`, so an `INSERT OR REPLACE` would reassign the rowid,
/// orphan the text entry and delete every derived row a save is not entitled
/// to touch (migration 040 records the same constraint).
///
/// The four derived tables are rewritten in the same transaction, so a note
/// and the facts read out of it are never a save apart. A walk's write is
/// conditional because the walk reads a file and writes the row some
/// milliseconds later: a save landing in that window would otherwise be
/// overwritten by what the walk read before it, and nothing would correct the
/// row until the file changed again.
///
/// A path new to `names` is added to them, and the links elsewhere that name
/// it are resolved again in the same transaction: a note arriving fills in the
/// links that waited for it and makes a name it shares ambiguous (ADR-034). A
/// path already held changes no other link's answer, so a save of an existing
/// note costs its own rows and nothing else (RC-14). A new row moves the names
/// version in the same transaction. When the transaction fails, the path
/// leaves `names` again.
pub(crate) fn write_file<N: NameSource>(
    conn: &Connection,
    names: &N,
    note: &IndexedNote,
    content: &str,
    expect: Expect,
) -> StorageResult<bool> {
    let tx = begin_write(conn, names)?;
    let (wrote, created) = match expect {
        Expect::Anything => match insert_row(&tx, note)? {
            0 => (update_row(&tx, note, None)?, false),
            inserted => (inserted, true),
        },
        Expect::NoRow => {
            let inserted = insert_row(&tx, note)?;
            (inserted, inserted > 0)
        }
        Expect::Row(size, mtime) => (update_row(&tx, note, Some((size, mtime)))?, false),
    };
    if wrote == 0 {
        return Ok(false);
    }
    let rowid: i64 = tx.query_row(
        "SELECT rowid FROM files WHERE path = ?1",
        params![note.path],
        |row| row.get(0),
    )?;
    tx.execute("DELETE FROM files_fts WHERE rowid = ?1", params![rowid])?;
    tx.execute(
        "INSERT INTO files_fts (rowid, name, content) VALUES (?1, ?2, ?3)",
        params![rowid, note.name, content],
    )?;
    names.read(|index| write_facts(&tx, &note.path, content, index))?;

    let added = names.insert(&note.path);
    let version = (|| -> StorageResult<Option<i64>> {
        if added {
            names.read(|index| {
                resolve::resolve_links_named(&tx, index, &links::candidate_name_keys(&note.path))
            })?;
        }
        Ok(if created {
            Some(schema::bump_names_version(&tx)?)
        } else {
            None
        })
    })();
    if let Err(error) = version.and_then(|version| commit_names(tx, names, version)) {
        if added {
            names.remove(&note.path);
        }
        names.abandon();
        return Err(error);
    }
    Ok(true)
}

/// Inserts the `files` row for `note` unless the path has one. Returns the
/// rows inserted.
fn insert_row(tx: &Transaction<'_>, note: &IndexedNote) -> rusqlite::Result<usize> {
    tx.execute(
        "INSERT INTO files (path, size, mtime, hash, indexed_by, indexed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))
         ON CONFLICT(path) DO NOTHING",
        params![
            note.path,
            note.size as i64,
            note.mtime,
            note.hash,
            note.indexed_by.as_str()
        ],
    )
}

/// Updates the `files` row for `note` in place, only while it holds `expected`
/// size and mtime when one is given. Returns the rows updated.
fn update_row(
    tx: &Transaction<'_>,
    note: &IndexedNote,
    expected: Option<(u64, i64)>,
) -> rusqlite::Result<usize> {
    let Some((size, mtime)) = expected else {
        return tx.execute(
            "UPDATE files
                SET size = ?2, mtime = ?3, hash = ?4, indexed_by = ?5,
                    indexed_at = datetime('now')
              WHERE path = ?1",
            params![
                note.path,
                note.size as i64,
                note.mtime,
                note.hash,
                note.indexed_by.as_str()
            ],
        );
    };
    tx.execute(
        "UPDATE files
            SET size = ?2, mtime = ?3, hash = ?4, indexed_by = ?5,
                indexed_at = datetime('now')
          WHERE path = ?1 AND size = ?6 AND mtime = ?7",
        params![
            note.path,
            note.size as i64,
            note.mtime,
            note.hash,
            note.indexed_by.as_str(),
            size as i64,
            mtime
        ],
    )
}

/// Removes `key` from the index.
///
/// The text entry goes first, while the `files` row it joins on still exists;
/// the row itself follows, and `links`, `properties`, `tags` and `headings`
/// cascade with it. Losing the row without losing the text entry is what
/// produces a hit nothing can open, so every step propagates its error.
///
/// The cascade only reaches the links written *in* the removed note. The
/// links written in other notes that resolved *to* it are a plain column with
/// no foreign key behind it, so they are emptied here, and every link naming
/// it is then resolved again: a second note of the same name left standing
/// picks them up, and a link left pointing at a deleted file would read as
/// resolved and open nothing. Removing a path the index does not hold is not
/// an error, which is what lets the watcher replay a delete. A removed row
/// moves the names version in the same transaction.
pub(crate) fn forget_file<N: NameSource>(
    conn: &Connection,
    names: &N,
    key: &str,
) -> StorageResult<()> {
    let tx = begin_write(conn, names)?;
    tx.execute(
        "DELETE FROM files_fts
         WHERE rowid = (SELECT rowid FROM files WHERE path = ?1)",
        params![key],
    )?;
    let deleted = tx.execute("DELETE FROM files WHERE path = ?1", params![key])?;
    tx.execute(
        "UPDATE links SET to_path = NULL WHERE to_path = ?1",
        params![key],
    )?;
    let removed = names.remove(key);
    let version = (|| -> StorageResult<Option<i64>> {
        names.read(|index| {
            resolve::resolve_links_named(&tx, index, &links::candidate_name_keys(key))
        })?;
        Ok(if deleted > 0 {
            Some(schema::bump_names_version(&tx)?)
        } else {
            None
        })
    })();
    if let Err(error) = version.and_then(|version| commit_names(tx, names, version)) {
        if removed {
            names.insert(key);
        }
        names.abandon();
        return Err(error);
    }
    Ok(())
}

/// Rewrites `links`, `properties`, `tags` and `headings` for `path` from
/// `content`.
///
/// Runs inside the caller's transaction on the caller's connection. The four
/// tables cascade on a deleted `files` row, which covers a removal and nothing
/// else, so an update has to clear them itself or a note's old links outlive
/// the text that held them.
///
/// Facts are read out of Markdown files only. A `.txt` file is plain text: a
/// `[[x]]` in it is two brackets, `#include` is not a tag, and its rows are
/// cleared and none written (ADR-041 §2).
///
/// A link that resolves to exactly one note stores that note's path, and one
/// that resolves to nothing or to several notes stores `NULL`
/// ([`resolve::stored_path`]). Every link stores the folded name it names
/// ([`link_name_key`]), which is how the note it names finds it later.
fn write_facts(
    conn: &Connection,
    path: &str,
    content: &str,
    names: &NameIndex,
) -> StorageResult<()> {
    for sql in [
        "DELETE FROM links WHERE from_path = ?1",
        "DELETE FROM properties WHERE path = ?1",
        "DELETE FROM tags WHERE path = ?1",
        "DELETE FROM headings WHERE path = ?1",
    ] {
        conn.prepare_cached(sql)?.execute(params![path])?;
    }
    if !is_note_file(path) {
        return Ok(());
    }

    let facts = facts::extract(content);
    for link in &facts.links {
        let to_path = resolve::stored_path(names.resolve(&link.wikilink_target(), path));
        conn.prepare_cached(
            "INSERT INTO links (from_path, to_target, to_path, kind, line, col, name_key)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?
        .execute(params![
            path,
            link.target,
            to_path,
            link.kind.as_str(),
            link.line,
            link.col,
            link_name_key(&link.target)
        ])?;
    }
    for (key, value) in &facts.properties {
        conn.prepare_cached("INSERT INTO properties (path, key, value_json) VALUES (?1, ?2, ?3)")?
            .execute(params![path, key, value.to_string()])?;
    }
    for (tag, line) in &facts.tags {
        conn.prepare_cached("INSERT INTO tags (path, tag, line) VALUES (?1, ?2, ?3)")?
            .execute(params![path, tag, line])?;
    }
    for heading in &facts.headings {
        conn.prepare_cached(
            "INSERT INTO headings (path, level, text, line, slug)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?
        .execute(params![
            path,
            heading.level,
            heading.text,
            heading.line,
            heading.slug
        ])?;
    }
    Ok(())
}

/// Re-keys every row under `from` to the same file under `to`. See
/// [`NotesIndex::rekey_root`].
pub(crate) fn rekey(conn: &Connection, from: &Path, to: &Path) -> StorageResult<usize> {
    let old = root_prefix(from);
    let new = root_prefix(to);
    if old == new {
        return Ok(0);
    }
    let width = old.chars().count() as i64;
    let rest = width + 1;

    let tx = conn.unchecked_transaction()?;
    // Scoped to this transaction and reset at its commit: the children point
    // at a parent key that does not exist yet for the length of the rewrite,
    // which an immediate constraint would refuse.
    tx.pragma_update(None, "defer_foreign_keys", "on")?;

    let changed = tx.execute(
        "UPDATE files SET path = ?1 || substr(path, ?2) WHERE substr(path, 1, ?3) = ?4",
        params![new, rest, width, old],
    )?;
    for (table, column) in [
        ("links", "from_path"),
        ("links", "to_path"),
        ("properties", "path"),
        ("tags", "path"),
        ("headings", "path"),
    ] {
        tx.execute(
            &format!(
                "UPDATE {table} SET {column} = ?1 || substr({column}, ?2) \
                 WHERE substr({column}, 1, ?3) = ?4"
            ),
            params![new, rest, width, old],
        )?;
    }
    if changed > 0 {
        schema::bump_names_version(&tx)?;
    }
    tx.commit()?;
    Ok(changed)
}

impl NotesIndex<'_> {
    /// Records `note` and its text, replacing whatever the index held for that
    /// path. See [`write_file`].
    ///
    /// The folder's names are read from the connection for this one call; the
    /// store keeps them in memory instead
    /// ([`super::NotesIndexStore::upsert`]).
    pub fn upsert(&self, note: &IndexedNote, content: &str) -> StorageResult<()> {
        let names = RefCell::new(NameIndex::load(self.conn)?);
        write_file(self.conn, &names, note, content, Expect::Anything).map(|_| ())
    }

    /// Removes `path` from the index. See [`forget_file`].
    pub fn remove(&self, path: &str) -> StorageResult<()> {
        let names = RefCell::new(NameIndex::load(self.conn)?);
        forget_file(self.conn, &names, path)
    }

    /// Re-keys every row under `from` to the same file under `to`.
    ///
    /// The move already put the files there, so the rows describe the right
    /// bytes at the wrong key. Rewriting the key rather than dropping the rows
    /// is what keeps the following walk from reading anything: `size` and
    /// `mtime` still match the file, so `reconcile` reports no work. Dropping
    /// them would make every note in the folder look new, and in a sync folder
    /// a walk that reads every note pulls down every note (ADR-028 section 7).
    ///
    /// `files.path` is the parent of `links`, `properties`, `tags` and
    /// `headings`, which cascade on delete but have no `ON UPDATE`, so the
    /// children are rewritten in the same transaction with the constraint
    /// deferred to its commit. `files_fts` joins on rowid, which an `UPDATE`
    /// leaves alone, so the text entries need no work at all. A link's
    /// `name_key` is a name, not a path, so a move leaves it as it is.
    ///
    /// Both folders are spelled through [`super::index_key`], so a caller may
    /// pass either folder's everyday path: `from` no longer exists by the time
    /// this runs, and its key comes from its parent exactly as a deleted
    /// file's does. Prefix comparison is against the folder plus a separator,
    /// so a move out of `~/Writ` never claims a row in `~/Writing`. Returns the
    /// rows re-keyed.
    pub fn rekey_root(&self, from: &Path, to: &Path) -> StorageResult<usize> {
        rekey(self.conn, from, to)
    }
}
