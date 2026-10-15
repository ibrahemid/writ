//! The path-keyed index over the notes folder (ADR-028 section 7).
//!
//! Files are the only copy of the text, so the index is keyed by the canonical
//! path of the file rather than by a database row. [`NotesIndexStore`] is the
//! index the app holds: one writer connection that the walk and every save
//! lock for one file at a time, and read connections every command reads
//! through, so quick open, link completion and the backlink list answer while
//! a walk is under way (ADR-036 section 7). [`NotesIndex`] is the same surface
//! over one borrowed connection. [`reconcile`] walks the notes folder and
//! brings the index back in line with what is on disk, which is what makes
//! deleting `writ.db` safe and what picks up notes written by another editor
//! while Writ was closed.
//!
//! The parts:
//!
//! - `schema`: the folded name every link row carries (migration 045), and the
//!   census a complete walk records.
//! - `upsert`: the writes, one file per transaction.
//! - `reconcile`: the walk.
//! - `queries`: the reads.
//! - `resolve`: the names a link can mean, kept in memory, and the passes that
//!   point stored links at the notes they name.
//! - `walk`: which files are notes, and how the notes folder is walked.
//! - `store`: the writer connection, the read connections and the names.
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

use std::path::Path;

use rusqlite::Connection;
use writ_core::notes::extensions::has_markdown_extension;

mod queries;
mod reconcile;
mod resolve;
mod schema;
mod store;
mod upsert;
mod walk;
mod writer;

pub use reconcile::{reconcile, ReconcileOutcome};
pub use store::NotesIndexStore;
pub use walk::{has_text_extension, is_dataless, notes_walk_skips};

pub(crate) use schema::{backfill_folded, backfill_link_name_keys};
pub(crate) use walk::{build_notes_walk, indexes_as_note, names_a_note};

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
            mtime: walk::mtime_millis(&metadata),
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

/// One file as the folder index lists it (ADR-052).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderIndexRow {
    /// Canonical path of the file.
    pub path: String,
    /// The file's length in bytes, as last indexed.
    pub bytes: u64,
    /// The frontmatter `title` when it is a string, else the text of the
    /// file's first heading, else `None`.
    pub title: Option<String>,
    /// Each tag the file carries, once, in the order first written.
    pub tags: Vec<String>,
    /// Each file its links resolve to, once, in path order. A link that
    /// resolves nowhere, or to the file itself, is left out.
    pub links: Vec<String>,
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

/// Read and write access to the notes index over a borrowed connection.
///
/// Every call works out what it needs from the connection it is given, so a
/// call that resolves a link reads the folder's names afresh. That suits a
/// test or a one-off tool. The app holds a [`NotesIndexStore`], which keeps
/// the names in memory and reads through connections of its own.
pub struct NotesIndex<'a> {
    conn: &'a Connection,
}

impl<'a> NotesIndex<'a> {
    /// Constructs an index view over the given connection.
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }
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

/// Whether `path` names a Markdown note rather than one of the other text
/// files the index holds.
///
/// The one rule for the graph and for the facts a file is read for: a link
/// can name `.md` and `.markdown` by their name alone, and the graph draws
/// them. `.txt` and `.text` are indexed, searchable and openable, and they
/// are plain text: no links, tags, headings or properties are read out of
/// them and they are no node (ADR-041 §2).
fn is_note_file(path: &str) -> bool {
    has_markdown_extension(Path::new(path))
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

/// The last segment of an index key: the file's name.
fn file_name_of(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn only_markdown_files_are_notes_of_the_graph() {
        assert!(is_note_file("/n/a.md"));
        assert!(is_note_file("/n/a.Markdown"));
        assert!(!is_note_file("/n/a.txt"));
        assert!(!is_note_file("/n/a.json"));
    }

    #[test]
    fn a_file_name_is_the_last_segment_with_either_separator() {
        assert_eq!(file_name_of("/n/sub/Plan.md"), "Plan.md");
        assert_eq!(file_name_of("C:\\n\\sub\\Plan.md"), "Plan.md");
        assert_eq!(file_name_of("Plan.md"), "Plan.md");
    }
}
