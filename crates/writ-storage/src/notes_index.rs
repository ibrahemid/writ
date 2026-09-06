//! The path-keyed index over the notes folder (ADR-028 section 7).
//!
//! Files are the only copy of the text, so the index is keyed by the canonical
//! path of the file rather than by a database row. [`NotesIndex`] is the write
//! and query surface over `files` and `files_fts`; [`reconcile`] walks the
//! notes folder and brings the index back in line with what is on disk, which
//! is what makes deleting `writ.db` safe and what picks up notes written by
//! another editor while Writ was closed.
//!
//! # One key policy
//!
//! Three writers put rows into `files`: [`reconcile`], the notes watcher's
//! subscriber, and the deferred reindex behind a save. They must agree on the
//! spelling of a path byte for byte or the same note lands in the index twice
//! and a search returns a row nothing can open. [`index_key`] is that policy,
//! and every writer goes through it.
//!
//! # Files that are not downloaded
//!
//! A sync provider can leave a placeholder with no local data behind it.
//! Reading one asks the provider daemon to materialise it, which stalls the
//! walk and can pull gigabytes down a metered connection, so [`is_dataless`]
//! answers from the file's metadata and the walk never opens those files. They
//! are indexed by name alone: the note is findable and openable, and its text
//! joins the index the first time something else downloads it.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use rusqlite::{params, Connection};
use writ_core::file_ops::{classify_path, FileOpenMode, THRESHOLD_NORMAL_BYTES};
use writ_core::notes::links::{self, Resolution, WikilinkTarget};
use writ_core::notes::snippet;
use writ_core::notes::{self, facts};
use writ_core::search::{build_hit, SearchHit};
use writ_core::workspace::file_search::{rank_keyed_file_hits, FileHit};

use crate::errors::StorageResult;
use crate::schema_meta;
use crate::workspace_search::build_walk;

/// Extensions indexed without sniffing the file, so the common case never
/// opens a file to decide whether to open it.
const TEXT_EXTENSIONS: &[&str] = &["md", "markdown", "txt", "text"];

/// macOS `SF_DATALESS`: the file has no local data behind it.
const SF_DATALESS: u32 = 0x4000_0000;

/// How much of a file the index holds, stored in `files.indexed_by`
/// (migration 042).
///
/// A placeholder with no local data is recorded without being read, so its row
/// carries its name and nothing else. [`reconcile`] reads this back to find
/// those rows once the file has data: a download leaves size and mtime where
/// they were, so no other column can tell it happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IndexedBy {
    /// The row was written from the file's text.
    #[default]
    Content,
    /// The row was written from the file's name, with no read.
    Name,
}

impl IndexedBy {
    /// The value stored in `files.indexed_by`, which is also the wire spelling
    /// a caller matches on.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Content => "content",
            Self::Name => "name",
        }
    }

    /// Reads the value back. Anything the column does not name is read as
    /// [`IndexedBy::Content`], the value migration 042 backfilled every
    /// existing row with.
    pub fn from_stored(value: &str) -> Self {
        match value {
            "name" => Self::Name,
            _ => Self::Content,
        }
    }
}

/// One indexed note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedNote {
    /// Canonical path, as produced by [`index_key`].
    pub path: String,
    /// File name, the label a result carries.
    pub name: String,
    /// Size in bytes at index time.
    pub size: u64,
    /// Modification time in milliseconds since the Unix epoch at index time.
    pub mtime: i64,
    /// Content hash, when the caller has one to record.
    pub hash: Option<String>,
    /// Whether the row holds the file's text or only its name.
    pub indexed_by: IndexedBy,
}

impl IndexedNote {
    /// Describes the file at `path` as the index holds it, or `None` when its
    /// metadata cannot be read or it has no file name.
    ///
    /// The one place an [`IndexedNote`] is built from a path, so the reconcile
    /// walk and the save path key and label a file identically.
    pub fn from_file(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        Some(Self {
            path: index_key(path),
            name: path.file_name()?.to_string_lossy().into_owned(),
            size: metadata.len(),
            mtime: mtime_millis(&metadata),
            hash: None,
            indexed_by: IndexedBy::Content,
        })
    }
}

/// One row of the `links` table, mirroring migration 040's columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkRow {
    /// Canonical path of the note the link is written in.
    pub from_path: String,
    /// The link's target as it was written: no alias, no heading.
    pub to_target: String,
    /// The note the target resolved to, or `None` when it resolved to nothing
    /// or to more than one note.
    pub to_path: Option<String>,
    /// `wikilink` or `markdown` ([`writ_core::notes::links::LinkKind`]).
    pub kind: String,
    /// 1-based line the link is on.
    pub line: u32,
    /// 0-based character offset of the link inside that line.
    pub col: u32,
}

/// One row of the `headings` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadingRow {
    /// `1` for `#` through `6` for `######`.
    pub level: u8,
    /// The heading text.
    pub text: String,
    /// 1-based line the heading is on.
    pub line: u32,
    /// The anchor `[[Note#Heading]]` matches.
    pub slug: String,
}

/// Whether a backlink means the note it is listed under for certain.
///
/// A link whose name matches two notes belongs in the list of both, flagged:
/// the alternative is a list that quietly under-reports, and a rename that
/// then breaks a link nobody was shown (ADR-034).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BacklinkCertainty {
    /// The link resolved to this note and to no other.
    Resolved,
    /// The link names this note and at least one other, and picks neither.
    Ambiguous,
}

impl BacklinkCertainty {
    /// The wire spelling, which is also what a caller matches on.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Resolved => "resolved",
            Self::Ambiguous => "ambiguous",
        }
    }
}

/// One link written in another note that points at this one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacklinkRow {
    /// Canonical path of the note the link is written in.
    pub from_path: String,
    /// What that note is called, taken from `from_path` by
    /// [`writ_core::notes::note_display_name`]: the file name without a note
    /// extension, which is both what a link names it by and what a list shows.
    pub from_name: String,
    /// The link's target as it was written: no alias, no heading.
    pub to_target: String,
    /// A wikilink's `|alias`. `None` for a markdown link: the parser keeps the
    /// destination, not the `[label]`, which [`BacklinkRow::context`] quotes.
    pub alias: Option<String>,
    /// `wikilink` or `markdown` ([`writ_core::notes::links::LinkKind`]).
    pub kind: String,
    /// 1-based line the link is on.
    pub line: u32,
    /// 0-based character offset of the link inside that line.
    pub col: u32,
    /// The sentence the link sits in, cut from the text the index holds. Empty
    /// when the index holds no text for the linking note.
    pub context: String,
    /// Whether the link means this note for certain.
    pub certainty: BacklinkCertainty,
    /// The other notes the link might mean, by path, when it means more than
    /// one. Empty for a link that means this note and no other, so a reader is
    /// told which notes an ambiguity is between rather than that there is one.
    pub candidates: Vec<String>,
}

/// Everything the index holds about one note beyond its `files` row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoteFactsRow {
    /// Links written in the note.
    pub links: Vec<LinkRow>,
    /// Frontmatter properties, each value as the JSON it is stored as.
    pub properties: Vec<(String, String)>,
    /// Each `#tag` and the line it is on.
    pub tags: Vec<(String, u32)>,
    /// Headings, in document order.
    pub headings: Vec<HeadingRow>,
}

/// One note in the folder's link graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphNode {
    /// Canonical path of the note, which is what every edge names it by.
    pub path: String,
    /// What the note is called, from [`writ_core::notes::note_display_name`].
    pub name: String,
    /// The first path segment under the notes root. Empty for a note sitting
    /// in the root itself, and empty for a path the root does not contain.
    pub folder: String,
}

/// A link between two notes, and how many times it is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphEdge {
    /// Canonical path of the note the links are written in.
    pub from_path: String,
    /// Canonical path of the note they reached.
    pub to_path: String,
    /// How many links in `from_path` resolved to `to_path`.
    pub count: usize,
}

/// Every note in the folder and every resolved link among them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GraphRows {
    /// One entry per indexed note, in path order.
    pub nodes: Vec<GraphNode>,
    /// One entry per linked pair, in `(from_path, to_path)` order.
    pub edges: Vec<GraphEdge>,
}

/// Every indexed path, grouped by the names a link can call it by.
///
/// Resolution compares a target against every note that shares its name, so
/// reading `files` once per note and grouping it here is what keeps a walk over
/// a large folder from re-reading the table for every link it finds. The group
/// keys come from [`writ_core::notes::links::candidate_name_keys`], the same
/// function the resolver matches with, so the prefilter can never hide a
/// candidate the resolver would have accepted.
#[derive(Debug, Default)]
struct NameIndex {
    by_key: HashMap<String, Vec<String>>,
}

impl NameIndex {
    /// Records `path` under every name it answers to.
    fn insert(&mut self, path: &str) {
        for key in links::candidate_name_keys(path) {
            let group = self.by_key.entry(key).or_default();
            if !group.iter().any(|held| held == path) {
                group.push(path.to_string());
            }
        }
    }

    /// The indexed paths a link naming `name` could mean.
    fn candidates(&self, name: &str) -> &[String] {
        const NONE: &[String] = &[];
        self.by_key
            .get(&links::name_key(name))
            .map_or(NONE, Vec::as_slice)
    }

    /// Resolves `target` as seen from the note at `from`.
    fn resolve(&self, target: &WikilinkTarget, from: &str) -> Resolution {
        links::resolve(target, from, self.candidates(&target.name))
    }
}

/// What one [`reconcile`] pass did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReconcileOutcome {
    /// Files indexed for the first time.
    pub added: usize,
    /// Files re-read because their size or mtime had changed.
    pub updated: usize,
    /// Rows removed because the file behind them is gone.
    pub removed: usize,
    /// Files the filesystem reports as not downloaded, which are indexed by
    /// name with no text rather than read.
    pub skipped_dataless: usize,
    /// `true` when the walk stopped early because the caller cancelled it. A
    /// cancelled pass never removes rows: it has not seen the whole tree, so
    /// every unseen path is unproven rather than vanished.
    pub cancelled: bool,
}

/// The index's spelling of `path`.
///
/// Canonical, so `/var` and `/private/var` are one key on macOS, a symlinked
/// notes folder agrees with the walk that resolved it, and the filesystem's own
/// unicode normalisation is on both sides of every comparison. Case is
/// preserved: the key is also the path a result is opened by.
///
/// A path whose file is already gone cannot be canonicalised, so the parent is
/// canonicalised instead and the file name rejoined. That is the delete arm of
/// the watcher, and it must produce the same key the file had while it existed.
/// When nothing resolves, the path is returned as it came in, which keys the
/// row consistently even if it keys it by a spelling no walk will match.
///
/// The answer carries no Windows verbatim prefix
/// ([`crate::paths::strip_verbatim_prefix`]), which is what lets a key stand
/// beside the paths [`crate::workspace_store::list_dir`] hands the file tree.
pub fn index_key(path: &Path) -> String {
    let resolved = std::fs::canonicalize(path)
        .ok()
        .or_else(|| {
            let parent = path.parent()?;
            let name = path.file_name()?;
            Some(std::fs::canonicalize(parent).ok()?.join(name))
        })
        .unwrap_or_else(|| path.to_path_buf());

    crate::paths::strip_verbatim_prefix(resolved)
        .to_string_lossy()
        .into_owned()
}

/// `true` when the filesystem reports the file has no local data: an iCloud (or
/// other provider) placeholder that reading would materialise.
///
/// macOS reads `SF_DATALESS` out of the stat flags, which is metadata only and
/// never opens the file. Every other platform answers `false`: no other
/// filesystem Writ supports reports the state, and guessing would skip real
/// files.
pub fn is_dataless(path: &Path) -> bool {
    stat_flags(path).is_some_and(dataless_from_flags)
}

/// The flag test, split out from the platform-specific stat so it is exercised
/// by the test suite on every platform rather than only on the one that can
/// produce the flag.
fn dataless_from_flags(flags: u32) -> bool {
    flags & SF_DATALESS != 0
}

#[cfg(target_os = "macos")]
fn stat_flags(path: &Path) -> Option<u32> {
    use std::os::macos::fs::MetadataExt;
    // symlink_metadata, so a link to a placeholder is judged by the link.
    std::fs::symlink_metadata(path).ok().map(|m| m.st_flags())
}

#[cfg(not(target_os = "macos"))]
fn stat_flags(_path: &Path) -> Option<u32> {
    None
}

/// Read and write access to the notes index over a borrowed connection.
pub struct NotesIndex<'a> {
    conn: &'a Connection,
}

impl<'a> NotesIndex<'a> {
    /// Constructs an index view over the given connection.
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Records `note` and its text, replacing whatever the index held for that
    /// path.
    ///
    /// The `files` row is updated in place rather than replaced. `files_fts`
    /// joins `files` on rowid and `links`, `properties`, `tags` and `headings`
    /// cascade from `files(path)`, so an `INSERT OR REPLACE` would reassign the
    /// rowid, orphan the text entry and delete every derived row a save is not
    /// entitled to touch (migration 040 records the same constraint).
    ///
    /// The four derived tables are rewritten in the same transaction, so a note
    /// and the facts read out of it are never a save apart. A note whose file
    /// was never read — a placeholder indexed by name — has no text to read
    /// facts from and gets none, which is what its empty `content` says.
    /// Finally, links elsewhere that named this note before it existed are
    /// resolved (`to_path` backfill, ADR-034).
    pub fn upsert(&self, note: &IndexedNote, content: &str) -> StorageResult<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO files (path, size, mtime, hash, indexed_by, indexed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))
             ON CONFLICT(path) DO UPDATE SET
                 size = excluded.size,
                 mtime = excluded.mtime,
                 hash = excluded.hash,
                 indexed_by = excluded.indexed_by,
                 indexed_at = excluded.indexed_at",
            params![
                note.path,
                note.size as i64,
                note.mtime,
                note.hash,
                note.indexed_by.as_str()
            ],
        )?;
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
        let names = self.name_index()?;
        self.write_facts(&note.path, content, &names)?;
        self.resolve_links(&names, Some(&links::candidate_name_keys(&note.path)))?;
        tx.commit()?;
        Ok(())
    }

    /// A walk's write, which loses every race it is in.
    ///
    /// A walk reads a file and writes the row some milliseconds later. A save
    /// landing in that window would be overwritten by what the walk read
    /// before it, and nothing would correct the row until the file changed
    /// again. So the write is conditional on the row still holding `expected`
    /// — the state the walk decided from, or `None` for a row it did not see —
    /// and a walk that finds anything else leaves the newer entry alone.
    /// `false` says the write was declined.
    ///
    /// The four derived tables are written under the same condition. Writing
    /// them for a declined row would attach the text the walk read minutes ago
    /// to the row a save just wrote, and nothing would correct it until the
    /// file changed again.
    fn upsert_walked(
        &self,
        note: &IndexedNote,
        content: &str,
        expected: Option<(u64, i64)>,
        names: &NameIndex,
    ) -> StorageResult<bool> {
        let tx = self.conn.unchecked_transaction()?;
        let wrote = match expected {
            None => tx.execute(
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
            )?,
            Some((size, mtime)) => tx.execute(
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
            )?,
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
        self.write_facts(&note.path, content, names)?;
        tx.commit()?;
        Ok(true)
    }

    /// Rewrites `links`, `properties`, `tags` and `headings` for `path` from
    /// `content`.
    ///
    /// Runs inside the caller's transaction on the caller's connection. The
    /// four tables cascade on a deleted `files` row, which covers
    /// [`NotesIndex::remove`] and nothing else, so an update has to clear them
    /// itself or a note's old links outlive the text that held them.
    ///
    /// A link that resolves to exactly one note stores that note's path. One
    /// that resolves to nothing, or to several notes, stores `NULL`: an
    /// ambiguous target is not a link to the alphabetically first candidate
    /// (ADR-034), and a target with no note behind it yet is picked up by
    /// [`NotesIndex::resolve_links`] when that note arrives.
    fn write_facts(&self, path: &str, content: &str, names: &NameIndex) -> StorageResult<()> {
        for sql in [
            "DELETE FROM links WHERE from_path = ?1",
            "DELETE FROM properties WHERE path = ?1",
            "DELETE FROM tags WHERE path = ?1",
            "DELETE FROM headings WHERE path = ?1",
        ] {
            self.conn.prepare_cached(sql)?.execute(params![path])?;
        }

        let facts = facts::extract(content);
        for link in &facts.links {
            let to_path = match names.resolve(&link.wikilink_target(), path) {
                Resolution::Resolved(target) => Some(target),
                Resolution::Ambiguous(_) | Resolution::Missing => None,
            };
            self.conn
                .prepare_cached(
                    "INSERT INTO links (from_path, to_target, to_path, kind, line, col)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                )?
                .execute(params![
                    path,
                    link.target,
                    to_path,
                    link.kind.as_str(),
                    link.line,
                    link.col
                ])?;
        }
        for (key, value) in &facts.properties {
            self.conn
                .prepare_cached(
                    "INSERT INTO properties (path, key, value_json) VALUES (?1, ?2, ?3)",
                )?
                .execute(params![path, key, value.to_string()])?;
        }
        for (tag, line) in &facts.tags {
            self.conn
                .prepare_cached("INSERT INTO tags (path, tag, line) VALUES (?1, ?2, ?3)")?
                .execute(params![path, tag, line])?;
        }
        for heading in &facts.headings {
            self.conn
                .prepare_cached(
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

    /// Every indexed path, in the spelling resolution compares against.
    fn paths(&self) -> StorageResult<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT path FROM files")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Every indexed path, grouped by the names a link can call it by.
    fn name_index(&self) -> StorageResult<NameIndex> {
        let mut stmt = self.conn.prepare("SELECT path FROM files")?;
        let mut index = NameIndex::default();
        for path in stmt.query_map([], |row| row.get::<_, String>(0))? {
            index.insert(&path?);
        }
        Ok(index)
    }

    /// Recomputes `to_path` for the links a change to the set of indexed paths
    /// can have moved. Returns the number of links whose target changed.
    ///
    /// Every visited row is resolved from scratch and written back, including
    /// back to `NULL`. A link is stored with no target both when the note it
    /// names does not exist yet and when two notes answer to that name, and
    /// either can become the other: a note arriving fills in the links that
    /// waited for it, a second note of the same name makes an answered link
    /// ambiguous again, and a note leaving empties the links that reached it.
    /// A pass that only looked at `to_path IS NULL` would do the first and
    /// miss the other two, and the stale target would then survive every walk,
    /// because a walk re-reads a note only when its bytes moved.
    ///
    /// `to_target` was stored from a target the scanner had already parsed, so
    /// it is read back through [`links::stored_target`] and never parsed a
    /// second time. This pass runs after [`NotesIndex::write_facts`] over every
    /// row, so a second parse here does not merely answer differently, it
    /// overwrites what the writer resolved: `[[Note.md.md]]` lost another
    /// extension and the index named `Note` where the editor named `Note.md`.
    ///
    /// `only` is the set of folded names whose links are worth revisiting: a
    /// single note arriving or leaving can only change the links that named
    /// *it*, so a save re-resolves that note's own name keys and leaves the
    /// rest of a vault alone. `None` revisits every link, which is what a walk
    /// wants and what also drops the targets of files that vanished while Writ
    /// was not running.
    fn resolve_links(&self, names: &NameIndex, only: Option<&[String]>) -> StorageResult<usize> {
        let stored: Vec<(i64, String, String, Option<String>)> = {
            let mut stmt = self
                .conn
                .prepare("SELECT rowid, from_path, to_target, to_path FROM links")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let mut changed = 0usize;
        for (rowid, from_path, to_target, was) in stored {
            let target = links::stored_target(&to_target);
            if let Some(keys) = only {
                if !keys.contains(&links::name_key(&target.name)) {
                    continue;
                }
            }
            let now = match names.resolve(&target, &from_path) {
                Resolution::Resolved(path) => Some(path),
                Resolution::Ambiguous(_) | Resolution::Missing => None,
            };
            if now == was {
                continue;
            }
            self.conn
                .prepare_cached("UPDATE links SET to_path = ?2 WHERE rowid = ?1")?
                .execute(params![rowid, now])?;
            changed += 1;
        }
        Ok(changed)
    }

    /// Removes `path` from the index.
    ///
    /// The text entry goes first, while the `files` row it joins on still
    /// exists; the row itself follows, and `links`, `properties`, `tags` and
    /// `headings` cascade with it. Losing the row without losing the text entry
    /// is what produces a hit nothing can open, so both steps propagate errors.
    ///
    /// The cascade only reaches the links written *in* the removed note. The
    /// links written in other notes that resolved *to* it are a plain column
    /// with no foreign key behind it, so they are emptied here: a link left
    /// pointing at a deleted file reads as resolved and opens nothing.
    pub fn remove(&self, path: &str) -> StorageResult<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM files_fts
             WHERE rowid = (SELECT rowid FROM files WHERE path = ?1)",
            params![path],
        )?;
        tx.execute("DELETE FROM files WHERE path = ?1", params![path])?;
        tx.execute(
            "UPDATE links SET to_path = NULL WHERE to_path = ?1",
            params![path],
        )?;
        tx.commit()?;
        Ok(())
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
    /// leaves alone, so the text entries need no work at all.
    ///
    /// Both folders are spelled through [`index_key`], so a caller may pass
    /// either folder's everyday path: `from` no longer exists by the time this
    /// runs, and its key comes from its parent exactly as a deleted file's
    /// does. Prefix comparison is against the folder plus a separator, so a
    /// move out of `~/Writ` never claims a row in `~/Writing`. Returns the
    /// rows re-keyed.
    pub fn rekey_root(&self, from: &Path, to: &Path) -> StorageResult<usize> {
        let old = root_prefix(from);
        let new = root_prefix(to);
        if old == new {
            return Ok(0);
        }
        let width = old.chars().count() as i64;
        let rest = width + 1;

        let tx = self.conn.unchecked_transaction()?;
        // Scoped to this transaction and reset at its commit: the children
        // point at a parent key that does not exist yet for the length of the
        // rewrite, which an immediate constraint would refuse.
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
        tx.commit()?;
        Ok(changed)
    }

    /// Up to `limit` ranked content hits, each carrying the note's path, name,
    /// matching line and a highlighted snippet.
    pub fn search_hits(
        &self,
        query: &str,
        terms: &[String],
        limit: usize,
    ) -> StorageResult<Vec<SearchHit>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.path, x.name, x.content FROM files_fts x
             JOIN files f ON f.rowid = x.rowid
             WHERE files_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![query, limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;

        let mut hits = Vec::new();
        for row in rows {
            let (path, name, content) = row?;
            // A note has no buffer id until something opens it. The adapter
            // fills one in for a note that is already a tab; the path is what
            // opens the rest.
            let mut hit = build_hit("", &name, &content, terms);
            hit.path = Some(path);
            hits.push(hit);
        }
        Ok(hits)
    }

    /// Total number of notes matching `query`, independent of any result limit,
    /// so the UI can report "N of M" honestly.
    pub fn count(&self, query: &str) -> StorageResult<usize> {
        let total: i64 = self.conn.query_row(
            "SELECT count(*) FROM files_fts WHERE files_fts MATCH ?1",
            params![query],
            |row| row.get(0),
        )?;
        Ok(total as usize)
    }

    /// Up to `limit` ranked name-only hits, for quick open.
    ///
    /// Ranked by the same subsequence scorer the workspace file palette uses,
    /// so a prefix of a note's name outranks a match in the middle of one and
    /// the two surfaces order their results the same way.
    ///
    /// A note is findable by its name and by where it sits inside
    /// `notes_root`, never by the folders above it: rows are keyed by absolute
    /// path, so ranking that key would let a query reach every note through
    /// the letters of the home directory. What is ranked is the path under the
    /// root, spelled with forward slashes so the order two notes come back in
    /// is the same on every platform, and each hit still carries the index key
    /// the caller opens. A row outside the root (a folder that moved while the
    /// walk was retired) is ranked on its name alone.
    pub fn search_names(
        &self,
        query: &str,
        notes_root: &Path,
        limit: usize,
    ) -> StorageResult<Vec<FileHit>> {
        let prefix = root_prefix(notes_root);
        let mut stmt = self
            .conn
            .prepare("SELECT f.path, x.name FROM files f JOIN files_fts x ON x.rowid = f.rowid")?;
        let candidates = stmt
            .query_map([], |row| {
                let path = row.get::<_, String>(0)?;
                let name = row.get::<_, String>(1)?;
                let relative = match path.strip_prefix(&prefix) {
                    Some(rest) => rest.replace('\\', "/"),
                    None => name.clone(),
                };
                Ok((path, relative, name))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(rank_keyed_file_hits(
            query,
            candidates
                .iter()
                .map(|(path, relative, name)| (path.as_str(), relative.as_str(), name.as_str())),
            limit,
        ))
    }

    /// Every indexed path with the size and mtime it was indexed at, for
    /// reconciliation.
    pub fn snapshot(&self) -> StorageResult<Vec<(String, u64, i64)>> {
        let mut stmt = self.conn.prepare("SELECT path, size, mtime FROM files")?;
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

    /// Every link written in the note at `path`.
    pub fn links_from(&self, path: &str) -> StorageResult<Vec<LinkRow>> {
        self.link_rows(
            "SELECT from_path, to_target, to_path, kind, line, col FROM links
              WHERE from_path = ?1 ORDER BY line, col",
            path,
        )
    }

    /// Every link that resolved to the note at `path`.
    ///
    /// An unresolved link and an ambiguous one are absent by construction:
    /// their `to_path` is `NULL`, which is the honest record of a link that
    /// points at no one note. [`NotesIndex::backlinks`] adds the ambiguous ones
    /// back, flagged, because a link that might mean this note is a fact about
    /// this note.
    pub fn links_to(&self, path: &str) -> StorageResult<Vec<LinkRow>> {
        self.link_rows(
            "SELECT from_path, to_target, to_path, kind, line, col FROM links
              WHERE to_path = ?1 ORDER BY from_path, line, col",
            path,
        )
    }

    /// Runs a one-parameter query over `links`.
    fn link_rows(&self, sql: &str, path: &str) -> StorageResult<Vec<LinkRow>> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt
            .query_map(params![path], |row| {
                Ok(LinkRow {
                    from_path: row.get(0)?,
                    to_target: row.get(1)?,
                    to_path: row.get(2)?,
                    kind: row.get(3)?,
                    line: row.get::<_, i64>(4)? as u32,
                    col: row.get::<_, i64>(5)? as u32,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// The notes that link to the note at `path`, each link with the sentence
    /// it sits in (spec L2).
    ///
    /// Three kinds of link name a note and only two of them belong here. A
    /// link that resolved to `path` is a backlink. A link that names `path` and
    /// one other note picks neither, so it is a backlink of both, marked
    /// [`BacklinkCertainty::Ambiguous`]. A link that resolved to a *different*
    /// note of the same name, and a link that resolved to nothing at all, are
    /// backlinks of no note: the first belongs to the note it reached and the
    /// second to none. Neither is dropped without being counted somewhere.
    ///
    /// The snippet is cut from the text `files_fts` holds, so no note is read
    /// off disk to answer this and a placeholder is not materialised by a list
    /// being opened. A note indexed by name alone therefore links to nothing:
    /// indexing it drops the facts derived from its text, the links among them,
    /// and it leaves every list until something downloads it.
    pub fn backlinks(&self, path: &str) -> StorageResult<Vec<BacklinkRow>> {
        let mut found: Vec<(LinkRow, BacklinkCertainty, Vec<String>)> = self
            .links_to(path)?
            .into_iter()
            .map(|row| (row, BacklinkCertainty::Resolved, Vec::new()))
            .collect();

        // One name index for the whole call: `resolve_link` builds a fresh one
        // per target, which over a folder's worth of unresolved links is the
        // table read once per link.
        let names = self.name_index()?;
        let keys = links::candidate_name_keys(path);
        // Stored targets are read the way [`NotesIndex::resolve_links`] reads
        // them, so an ambiguous link is listed under the same notes the editor
        // offers to open.
        for row in self.unresolved_links()? {
            let target = links::stored_target(&row.to_target);
            if !keys.contains(&links::name_key(&target.name)) {
                continue;
            }
            if let Resolution::Ambiguous(candidates) = names.resolve(&target, &row.from_path) {
                if candidates.iter().any(|candidate| candidate == path) {
                    // The list the row is shown under is the one note it is
                    // already filed against, so what it carries is the others.
                    let others = candidates
                        .into_iter()
                        .filter(|candidate| candidate != path)
                        .collect();
                    found.push((row, BacklinkCertainty::Ambiguous, others));
                }
            }
        }

        found.sort_by(|(left, ..), (right, ..)| {
            (&left.from_path, left.line, left.col).cmp(&(&right.from_path, right.line, right.col))
        });
        self.describe_backlinks(found)
    }

    /// Attaches the linking note's name, the link's alias and its sentence to
    /// each row, reading each linking note's text once however many links it
    /// holds.
    fn describe_backlinks(
        &self,
        found: Vec<(LinkRow, BacklinkCertainty, Vec<String>)>,
    ) -> StorageResult<Vec<BacklinkRow>> {
        let mut described = Vec::with_capacity(found.len());
        let mut start = 0usize;
        while start < found.len() {
            let from_path = found[start].0.from_path.clone();
            let end = found[start..]
                .iter()
                .position(|(row, ..)| row.from_path != from_path)
                .map_or(found.len(), |offset| start + offset);

            let content = self.note_text(&from_path)?;
            // Taken from the path, not from the indexed name: the path is the
            // note's identity and is spelled the same on every row.
            let from_name = notes::note_display_name(&from_path);
            // The same parser that wrote the rows, so the alias and the row can
            // never disagree about which link is which.
            let scanned = links::scan(&content);
            for (row, certainty, candidates) in &found[start..end] {
                let written = scanned
                    .iter()
                    .find(|link| link.line == row.line && link.col == row.col);
                described.push(BacklinkRow {
                    from_path: from_path.clone(),
                    from_name: from_name.clone(),
                    to_target: row.to_target.clone(),
                    alias: written.and_then(|link| link.alias.clone()),
                    kind: row.kind.clone(),
                    line: row.line,
                    col: row.col,
                    context: written
                        .map(|link| snippet::sentence_at(&content, link.byte_range.start))
                        .unwrap_or_default(),
                    certainty: *certainty,
                    candidates: candidates.clone(),
                });
            }
            start = end;
        }
        Ok(described)
    }

    /// Every link the index could not point at one note.
    fn unresolved_links(&self) -> StorageResult<Vec<LinkRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT from_path, to_target, to_path, kind, line, col FROM links
              WHERE to_path IS NULL ORDER BY from_path, line, col",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(LinkRow {
                    from_path: row.get(0)?,
                    to_target: row.get(1)?,
                    to_path: row.get(2)?,
                    kind: row.get(3)?,
                    line: row.get::<_, i64>(4)? as u32,
                    col: row.get::<_, i64>(5)? as u32,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// The indexed text of the note at `path`: empty when the index holds the
    /// note by name alone, and empty when it does not hold it at all.
    fn note_text(&self, path: &str) -> StorageResult<String> {
        let mut stmt = self.conn.prepare(
            "SELECT x.content FROM files_fts x
             JOIN files f ON f.rowid = x.rowid
             WHERE f.path = ?1",
        )?;
        let mut rows = stmt.query_map(params![path], |row| row.get::<_, String>(0))?;
        Ok(match rows.next() {
            Some(row) => row?,
            None => String::new(),
        })
    }

    /// Everything the index holds about the note at `path`.
    pub fn facts(&self, path: &str) -> StorageResult<NoteFactsRow> {
        Ok(NoteFactsRow {
            links: self.links_from(path)?,
            properties: self.property_rows(path)?,
            tags: self.tag_rows(path)?,
            headings: self.heading_rows(path)?,
        })
    }

    /// The note's frontmatter properties, each value as the JSON it is stored
    /// as, in the order they were written.
    fn property_rows(&self, path: &str) -> StorageResult<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT key, value_json FROM properties WHERE path = ?1 ORDER BY rowid")?;
        let rows = stmt
            .query_map(params![path], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// The note's tags with the line each is on.
    fn tag_rows(&self, path: &str) -> StorageResult<Vec<(String, u32)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT tag, line FROM tags WHERE path = ?1 ORDER BY line, rowid")?;
        let rows = stmt
            .query_map(params![path], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u32))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// The note's headings, in document order.
    fn heading_rows(&self, path: &str) -> StorageResult<Vec<HeadingRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT level, text, line, slug FROM headings WHERE path = ?1 ORDER BY line, rowid",
        )?;
        let rows = stmt
            .query_map(params![path], |row| {
                Ok(HeadingRow {
                    level: row.get::<_, i64>(0)? as u8,
                    text: row.get(1)?,
                    line: row.get::<_, i64>(2)? as u32,
                    slug: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// How much of the note at `path` the index holds, or `None` when it holds
    /// no row for it at all.
    ///
    /// The three answers a caller needs are `None` (not indexed),
    /// `Some(IndexedBy::Name)` (the row carries the file's name and nothing
    /// else, so its links, properties and tags are empty because nothing was
    /// read) and `Some(IndexedBy::Content)` (empty means empty). Without this
    /// the first two are indistinguishable from the third.
    pub fn indexed_by(&self, path: &str) -> StorageResult<Option<IndexedBy>> {
        let mut stmt = self
            .conn
            .prepare("SELECT indexed_by FROM files WHERE path = ?1")?;
        let mut rows = stmt.query_map(params![path], |row| row.get::<_, String>(0))?;
        Ok(match rows.next() {
            Some(value) => Some(IndexedBy::from_stored(&value?)),
            None => None,
        })
    }

    /// Every tag the index holds, with the number of notes carrying each.
    ///
    /// Tags come back as the `tags` table stores them, without the leading `#`,
    /// which is also what [`NoteFactsRow::tags`] carries. Ordered by note count
    /// descending, then by tag, so the folder's common tags come first and the
    /// order is stable between two calls over the same rows. A note tagged
    /// twice with the same tag counts once.
    pub fn all_tags(&self) -> StorageResult<Vec<(String, usize)>> {
        let mut stmt = self.conn.prepare(
            "SELECT tag, COUNT(DISTINCT path) AS notes FROM tags
             GROUP BY tag ORDER BY notes DESC, tag ASC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Every note carrying `tag`, in path order.
    ///
    /// The tag is matched whole: `project` answers with the notes carrying
    /// `#project` and not with the notes carrying `#project/alpha`, which is a
    /// tag of its own with a row of its own in [`all_tags`](Self::all_tags).
    /// A note tagging itself twice comes back once.
    ///
    /// Case is folded on the way in, because it was folded on the way into the
    /// rows: `Project` and `project` are one tag, and asking with the casing a
    /// note was written in finds it (ADR-036).
    ///
    /// One statement, joined to `files` so a tag row left behind by a file the
    /// index no longer holds cannot name a note that is gone.
    pub fn paths_for_tag(&self, tag: &str) -> StorageResult<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT tags.path FROM tags
               JOIN files ON files.path = tags.path
              WHERE tags.tag = ?1
              ORDER BY tags.path",
        )?;
        let rows = stmt
            .query_map(params![tag.to_lowercase()], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Every note in the folder and every resolved link among them.
    ///
    /// One statement per table, never a query per note: a folder of five
    /// thousand notes is one read of `files` and one grouped read of `links`,
    /// which is what keeps a graph the user opens from costing what a walk
    /// costs.
    ///
    /// What is left out is as much of the answer as what is in it.
    ///
    /// - A link with no `to_path` is a link that reached no one note: it is
    ///   unresolved, or it is ambiguous and the resolver refused to pick
    ///   (ADR-034). Drawing it would draw a guess, so it is not an edge. The
    ///   note it was written in is still a node.
    /// - A note linking to itself is dropped: it is a loop on one node and it
    ///   says nothing about the folder's shape.
    /// - Links written more than once between the same pair collapse into
    ///   `count`, so a note referenced twelve times is one edge with a weight
    ///   rather than twelve lines drawn over each other.
    /// - Only note files are nodes. The index also holds `.txt` and `.text`,
    ///   which are findable and openable but are not notes a link can name, so
    ///   an edge with either end outside the node set is dropped rather than
    ///   drawn to a node that is not there.
    pub fn graph(&self, notes_root: &Path) -> StorageResult<GraphRows> {
        let prefix = root_prefix(notes_root);
        let mut stmt = self.conn.prepare("SELECT path FROM files ORDER BY path")?;
        let nodes = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|path| is_note_file(path))
            .map(|path| GraphNode {
                name: notes::note_display_name(&path),
                folder: folder_segment(&path, &prefix),
                path,
            })
            .collect::<Vec<_>>();

        let known: HashSet<&str> = nodes.iter().map(|node| node.path.as_str()).collect();
        let mut stmt = self.conn.prepare(
            "SELECT from_path, to_path, COUNT(*) FROM links
              WHERE to_path IS NOT NULL AND to_path <> from_path
              GROUP BY from_path, to_path
              ORDER BY from_path, to_path",
        )?;
        let edges = stmt
            .query_map([], |row| {
                Ok(GraphEdge {
                    from_path: row.get(0)?,
                    to_path: row.get(1)?,
                    count: row.get::<_, i64>(2)? as usize,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|edge| {
                known.contains(edge.from_path.as_str()) && known.contains(edge.to_path.as_str())
            })
            .collect();

        Ok(GraphRows { nodes, edges })
    }

    /// Every indexed note a link naming `name` could mean, in byte order.
    ///
    /// The list [`Resolution::Ambiguous`] hands the user, and the input the
    /// resolver ranks. Nothing here picks one.
    pub fn candidate_paths(&self, name: &str) -> StorageResult<Vec<String>> {
        let mut found = self.name_index()?.candidates(name).to_vec();
        found.sort_unstable();
        Ok(found)
    }

    /// Resolves `target` — the inside of a `[[…]]`, alias and heading included
    /// — as seen from the note at `from_path`.
    pub fn resolve_link(&self, from_path: &str, target: &str) -> StorageResult<Resolution> {
        let parsed = links::parse_wikilink(target);
        Ok(self.name_index()?.resolve(&parsed, from_path))
    }

    /// The line the heading `slug` is on in the note at `path`, or `None` when
    /// the note has no such heading.
    ///
    /// A heading a link names but the note does not have leaves the link
    /// resolved and the heading unreported: the note still opens.
    pub fn heading_line(&self, path: &str, slug: &str) -> StorageResult<Option<u32>> {
        let mut stmt = self
            .conn
            .prepare("SELECT line FROM headings WHERE path = ?1 AND slug = ?2 LIMIT 1")?;
        let mut rows = stmt.query_map(params![path, slug], |row| row.get::<_, i64>(0))?;
        Ok(match rows.next() {
            Some(line) => Some(line? as u32),
            None => None,
        })
    }

    /// How many rows the four derived tables hold, and how many files they were
    /// derived from.
    ///
    /// The pair [`reconcile`] records at the end of a pass and compares against
    /// at the start of the next one, which is how it notices that the derived
    /// tables were emptied out from under it.
    fn facts_census(&self) -> StorageResult<(i64, i64)> {
        let census = self.conn.query_row(
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

    /// The paths whose row holds a name and no text ([`IndexedBy::Name`]).
    ///
    /// Materialising a placeholder leaves its size and mtime where they were,
    /// so [`reconcile`]'s unchanged shortcut cannot tell a downloaded note from
    /// the placeholder it replaced. This is how it tells, and it reads a column
    /// of its own so that a writer recording a digest cannot disturb it.
    fn name_only_paths(&self) -> StorageResult<std::collections::HashSet<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path FROM files WHERE indexed_by = ?1")?;
        let rows = stmt
            .query_map(params![IndexedBy::Name.as_str()], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<Result<std::collections::HashSet<_>, _>>()?;
        Ok(rows)
    }
}

/// Walks `notes_root` and brings the index in line with it: new files are
/// added, files whose size or mtime moved are re-read, and rows whose file is
/// gone are removed.
///
/// The walk is the shared search walk ([`build_walk`]), so the notes index, the
/// name index and the content grep agree on which files exist and which folders
/// another client left behind (`.obsidian`, `.trash`, `.stfolder`,
/// `.stversions`) are skipped.
///
/// `cancelled` is polled per entry so a shutdown does not wait for a large
/// folder. A cancelled pass reports what it did and removes nothing: it has not
/// seen the whole tree, so an unseen path is unproven rather than vanished.
///
/// `is_dataless` is injected rather than called directly so the skip is
/// testable without a real sync placeholder; production passes [`is_dataless`].
///
/// # Rebuilding the derived tables
///
/// `links`, `properties`, `tags` and `headings` are derived: emptying them and
/// running this rebuilds them (ADR-034). The size-and-mtime shortcut is what
/// would stop that — every file still matches its row, so nothing would be
/// re-read — so a pass that finds fewer derived rows than the last complete
/// pass left behind, over a file count that has not shrunk, re-reads every
/// file. The census is kept in `schema_meta`, so an index written before the
/// four tables were filled at all rebuilds on its first pass.
pub fn reconcile(
    conn: &Connection,
    notes_root: &Path,
    cancelled: &dyn Fn() -> bool,
    is_dataless: &dyn Fn(&Path) -> bool,
) -> StorageResult<ReconcileOutcome> {
    let index = NotesIndex::new(conn);
    let known: HashMap<String, (u64, i64)> = index
        .snapshot()?
        .into_iter()
        .map(|(path, size, mtime)| (path, (size, mtime)))
        .collect();
    let name_only = index.name_only_paths()?;
    let census = index.facts_census()?;
    let rebuild_facts = match read_facts_census(conn)? {
        // Nothing recorded: either the index is empty, or it was written before
        // this pass knew how to derive anything, and those rows need one read.
        None => census.1 > 0,
        // Fewer derived rows than the last complete pass left, over at least as
        // many files. Notes deleted outside Writ take their rows with them and
        // shrink both numbers, which is not this.
        Some((rows, files)) => census.0 < rows && census.1 >= files,
    };
    // Built once from what the index already holds and grown as the walk finds
    // notes it did not. A note linked before the walk reaches it resolves to
    // nothing here and is filled in by the backfill at the end.
    let mut names = NameIndex::default();
    for path in known.keys() {
        names.insert(path);
    }

    let mut outcome = ReconcileOutcome::default();
    let mut seen: Vec<String> = Vec::with_capacity(known.len());

    for entry in build_walk(notes_root).build() {
        if cancelled() {
            outcome.cancelled = true;
            break;
        }
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.path();

        // A name no listing, index or watcher event carries, on the file or on
        // a folder above it. Decided from the path alone, before anything
        // touches the file.
        if writ_core::workspace::path_has_ignored_name(notes_root, path) {
            continue;
        }

        // Before anything that opens the file: a placeholder is materialised by
        // the read, not by the decision to read.
        let dataless = is_dataless(path);
        if dataless {
            outcome.skipped_dataless += 1;
            // The kind question is settled by the extension or not at all:
            // `should_index`'s fallback sniffs the bytes, which is the read
            // this whole branch exists to avoid.
            if !has_text_extension(path) {
                continue;
            }
        }

        let Ok(metadata) = std::fs::metadata(path) else {
            continue;
        };
        let size = metadata.len();

        if !dataless {
            // The same ceiling `BufferStore::index_note` applies. Indexing a
            // file the save path will not re-index leaves its first contents in
            // the index for good. The ceiling bounds a read, so a placeholder,
            // which is never read, is not measured against it: an evicted
            // 20 MB note keeps its name in the index.
            if size > THRESHOLD_NORMAL_BYTES {
                continue;
            }
            if !should_index(path) {
                continue;
            }
        }

        let mtime = mtime_millis(&metadata);
        let key = index_key(path);

        // A download leaves size and mtime alone, so an unchanged file whose
        // row holds nothing but its name is read now: this is the pass where a
        // placeholder became a note.
        let downloaded = !dataless && name_only.contains(&key);

        let expected = match known.get(&key) {
            Some(&state) if state == (size, mtime) && !downloaded && !rebuild_facts => {
                seen.push(key);
                continue;
            }
            other => other.copied(),
        };

        // A placeholder is indexed by name and nothing else: the row and the
        // name entry are what make it findable and openable, and the empty
        // text is the honest record of what has been read (ADR-028 section 7).
        let content = if dataless {
            String::new()
        } else {
            let Ok(content) = std::fs::read_to_string(path) else {
                // Not text after all (or unreadable): leave it out rather than
                // index bytes nobody can search, and let any row it already had
                // be pruned. A row kept here would answer searches with text the
                // file no longer holds. Same answer the size gate above gives.
                continue;
            };
            content
        };

        let name = entry.file_name().to_string_lossy().into_owned();
        seen.push(key.clone());
        names.insert(&key);
        let note = IndexedNote {
            path: key,
            name,
            size,
            mtime,
            // No read happened for a placeholder, so there is no digest to
            // record and the row says so in `indexed_by`.
            hash: None,
            indexed_by: if dataless {
                IndexedBy::Name
            } else {
                IndexedBy::Content
            },
        };

        // One transaction per file. The walk runs on its own connection, so a
        // long-held write lock would stall saves for the length of the walk.
        if index.upsert_walked(&note, &content, expected, &names)? {
            if expected.is_some() {
                outcome.updated += 1;
            } else {
                outcome.added += 1;
            }
        }
    }

    if !outcome.cancelled {
        let seen: std::collections::HashSet<String> = seen.into_iter().collect();
        for path in known.keys() {
            if seen.contains(path) {
                continue;
            }
            index.remove(path)?;
            outcome.removed += 1;
        }
    }

    // After the removals, so a link pointing at a note this pass deleted is
    // emptied rather than left pointing at a row that is gone. A cancelled pass
    // still runs it: what it wrote is real, and the links it left unresolved
    // are the ones it can resolve.
    index.resolve_links(&index.name_index()?, None)?;

    // Only a complete pass may speak for the whole folder. A cancelled one has
    // written fewer derived rows than the folder holds, and recording that
    // would tell the next pass the tables had been emptied.
    if !outcome.cancelled {
        let after = index.facts_census()?;
        schema_meta::set(
            conn,
            schema_meta::KEY_NOTES_FACTS_CENSUS,
            &format!("{}:{}", after.0, after.1),
        )?;
    }

    Ok(outcome)
}

/// The derived-row census the last complete [`reconcile`] recorded, or `None`
/// when there is none or the row is not the pair it should be.
fn read_facts_census(conn: &Connection) -> StorageResult<Option<(i64, i64)>> {
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

/// A folder's index key with a trailing separator, the form a prefix
/// comparison needs so a folder never claims a sibling whose name it is a
/// prefix of.
fn root_prefix(path: &Path) -> String {
    let mut text = index_key(path);
    if !text.ends_with(std::path::MAIN_SEPARATOR) {
        text.push(std::path::MAIN_SEPARATOR);
    }
    text
}

/// Whether `path` names a note rather than one of the other text files the
/// index holds.
///
/// The extension set is [`writ_core::notes::links`]'s, reached through
/// [`links::strip_note_extension`] so the graph calls a file a note exactly
/// when a `[[…]]` can name it. `.txt` and `.text` are indexed, searchable and
/// openable, and they are not notes.
fn is_note_file(path: &str) -> bool {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    links::strip_note_extension(name).len() != name.len()
}

/// The first path segment of `path` under the notes root.
///
/// Empty for a note in the root itself, and empty for a path the root does not
/// contain: a graph groups by folder, and a note with no folder above it
/// belongs to no group rather than to an invented one.
fn folder_segment(path: &str, root_prefix: &str) -> String {
    let Some(relative) = path.strip_prefix(root_prefix) else {
        return String::new();
    };
    match relative.split_once(['/', '\\']) {
        Some((first, _)) => first.to_string(),
        None => String::new(),
    }
}

/// Whether `path`'s name is one no listing, index or watcher event carries:
/// the name `write_atomic` gives the file it writes before renaming it into
/// place, a sync client's in-flight file, an undownloaded placeholder, an
/// editor swap file.
///
/// A walk that runs while a note is being saved would otherwise index the
/// half-written copy and leave a row for a file that no longer exists. The
/// notes watcher and the tree listing ask the same predicate.
fn is_ignored_path(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| writ_core::workspace::is_ignored_name(&name.to_string_lossy()))
}

/// Whether the index would hold `path` as a note, size aside.
///
/// The questions [`reconcile`]'s walk asks about a file it has reached, in its
/// order and with its answers: a name no listing carries, a placeholder whose
/// kind can only be read off the name, and the kind test itself. Size is left
/// out, because a note over the ceiling is one the index refuses and a link
/// still reaches.
///
/// The pruning half of the walk is [`build_walk`], which every caller of this
/// runs first.
pub(crate) fn indexes_as_note(root: &Path, path: &Path) -> bool {
    if writ_core::workspace::path_has_ignored_name(root, path) {
        return false;
    }
    if is_dataless(path) {
        return has_text_extension(path);
    }
    should_index(path)
}

/// The half of [`indexes_as_note`] that answers from the name alone.
///
/// For a file the walk reached but will not open: a placeholder, whose bytes
/// the read would pull down (ADR-028 §5), and a symlink, whose target is
/// wherever the link points and outside everything this walk was pointed at.
pub(crate) fn names_a_note(root: &Path, path: &Path) -> bool {
    !writ_core::workspace::path_has_ignored_name(root, path) && has_text_extension(path)
}

/// Whether `path` holds note text worth indexing.
///
/// A known text extension is taken at its word so the common case never opens
/// the file, which means this answers the *kind* question only: callers gate
/// on size themselves. Anything without a known extension is classified, and
/// only a file that opens with the full feature set is indexed.
fn should_index(path: &Path) -> bool {
    if is_ignored_path(path) {
        return false;
    }

    if has_text_extension(path) {
        return true;
    }
    matches!(
        classify_path(path).map(|c| c.mode),
        Ok(FileOpenMode::Normal)
    )
}

/// Whether `path` carries one of the [`TEXT_EXTENSIONS`].
///
/// The half of the kind question that answers from the name, which is the only
/// half a file with no local data behind it can be asked.
fn has_text_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| TEXT_EXTENSIONS.iter().any(|t| t.eq_ignore_ascii_case(ext)))
}

/// Modification time in milliseconds since the Unix epoch, or `0` when the
/// filesystem does not report one. Milliseconds rather than seconds so two
/// edits inside one second are still two different index states.
fn mtime_millis(metadata: &std::fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// The notes index over a connection of its own.
///
/// The adapter's [`crate::buffer_store::BufferStore`] holds a single connection
/// behind a mutex that every save and every tab operation queues on. A
/// reconcile of a large notes folder would hold it for the whole walk, and a
/// keystroke would wait for it, so the index that the walk and the search box
/// use opens its own connection to the same database. WAL lets the two proceed
/// together: readers do not block the writer, and the walk commits one file at
/// a time so the write lock is never held for long.
pub struct NotesIndexStore {
    conn: Mutex<Connection>,
    generation: AtomicU64,
}

impl NotesIndexStore {
    /// Opens a connection of its own to the database at `db_path`.
    ///
    /// Migrations are not run here. The primary connection runs them inside
    /// `AppState::initialize` before this one is opened, which is the same
    /// ordering the layout store relies on.
    pub fn open(db_path: &Path) -> StorageResult<Self> {
        Ok(Self {
            conn: Mutex::new(crate::database::connection::open_database(db_path)?),
            generation: AtomicU64::new(0),
        })
    }

    /// Opens the database at `db_path` for reading only.
    ///
    /// For a process that is not the app: the `writ` command reads the index
    /// this way so it can never create a database, run a migration or change a
    /// row. Every write method on this type fails on the connection it returns,
    /// which is the point.
    ///
    /// An absent file is an error rather than an empty database. An existing
    /// one in WAL mode still gets its `-shm` and `-wal` companions created if
    /// they are not already there: SQLite needs the shared-memory index to read
    /// a WAL database at all, and it writes no frame into either.
    pub fn open_read_only(db_path: &Path) -> StorageResult<Self> {
        Ok(Self {
            conn: Mutex::new(crate::database::connection::open_database_read_only(
                db_path,
            )?),
            generation: AtomicU64::new(0),
        })
    }

    /// The highest migration version the open database records. See
    /// [`crate::database::migrations::applied_schema_version`].
    pub fn schema_version(&self) -> StorageResult<i32> {
        crate::database::migrations::applied_schema_version(&self.conn())
    }

    /// Which folder the index is describing, as a number that changes whenever
    /// it changes.
    ///
    /// A walk takes seconds. If the notes folder moves while one is running,
    /// that walk finishes against the old folder and prunes every row it did
    /// not see, which is every row the move re-keyed. A walk therefore captures
    /// this value when it starts and gives up as soon as it differs.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    /// Retires every walk in flight. Called before the rows move, so no walk
    /// started against the old folder can still be running when they do.
    pub fn bump_generation(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    /// Re-keys the index from one notes folder to another. See
    /// [`NotesIndex::rekey_root`].
    pub fn rekey_root(&self, from: &Path, to: &Path) -> StorageResult<usize> {
        NotesIndex::new(&self.conn()).rekey_root(from, to)
    }

    /// A poisoned index lock is recovered rather than cascaded: the index is
    /// derived data that a reconcile rebuilds, so a panic while holding it
    /// costs at most a stale row, and refusing every later search would cost
    /// the user the feature.
    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Up to `limit` ranked content hits. See [`NotesIndex::search_hits`].
    pub fn search_hits(
        &self,
        query: &str,
        terms: &[String],
        limit: usize,
    ) -> StorageResult<Vec<SearchHit>> {
        NotesIndex::new(&self.conn()).search_hits(query, terms, limit)
    }

    /// Total number of notes matching `query`. See [`NotesIndex::count`].
    pub fn count(&self, query: &str) -> StorageResult<usize> {
        NotesIndex::new(&self.conn()).count(query)
    }

    /// Up to `limit` ranked name hits under `notes_root`. See
    /// [`NotesIndex::search_names`].
    pub fn search_names(
        &self,
        query: &str,
        notes_root: &Path,
        limit: usize,
    ) -> StorageResult<Vec<FileHit>> {
        NotesIndex::new(&self.conn()).search_names(query, notes_root, limit)
    }

    /// Every link written in the note at `path`. See [`NotesIndex::links_from`].
    pub fn links_from(&self, path: &str) -> StorageResult<Vec<LinkRow>> {
        NotesIndex::new(&self.conn()).links_from(path)
    }

    /// Every link that resolved to `path`. See [`NotesIndex::links_to`].
    pub fn links_to(&self, path: &str) -> StorageResult<Vec<LinkRow>> {
        NotesIndex::new(&self.conn()).links_to(path)
    }

    /// Every note a link could reach, for a caller resolving links itself.
    ///
    /// A rename asks the same question of a link that the index asks: which
    /// note does this reach. Handing out the candidate list is what keeps the
    /// two answers the same one, so a rewrite never repoints a link the
    /// backlink list says belongs to another note.
    pub fn note_paths(&self) -> StorageResult<Vec<String>> {
        NotesIndex::new(&self.conn()).paths()
    }

    /// The notes that link to `path`. See [`NotesIndex::backlinks`].
    pub fn backlinks(&self, path: &str) -> StorageResult<Vec<BacklinkRow>> {
        NotesIndex::new(&self.conn()).backlinks(path)
    }

    /// Everything the index holds about `path`. See [`NotesIndex::facts`].
    pub fn facts(&self, path: &str) -> StorageResult<NoteFactsRow> {
        NotesIndex::new(&self.conn()).facts(path)
    }

    /// How much of the note at `path` the index holds. See
    /// [`NotesIndex::indexed_by`].
    pub fn indexed_by(&self, path: &str) -> StorageResult<Option<IndexedBy>> {
        NotesIndex::new(&self.conn()).indexed_by(path)
    }

    /// Every tag the index holds, with a note count each. See
    /// [`NotesIndex::all_tags`].
    pub fn all_tags(&self) -> StorageResult<Vec<(String, usize)>> {
        NotesIndex::new(&self.conn()).all_tags()
    }

    /// Every note carrying one tag. See [`NotesIndex::paths_for_tag`].
    pub fn paths_for_tag(&self, tag: &str) -> StorageResult<Vec<String>> {
        NotesIndex::new(&self.conn()).paths_for_tag(tag)
    }

    /// Every note in the folder and the resolved links among them. See
    /// [`NotesIndex::graph`].
    pub fn graph(&self, notes_root: &Path) -> StorageResult<GraphRows> {
        NotesIndex::new(&self.conn()).graph(notes_root)
    }

    /// The notes a link naming `name` could mean. See
    /// [`NotesIndex::candidate_paths`].
    pub fn candidate_paths(&self, name: &str) -> StorageResult<Vec<String>> {
        NotesIndex::new(&self.conn()).candidate_paths(name)
    }

    /// Resolves a link target. See [`NotesIndex::resolve_link`].
    pub fn resolve_link(&self, from_path: &str, target: &str) -> StorageResult<Resolution> {
        NotesIndex::new(&self.conn()).resolve_link(from_path, target)
    }

    /// The line a heading anchor is on. See [`NotesIndex::heading_line`].
    pub fn heading_line(&self, path: &str, slug: &str) -> StorageResult<Option<u32>> {
        NotesIndex::new(&self.conn()).heading_line(path, slug)
    }

    /// Records the file at `path` in the index, reading its text.
    ///
    /// The watcher's create-or-modify arm. A file the filesystem reports as not
    /// downloaded, one over [`THRESHOLD_NORMAL_BYTES`], or one that is not note
    /// text is left alone rather than materialised or indexed as bytes; `false`
    /// says nothing was recorded.
    pub fn index_path(&self, path: &Path) -> StorageResult<bool> {
        if is_dataless(path) {
            return Ok(false);
        }
        let Some(note) = IndexedNote::from_file(path) else {
            return Ok(false);
        };
        if note.size > THRESHOLD_NORMAL_BYTES || !should_index(path) {
            return Ok(false);
        }
        let Ok(content) = std::fs::read_to_string(path) else {
            return Ok(false);
        };
        NotesIndex::new(&self.conn()).upsert(&note, &content)?;
        Ok(true)
    }

    /// Removes the file at `path` from the index.
    ///
    /// The watcher's delete arm, and the second half of a rename. Removing a
    /// path the index does not hold is not an error, which is what lets the
    /// subscriber replay an event without checking first.
    ///
    /// The links that pointed at the removed note lose their target in
    /// [`NotesIndex::remove`]; they are re-resolved here, so a second note of
    /// the same name left standing picks them up rather than waiting for a
    /// walk.
    pub fn forget_path(&self, path: &Path) -> StorageResult<()> {
        let key = index_key(path);
        let conn = self.conn();
        let index = NotesIndex::new(&conn);
        index.remove(&key)?;
        let names = index.name_index()?;
        index.resolve_links(&names, Some(&links::candidate_name_keys(&key)))?;
        Ok(())
    }

    /// Walks `notes_root` and brings the index in line with it. See
    /// [`reconcile`].
    pub fn reconcile(
        &self,
        notes_root: &Path,
        cancelled: &dyn Fn() -> bool,
        is_dataless: &dyn Fn(&Path) -> bool,
    ) -> StorageResult<ReconcileOutcome> {
        reconcile(&self.conn(), notes_root, cancelled, is_dataless)
    }

    /// Every indexed path with its size and mtime. See [`NotesIndex::snapshot`].
    pub fn snapshot(&self) -> StorageResult<Vec<(String, u64, i64)>> {
        NotesIndex::new(&self.conn()).snapshot()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dataless_from_flags_reads_the_sf_dataless_bit() {
        assert!(dataless_from_flags(SF_DATALESS));
        assert!(dataless_from_flags(SF_DATALESS | 0x0000_0002));
    }

    #[test]
    fn dataless_from_flags_ignores_every_other_flag() {
        assert!(!dataless_from_flags(0));
        assert!(!dataless_from_flags(0x0000_0002));
        assert!(!dataless_from_flags(0x0080_0000));
    }

    #[test]
    fn a_file_with_local_data_is_not_dataless_on_any_platform() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = dir.path().join("here.md");
        std::fs::write(&path, "local").expect("write");
        assert!(!is_dataless(&path));
    }

    #[test]
    fn index_key_of_a_missing_file_matches_the_key_it_had() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = dir.path().join("gone.md");
        std::fs::write(&path, "body").expect("write");
        let before = index_key(&path);
        std::fs::remove_file(&path).expect("remove");
        assert_eq!(before, index_key(&path));
    }

    #[test]
    fn should_index_accepts_the_text_extensions() {
        for name in ["a.md", "a.markdown", "a.txt", "a.text", "a.MD"] {
            assert!(
                should_index(Path::new(name)),
                "{name} must be indexed without being opened"
            );
        }
    }
}
