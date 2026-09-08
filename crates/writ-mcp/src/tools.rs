//! The tool surface, in plain Rust.
//!
//! No `rmcp` type appears in any signature here: the protocol lives in
//! [`crate::server`] and this module is what an SDK bump does not touch. Every
//! method takes the calling client and puts it to the [`ConsentGate`] before it
//! opens anything, so a refusal costs no read (ADR-031 rule 3.2).
//!
//! Notes are read from the folder and facts about them from the index, which is
//! opened read-only: this process creates no database, runs no migration and
//! changes no row (ADR-031 rule 1.3). With the index absent or unreadable,
//! [`ToolHost::list_notes`] and [`ToolHost::read_note`] still answer from the
//! folder and the six index-derived tools return [`ToolError::IndexUnavailable`].
//!
//! The method bodies are shaped for U9 to lift onto `writ_plugin::host::NoteHost`
//! (ADR-032 section 3): the consent check is the first line and the rest of the
//! body is the operation, so the check can be replaced by a capability check
//! without the operation moving.

use std::path::{Path, PathBuf};

use writ_core::notes::containment::{resolve_for_containment, resolve_inside};
use writ_storage::database::migrations::binary_schema_version;
use writ_storage::notes_index::{self, BacklinkCertainty, NotesIndexStore};

use crate::consent::{ClientId, ConsentGate, Decision};

/// Largest note a tool reads, in bytes (ADR-031 rule 4.8).
pub const MAX_NOTE_BYTES: u64 = 2 * 1024 * 1024;

/// Most notes or hits one call answers with, whatever the caller asked for.
pub const MAX_RESULTS: usize = 500;

/// The extension `list_notes` counts as a note.
const NOTE_EXTENSION: &str = "md";

/// The tools registered in 0.5's read half, in the order `tools/list` reports
/// them. No write tool is in this crate.
pub const READ_TOOLS: &[&str] = &[
    "list_notes",
    "search_notes",
    "read_note",
    "note_links",
    "note_backlinks",
    "note_properties",
    "note_tags",
    "folder_tags",
];

/// The tools that answer from the index and cannot answer without it.
pub const INDEX_TOOLS: &[&str] = &[
    "search_notes",
    "note_links",
    "note_backlinks",
    "note_properties",
    "note_tags",
    "folder_tags",
];

/// Why a tool call produced no answer.
///
/// Every message names a path, a client or a tool and nothing else: no note
/// text reaches a client's error rendering (ADR-031 rule 5.3).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ToolError {
    /// The client has no approval for this direction.
    #[error(
        "{client} is not approved to read notes. Approve it in Writ, under Connected programs."
    )]
    NotApproved {
        /// The name the client sent.
        client: String,
        /// The tool it called.
        tool: String,
    },
    /// The path argument names something the notes folder does not hold.
    #[error("{path} is not in the notes folder.")]
    OutsideNotesFolder {
        /// The path as the client wrote it.
        path: String,
    },
    /// The index is absent, unreadable, or at another version.
    #[error("The note index is not readable. Open Writ once and it builds one.")]
    IndexUnavailable,
    /// Nothing is at the path.
    #[error("There is no note at {path}.")]
    NotFound {
        /// The path as the client wrote it.
        path: String,
    },
    /// The file is over [`MAX_NOTE_BYTES`].
    #[error("{path} is {bytes} bytes. A tool reads up to {MAX_NOTE_BYTES} bytes.")]
    TooLarge {
        /// The path as the client wrote it.
        path: String,
        /// The file's length.
        bytes: u64,
    },
    /// The file is there and this process could not read it: no permission, or
    /// text that is not UTF-8.
    #[error("{path} could not be read.")]
    Unreadable {
        /// The path as the client wrote it.
        path: String,
    },
}

/// One note in the folder.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NoteSummary {
    /// The note's path, in the spelling every other tool takes back.
    pub path: String,
    /// What the note is called: the file name without its extension.
    pub name: String,
    /// The file's length in bytes.
    pub bytes: u64,
}

/// A note's text, as the file holds it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NoteContent {
    /// The note's path.
    pub path: String,
    /// The file's length in bytes.
    pub bytes: u64,
    /// The whole file, frontmatter included.
    pub text: String,
}

/// One search hit.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SearchResult {
    /// The note's path.
    pub path: String,
    /// What the note is called: the file name without its extension, the same
    /// shape `list_notes` takes back.
    pub name: String,
    /// 1-based line the match is on, or `None` when the name matched.
    pub line: Option<u32>,
    /// The matching line, cut to a readable length.
    pub excerpt: String,
}

/// One link written in a note.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NoteLink {
    /// The link's target as it was written: no alias, no heading.
    pub target: String,
    /// The note the target resolved to. `None` when it resolved to nothing, and
    /// `None` when it names more than one note: an ambiguous link is never
    /// resolved to a guess (ADR-036 section 6).
    pub resolved_path: Option<String>,
    /// `wikilink` or `markdown`.
    pub kind: String,
    /// 1-based line the link is on.
    pub line: u32,
    /// 0-based character offset of the link inside that line.
    pub column: u32,
}

/// One link in another note that points at this one.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NoteBacklink {
    /// Path of the note the link is written in.
    pub from_path: String,
    /// What that note is called.
    pub from_name: String,
    /// The link's target as it was written.
    pub target: String,
    /// A wikilink's `|alias`, when it has one.
    pub alias: Option<String>,
    /// `wikilink` or `markdown`.
    pub kind: String,
    /// 1-based line the link is on.
    pub line: u32,
    /// 0-based character offset of the link inside that line.
    pub column: u32,
    /// The sentence the link sits in.
    pub context: String,
    /// `resolved` when the link means this note and no other, `ambiguous` when
    /// it names this one and at least one more.
    pub certainty: String,
    /// The other notes an ambiguous link might mean. Empty for a resolved one.
    pub candidates: Vec<String>,
}

/// One frontmatter property.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NoteProperty {
    /// The key as the frontmatter spells it.
    pub name: String,
    /// The value, as the JSON the index stores it as.
    pub value: String,
}

/// One `#tag` written in a note.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NoteTag {
    /// The tag, without its `#`.
    pub tag: String,
    /// 1-based line it is on.
    pub line: u32,
}

/// One tag in the folder, with how many notes carry it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FolderTag {
    /// The tag, without its `#`.
    pub tag: String,
    /// How many notes carry it.
    pub notes: usize,
}

/// The notes folder and the index over it, behind a consent gate.
pub struct ToolHost {
    notes_root: PathBuf,
    index: Option<NotesIndexStore>,
    gate: Box<dyn ConsentGate>,
}

impl std::fmt::Debug for ToolHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolHost")
            .field("notes_root", &self.notes_root)
            .field("index", &self.index.is_some())
            .finish_non_exhaustive()
    }
}

impl ToolHost {
    /// Opens the folder at `notes_root` and, if it can, the index at `db_path`.
    ///
    /// An index that is absent, unreadable, or written to another schema
    /// version leaves the host without one rather than failing: the two tools
    /// that read the folder still answer, and the rest say so
    /// ([`ToolError::IndexUnavailable`]).
    ///
    /// The root is resolved here so every containment check compares two paths
    /// the filesystem spells the same way.
    pub fn open(
        notes_root: &Path,
        db_path: &Path,
        gate: Box<dyn ConsentGate>,
    ) -> Result<Self, ToolError> {
        let resolved = resolve_for_containment(notes_root)
            .filter(|root| root.is_dir())
            .ok_or_else(|| ToolError::NotFound {
                path: notes_root.display().to_string(),
            })?;
        Ok(Self {
            notes_root: resolved,
            index: open_index(db_path),
            gate,
        })
    }

    /// The folder every path argument is checked against.
    pub fn notes_root(&self) -> &Path {
        &self.notes_root
    }

    /// Whether the index answered when the host was opened.
    pub fn has_index(&self) -> bool {
        self.index.is_some()
    }

    /// Every note in the folder, path-ordered.
    ///
    /// `prefix` is matched against the path relative to the folder and against
    /// the path this method hands back, so `Projects` and the folder's own
    /// spelling of it both list that folder and nothing else. Answers from the
    /// folder, not the index: the file is the only copy (ADR-028 section 1).
    pub fn list_notes(
        &self,
        client: &ClientId,
        prefix: Option<&str>,
        limit: usize,
    ) -> Result<Vec<NoteSummary>, ToolError> {
        self.allow(client, "list_notes")?;

        let mut notes = Vec::new();
        for entry in writ_storage::workspace_search::build_walk(&self.notes_root).build() {
            let Ok(entry) = entry else { continue };
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }
            let path = entry.path();
            if writ_core::workspace::path_has_ignored_name(&self.notes_root, path) {
                continue;
            }
            if !path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case(NOTE_EXTENSION))
            {
                continue;
            }
            let Some(relative) = relative_slug(&self.notes_root, path) else {
                continue;
            };
            let key = notes_index::index_key(path);
            if let Some(prefix) = prefix {
                if !relative.starts_with(prefix) && !key.starts_with(prefix) {
                    continue;
                }
            }
            notes.push(NoteSummary {
                name: writ_core::notes::note_display_name(&key),
                path: key,
                bytes: std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0),
            });
        }
        notes.sort_by(|a, b| a.path.cmp(&b.path));
        notes.truncate(limit.min(MAX_RESULTS));
        Ok(notes)
    }

    /// Up to `limit` notes whose text matches `query`.
    ///
    /// The query is turned into the same prefix-match expression the app's own
    /// search builds, so a tool and the window rank one folder the same way and
    /// an FTS operator in the argument never reaches the `MATCH` parser.
    pub fn search_notes(
        &self,
        client: &ClientId,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchResult>, ToolError> {
        self.allow(client, "search_notes")?;
        let index = self.index()?;

        let Some(expression) = writ_core::search::to_prefix_match(query) else {
            return Ok(Vec::new());
        };
        let terms = writ_core::search::search_terms(query);
        let hits = index
            .search_hits(&expression, &terms, limit.min(MAX_RESULTS))
            .map_err(|_| ToolError::IndexUnavailable)?;

        Ok(hits
            .into_iter()
            .map(|hit| {
                let path = hit.path.unwrap_or_default();
                SearchResult {
                    name: writ_core::notes::note_display_name(&path),
                    path,
                    line: hit.line,
                    excerpt: hit
                        .snippet
                        .into_iter()
                        .map(|segment| segment.text)
                        .collect(),
                }
            })
            .collect())
    }

    /// The whole file at `path`, frontmatter included.
    pub fn read_note(&self, client: &ClientId, path: &str) -> Result<NoteContent, ToolError> {
        self.allow(client, "read_note")?;
        let file = self.note_file(path)?;

        let bytes = std::fs::metadata(&file)
            .map_err(|_| ToolError::NotFound {
                path: path.to_string(),
            })?
            .len();
        if bytes > MAX_NOTE_BYTES {
            return Err(ToolError::TooLarge {
                path: path.to_string(),
                bytes,
            });
        }
        let text = std::fs::read_to_string(&file).map_err(|_| ToolError::Unreadable {
            path: path.to_string(),
        })?;
        Ok(NoteContent {
            path: notes_index::index_key(&file),
            bytes,
            text,
        })
    }

    /// Every link written in the note at `path`.
    pub fn note_links(&self, client: &ClientId, path: &str) -> Result<Vec<NoteLink>, ToolError> {
        self.allow(client, "note_links")?;
        let index = self.index()?;
        let key = self.note_key(path)?;

        let rows = index
            .links_from(&key)
            .map_err(|_| ToolError::IndexUnavailable)?;
        Ok(rows
            .into_iter()
            .map(|row| NoteLink {
                target: row.to_target,
                resolved_path: row.to_path,
                kind: row.kind,
                line: row.line,
                column: row.col,
            })
            .collect())
    }

    /// Every link in another note that points at the note at `path`.
    pub fn note_backlinks(
        &self,
        client: &ClientId,
        path: &str,
    ) -> Result<Vec<NoteBacklink>, ToolError> {
        self.allow(client, "note_backlinks")?;
        let index = self.index()?;
        let key = self.note_key(path)?;

        let rows = index
            .backlinks(&key)
            .map_err(|_| ToolError::IndexUnavailable)?;
        Ok(rows
            .into_iter()
            .map(|row| NoteBacklink {
                from_path: row.from_path,
                from_name: row.from_name,
                target: row.to_target,
                alias: row.alias,
                kind: row.kind,
                line: row.line,
                column: row.col,
                context: row.context,
                certainty: certainty_word(row.certainty).to_string(),
                candidates: row.candidates,
            })
            .collect())
    }

    /// The frontmatter properties of the note at `path`.
    pub fn note_properties(
        &self,
        client: &ClientId,
        path: &str,
    ) -> Result<Vec<NoteProperty>, ToolError> {
        self.allow(client, "note_properties")?;
        let facts = self.facts(path)?;
        Ok(facts
            .properties
            .into_iter()
            .map(|(name, value)| NoteProperty { name, value })
            .collect())
    }

    /// The tags written in the note at `path`.
    pub fn note_tags(&self, client: &ClientId, path: &str) -> Result<Vec<NoteTag>, ToolError> {
        self.allow(client, "note_tags")?;
        let facts = self.facts(path)?;
        Ok(facts
            .tags
            .into_iter()
            .map(|(tag, line)| NoteTag { tag, line })
            .collect())
    }

    /// Every tag in the folder, with the number of notes carrying each.
    pub fn folder_tags(&self, client: &ClientId) -> Result<Vec<FolderTag>, ToolError> {
        self.allow(client, "folder_tags")?;
        let index = self.index()?;
        let rows = index.all_tags().map_err(|_| ToolError::IndexUnavailable)?;
        Ok(rows
            .into_iter()
            .map(|(tag, notes)| FolderTag { tag, notes })
            .collect())
    }

    /// Refuses the call unless the gate allows this client this tool.
    ///
    /// Called first by every method, so a refusal opens no file and runs no
    /// query. `Pending` refuses too: U5 is what turns it into a row the user
    /// can act on.
    fn allow(&self, client: &ClientId, tool: &str) -> Result<(), ToolError> {
        match self.gate.decide(client, tool) {
            Decision::Allow => Ok(()),
            Decision::Refuse | Decision::Pending => Err(ToolError::NotApproved {
                client: client.name.clone(),
                tool: tool.to_string(),
            }),
        }
    }

    /// The index, or [`ToolError::IndexUnavailable`] when there is none.
    fn index(&self) -> Result<&NotesIndexStore, ToolError> {
        self.index.as_ref().ok_or(ToolError::IndexUnavailable)
    }

    /// The file a path argument names, refusing anything the folder does not
    /// hold.
    ///
    /// A path that is not absolute is read from the notes folder, which is the
    /// spelling `writ read` already takes and the one a client writes after
    /// seeing a name. Joining happens before resolution, so `../` in a relative
    /// argument is walked and refused like any other way out. Resolution
    /// happens before the file is opened, so a symlink out of the folder is
    /// refused rather than followed (ADR-031 rule 3.7).
    fn note_file(&self, path: &str) -> Result<PathBuf, ToolError> {
        let given = Path::new(path);
        let candidate = if given.is_absolute() {
            given.to_path_buf()
        } else {
            self.notes_root.join(given)
        };
        let file = resolve_inside(&self.notes_root, &candidate).ok_or_else(|| {
            ToolError::OutsideNotesFolder {
                path: path.to_string(),
            }
        })?;
        if !file.is_file() {
            return Err(ToolError::NotFound {
                path: path.to_string(),
            });
        }
        Ok(file)
    }

    /// The index key of the note a path argument names.
    fn note_key(&self, path: &str) -> Result<String, ToolError> {
        Ok(notes_index::index_key(&self.note_file(path)?))
    }

    /// Everything the index holds about one note, read once for the two tools
    /// that cut a slice out of it (ADR-036 section 2).
    fn facts(&self, path: &str) -> Result<writ_storage::notes_index::NoteFactsRow, ToolError> {
        let index = self.index()?;
        let key = self.note_key(path)?;
        index.facts(&key).map_err(|_| ToolError::IndexUnavailable)
    }
}

/// The wire spelling of a backlink's certainty.
fn certainty_word(certainty: BacklinkCertainty) -> &'static str {
    certainty.as_str()
}

/// `path` relative to `root`, with forward slashes, or `None` when it is not
/// under the root.
fn relative_slug(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    Some(
        relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

/// Opens the index read-only, or `None` when there is nothing to open.
///
/// The schema check is `writ`'s: a database older than this build has columns a
/// read may not find, and a newer one was written by a build that knows more.
/// Both are the same situation to a client as an absent one, and none of the
/// three is repaired here.
fn open_index(db_path: &Path) -> Option<NotesIndexStore> {
    if !db_path.is_file() {
        return None;
    }
    let store = NotesIndexStore::open_read_only(db_path).ok()?;
    (store.schema_version().ok()? == binary_schema_version()).then_some(store)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consent::{DenyAll, EnabledReads};
    use tempfile::TempDir;

    /// A notes folder, and the path its index would live at.
    struct Fixture {
        _dir: TempDir,
        notes: PathBuf,
        db: PathBuf,
    }

    fn fixture() -> Fixture {
        let dir = TempDir::new().expect("temp dir");
        let notes = dir.path().join("notes");
        std::fs::create_dir_all(&notes).expect("notes folder");
        Fixture {
            db: dir.path().join("writ.db"),
            notes,
            _dir: dir,
        }
    }

    fn write_note(fixture: &Fixture, name: &str, body: &str) -> PathBuf {
        let path = fixture.notes.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent folder");
        }
        std::fs::write(&path, body).expect("write note");
        path
    }

    /// Builds the index the app would have built, in the same place.
    fn build_index(fixture: &Fixture) {
        let conn = writ_storage::database::connection::open_database(&fixture.db).expect("open");
        writ_storage::database::migrations::run_migrations(&conn).expect("migrations");
        drop(conn);
        NotesIndexStore::open(&fixture.db)
            .expect("index")
            .reconcile(&fixture.notes, &|| false, &|_| false)
            .expect("reconcile");
    }

    fn host(fixture: &Fixture) -> ToolHost {
        ToolHost::open(
            &fixture.notes,
            &fixture.db,
            Box::new(EnabledReads::new(true)),
        )
        .expect("host")
    }

    fn client() -> ClientId {
        ClientId::named("Test Client")
    }

    /// The path a tool takes back for a file on disk.
    fn key(path: &Path) -> String {
        notes_index::index_key(path)
    }

    #[test]
    fn list_notes_returns_every_markdown_file_and_nothing_else() {
        let fixture = fixture();
        write_note(&fixture, "Launch.md", "# Launch");
        write_note(&fixture, "Projects/Writ.md", "# Writ");
        write_note(&fixture, "notes.txt", "not a note");
        write_note(&fixture, "diagram.png", "not a note either");
        build_index(&fixture);

        let listed = host(&fixture)
            .list_notes(&client(), None, 100)
            .expect("list");

        let paths: Vec<&str> = listed.iter().map(|note| note.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                key(&fixture.notes.join("Launch.md")).as_str(),
                key(&fixture.notes.join("Projects/Writ.md")).as_str(),
            ]
        );
        assert_eq!(listed[0].name, "Launch");
        assert_eq!(listed[0].bytes, "# Launch".len() as u64);
    }

    #[test]
    fn list_notes_honours_a_prefix_and_a_limit() {
        let fixture = fixture();
        write_note(&fixture, "Launch.md", "one");
        write_note(&fixture, "Projects/Writ.md", "two");
        write_note(&fixture, "Projects/Tessera.md", "three");

        let host = host(&fixture);
        let under = host
            .list_notes(&client(), Some("Projects"), 100)
            .expect("list");
        assert_eq!(under.len(), 2);
        assert!(under.iter().all(|note| note.path.contains("Projects")));

        let listed_prefix = under[0].path.clone();
        let by_listed_path = host
            .list_notes(&client(), Some(&listed_prefix), 100)
            .expect("list");
        assert_eq!(by_listed_path.len(), 1);
        assert_eq!(by_listed_path[0].path, listed_prefix);

        let capped = host.list_notes(&client(), None, 1).expect("list");
        assert_eq!(capped.len(), 1);
    }

    #[test]
    fn search_notes_returns_what_the_index_returns_for_the_same_query() {
        let fixture = fixture();
        write_note(&fixture, "Launch.md", "the rerank pass ships on Monday");
        write_note(&fixture, "Other.md", "nothing to do with it");
        build_index(&fixture);

        let found = host(&fixture)
            .search_notes(&client(), "rerank", 50)
            .expect("search");

        let index = NotesIndexStore::open_read_only(&fixture.db).expect("index");
        let expression = writ_core::search::to_prefix_match("rerank").expect("query");
        let terms = writ_core::search::search_terms("rerank");
        let expected = index.search_hits(&expression, &terms, 50).expect("hits");

        assert_eq!(found.len(), expected.len());
        let found_paths: Vec<&str> = found.iter().map(|hit| hit.path.as_str()).collect();
        let expected_paths: Vec<&str> = expected
            .iter()
            .map(|hit| hit.path.as_deref().expect("path"))
            .collect();
        assert_eq!(found_paths, expected_paths);
        assert_eq!(found_paths, vec![key(&fixture.notes.join("Launch.md"))]);
    }

    #[test]
    fn search_notes_with_no_usable_term_answers_with_an_empty_list() {
        let fixture = fixture();
        write_note(&fixture, "Launch.md", "the rerank pass");
        build_index(&fixture);

        assert_eq!(
            host(&fixture)
                .search_notes(&client(), "  ", 50)
                .expect("search"),
            Vec::new()
        );
    }

    #[test]
    fn read_note_returns_the_file_unchanged_including_its_frontmatter() {
        let fixture = fixture();
        let body = "---\ntitle: Launch\nstatus: draft\n---\n\n# Launch\n\nthe text\n";
        let path = write_note(&fixture, "Launch.md", body);
        build_index(&fixture);

        let note = host(&fixture)
            .read_note(&client(), path.to_str().expect("utf-8"))
            .expect("read");

        assert_eq!(note.text, body);
        assert_eq!(note.bytes, body.len() as u64);
        assert_eq!(note.path, key(&path));
    }

    #[test]
    fn read_note_works_for_a_note_the_index_has_never_seen() {
        let fixture = fixture();
        let path = write_note(&fixture, "Launch.md", "written after the walk");

        let host = host(&fixture);
        assert!(!host.has_index());
        assert_eq!(
            host.read_note(&client(), path.to_str().expect("utf-8"))
                .expect("read")
                .text,
            "written after the walk"
        );
    }

    #[test]
    fn note_links_match_the_index_for_the_same_note() {
        let fixture = fixture();
        write_note(&fixture, "Target.md", "# Target");
        let path = write_note(&fixture, "Launch.md", "see [[Target]] for the rest\n");
        build_index(&fixture);

        let links = host(&fixture)
            .note_links(&client(), path.to_str().expect("utf-8"))
            .expect("links");

        let index = NotesIndexStore::open_read_only(&fixture.db).expect("index");
        let rows = index.links_from(&key(&path)).expect("rows");

        assert_eq!(links.len(), rows.len());
        assert_eq!(links[0].target, rows[0].to_target);
        assert_eq!(links[0].resolved_path, rows[0].to_path);
        assert_eq!(links[0].kind, rows[0].kind);
        assert_eq!(links[0].line, rows[0].line);
        assert_eq!(links[0].column, rows[0].col);
        assert_eq!(
            links[0].resolved_path.as_deref(),
            Some(key(&fixture.notes.join("Target.md")).as_str())
        );
    }

    #[test]
    fn note_backlinks_match_the_index_for_the_same_note() {
        let fixture = fixture();
        let target = write_note(&fixture, "Target.md", "# Target");
        write_note(&fixture, "Launch.md", "see [[Target]] for the rest\n");
        build_index(&fixture);

        let backlinks = host(&fixture)
            .note_backlinks(&client(), target.to_str().expect("utf-8"))
            .expect("backlinks");

        let index = NotesIndexStore::open_read_only(&fixture.db).expect("index");
        let rows = index.backlinks(&key(&target)).expect("rows");

        assert_eq!(backlinks.len(), rows.len());
        assert_eq!(backlinks[0].from_path, rows[0].from_path);
        assert_eq!(backlinks[0].from_name, rows[0].from_name);
        assert_eq!(backlinks[0].certainty, "resolved");
        assert!(backlinks[0].candidates.is_empty());
    }

    #[test]
    fn an_ambiguous_link_is_reported_as_ambiguous_and_not_resolved_to_a_guess() {
        let fixture = fixture();
        let one = write_note(&fixture, "Projects/Launch.md", "# one");
        write_note(&fixture, "Archive/Launch.md", "# another");
        write_note(&fixture, "Tessera.md", "# the other one");
        let linking = write_note(&fixture, "Plan.md", "see [[Launch]] and [[Tessera]]\n");
        build_index(&fixture);
        let host = host(&fixture);

        let backlinks = host
            .note_backlinks(&client(), one.to_str().expect("utf-8"))
            .expect("backlinks");
        assert_eq!(backlinks.len(), 1);
        assert_eq!(backlinks[0].certainty, "ambiguous");
        assert!(backlinks[0]
            .candidates
            .contains(&key(&fixture.notes.join("Archive/Launch.md"))));

        // The unambiguous link in the same note resolves, so the ambiguous one
        // carrying no path is the ambiguity and not an index that resolved
        // nothing at all.
        let links = host
            .note_links(&client(), linking.to_str().expect("utf-8"))
            .expect("links");
        let ambiguous = links
            .iter()
            .find(|link| link.target == "Launch")
            .expect("the ambiguous link");
        let resolved = links
            .iter()
            .find(|link| link.target == "Tessera")
            .expect("the resolved link");
        assert_eq!(ambiguous.resolved_path, None);
        assert_eq!(
            resolved.resolved_path,
            Some(key(&fixture.notes.join("Tessera.md")))
        );
    }

    #[test]
    fn the_two_listing_tools_name_a_note_the_same_way() {
        let fixture = fixture();
        write_note(&fixture, "Launch.md", "the rerank pass ships on Monday");
        build_index(&fixture);
        let host = host(&fixture);

        let listed = host.list_notes(&client(), None, 100).expect("list");
        let found = host.search_notes(&client(), "rerank", 50).expect("search");

        assert_eq!(listed.len(), 1);
        assert_eq!(found.len(), 1);
        assert_eq!(listed[0].path, found[0].path);
        assert_eq!(listed[0].name, found[0].name);
        assert_eq!(listed[0].name, "Launch");
    }

    #[test]
    fn note_properties_match_the_index_for_the_same_note() {
        let fixture = fixture();
        let path = write_note(
            &fixture,
            "Launch.md",
            "---\ntitle: Launch\nstatus: draft\n---\n\nthe text\n",
        );
        build_index(&fixture);

        let properties = host(&fixture)
            .note_properties(&client(), path.to_str().expect("utf-8"))
            .expect("properties");

        let index = NotesIndexStore::open_read_only(&fixture.db).expect("index");
        let expected = index.facts(&key(&path)).expect("facts").properties;

        let pairs: Vec<(String, String)> = properties
            .into_iter()
            .map(|property| (property.name, property.value))
            .collect();
        assert_eq!(pairs, expected);
        assert!(pairs.iter().any(|(name, _)| name == "title"));
    }

    #[test]
    fn note_tags_match_the_index_for_the_same_note() {
        let fixture = fixture();
        let path = write_note(&fixture, "Launch.md", "the plan #ship and #soon\n");
        build_index(&fixture);

        let tags = host(&fixture)
            .note_tags(&client(), path.to_str().expect("utf-8"))
            .expect("tags");

        let index = NotesIndexStore::open_read_only(&fixture.db).expect("index");
        let expected = index.facts(&key(&path)).expect("facts").tags;

        let pairs: Vec<(String, u32)> = tags.into_iter().map(|tag| (tag.tag, tag.line)).collect();
        assert_eq!(pairs, expected);
        assert!(pairs.iter().any(|(tag, _)| tag == "ship"));
    }

    #[test]
    fn a_note_with_no_tags_answers_with_an_empty_list_and_not_an_object() {
        let fixture = fixture();
        let path = write_note(&fixture, "Launch.md", "no tags here\n");
        build_index(&fixture);

        let tags = host(&fixture)
            .note_tags(&client(), path.to_str().expect("utf-8"))
            .expect("tags");

        assert!(tags.is_empty());
        assert_eq!(serde_json::to_string(&tags).expect("json"), "[]");
    }

    #[test]
    fn folder_tags_match_the_index() {
        let fixture = fixture();
        write_note(&fixture, "Launch.md", "the plan #ship\n");
        write_note(&fixture, "Plan.md", "later #ship\n");
        build_index(&fixture);

        let tags = host(&fixture).folder_tags(&client()).expect("tags");

        let index = NotesIndexStore::open_read_only(&fixture.db).expect("index");
        let expected = index.all_tags().expect("tags");

        let pairs: Vec<(String, usize)> =
            tags.into_iter().map(|tag| (tag.tag, tag.notes)).collect();
        assert_eq!(pairs, expected);
        assert_eq!(pairs, vec![("ship".to_string(), 2)]);
    }

    #[test]
    fn a_walk_out_of_the_folder_is_refused_and_reads_nothing() {
        let fixture = fixture();
        let outside = fixture.notes.parent().expect("parent").join("secrets.md");
        std::fs::write(&outside, "somebody else's").expect("seed");
        build_index(&fixture);
        let host = host(&fixture);

        let relative = fixture.notes.join("../secrets.md");
        let refused = host
            .read_note(&client(), relative.to_str().expect("utf-8"))
            .expect_err("refused");
        assert!(matches!(refused, ToolError::OutsideNotesFolder { .. }));

        let absolute = host
            .read_note(&client(), outside.to_str().expect("utf-8"))
            .expect_err("refused");
        assert!(matches!(absolute, ToolError::OutsideNotesFolder { .. }));

        for message in [refused.to_string(), absolute.to_string()] {
            assert!(!message.contains("somebody else's"));
        }
    }

    #[test]
    fn a_relative_path_is_read_from_the_notes_folder() {
        let fixture = fixture();
        write_note(&fixture, "Launch.md", "the text");
        write_note(&fixture, "Ideas/Later.md", "the other text");

        let host = host(&fixture);
        assert_eq!(
            host.read_note(&client(), "Launch.md").expect("read").text,
            "the text"
        );
        assert_eq!(
            host.read_note(&client(), "Ideas/Later.md")
                .expect("read")
                .text,
            "the other text"
        );
    }

    #[test]
    fn a_relative_path_reaching_out_of_the_folder_is_refused() {
        let fixture = fixture();
        let outside = fixture.notes.parent().expect("parent").join("secrets.md");
        std::fs::write(&outside, "somebody else's").expect("seed");

        let refused = host(&fixture)
            .read_note(&client(), "../secrets.md")
            .expect_err("refused");

        assert!(matches!(refused, ToolError::OutsideNotesFolder { .. }));
        assert!(!refused.to_string().contains("somebody else's"));
    }

    #[test]
    fn a_relative_path_the_folder_does_not_hold_is_reported_as_missing() {
        let fixture = fixture();

        assert!(matches!(
            host(&fixture)
                .read_note(&client(), "Never-Written.md")
                .expect_err("refused"),
            ToolError::NotFound { .. }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_pointing_out_of_the_folder_is_refused_after_resolution() {
        let fixture = fixture();
        let outside = fixture.notes.parent().expect("parent").join("secrets.md");
        std::fs::write(&outside, "somebody else's").expect("seed");
        let link = fixture.notes.join("Looks-Like-A-Note.md");
        std::os::unix::fs::symlink(&outside, &link).expect("link");

        let refused = host(&fixture)
            .read_note(&client(), link.to_str().expect("utf-8"))
            .expect_err("refused");

        assert!(matches!(refused, ToolError::OutsideNotesFolder { .. }));
        assert!(!refused.to_string().contains("somebody else's"));
    }

    #[test]
    fn a_note_over_two_megabytes_is_refused_by_size() {
        let fixture = fixture();
        let path = write_note(&fixture, "Huge.md", &"x".repeat(3 * 1024 * 1024));

        let refused = host(&fixture)
            .read_note(&client(), path.to_str().expect("utf-8"))
            .expect_err("refused");

        match refused {
            ToolError::TooLarge { bytes, .. } => assert_eq!(bytes, 3 * 1024 * 1024),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_missing_note_is_reported_as_missing() {
        let fixture = fixture();
        let path = fixture.notes.join("Never-Written.md");

        assert!(matches!(
            host(&fixture)
                .read_note(&client(), path.to_str().expect("utf-8"))
                .expect_err("refused"),
            ToolError::NotFound { .. }
        ));
    }

    #[test]
    fn without_an_index_the_folder_tools_answer_and_the_index_tools_do_not() {
        let fixture = fixture();
        let path = write_note(&fixture, "Launch.md", "the text");
        assert!(!fixture.db.exists());

        let host = host(&fixture);
        let named = path.to_str().expect("utf-8");

        assert_eq!(
            host.list_notes(&client(), None, 100).expect("list").len(),
            1
        );
        assert_eq!(
            host.read_note(&client(), named).expect("read").text,
            "the text"
        );

        assert_eq!(
            host.search_notes(&client(), "text", 50)
                .expect_err("no index"),
            ToolError::IndexUnavailable
        );
        assert_eq!(
            host.note_links(&client(), named).expect_err("no index"),
            ToolError::IndexUnavailable
        );
        assert_eq!(
            host.note_backlinks(&client(), named).expect_err("no index"),
            ToolError::IndexUnavailable
        );
        assert_eq!(
            host.note_properties(&client(), named)
                .expect_err("no index"),
            ToolError::IndexUnavailable
        );
        assert_eq!(
            host.note_tags(&client(), named).expect_err("no index"),
            ToolError::IndexUnavailable
        );
        assert_eq!(
            host.folder_tags(&client()).expect_err("no index"),
            ToolError::IndexUnavailable
        );

        // Opening the host and running every tool against a folder with no
        // index leaves the folder without one: the crate reads `writ.db` and
        // never mints it (ADR-031 rule 1.3).
        assert!(!fixture.db.exists());
    }

    #[test]
    fn an_index_at_another_schema_version_reads_as_no_index() {
        let fixture = fixture();
        write_note(&fixture, "Launch.md", "the text");
        std::fs::write(&fixture.db, "this is not a database").expect("seed");

        assert!(!host(&fixture).has_index());
    }

    #[test]
    fn with_the_server_off_every_tool_is_refused() {
        let fixture = fixture();
        let path = write_note(&fixture, "Launch.md", "the text");
        build_index(&fixture);
        let host = ToolHost::open(
            &fixture.notes,
            &fixture.db,
            Box::new(EnabledReads::new(false)),
        )
        .expect("host");
        let named = path.to_str().expect("utf-8");

        let refusals = [
            host.list_notes(&client(), None, 100).map(|_| ()),
            host.search_notes(&client(), "text", 50).map(|_| ()),
            host.read_note(&client(), named).map(|_| ()),
            host.note_links(&client(), named).map(|_| ()),
            host.note_backlinks(&client(), named).map(|_| ()),
            host.note_properties(&client(), named).map(|_| ()),
            host.note_tags(&client(), named).map(|_| ()),
            host.folder_tags(&client()).map(|_| ()),
        ];
        for (tool, refusal) in READ_TOOLS.iter().zip(refusals) {
            assert_eq!(
                refusal.expect_err("refused"),
                ToolError::NotApproved {
                    client: "Test Client".to_string(),
                    tool: tool.to_string(),
                },
            );
        }
    }

    #[test]
    fn a_deny_all_gate_refuses_a_read() {
        let fixture = fixture();
        write_note(&fixture, "Launch.md", "the text");
        let host = ToolHost::open(&fixture.notes, &fixture.db, Box::new(DenyAll)).expect("host");

        assert!(matches!(
            host.list_notes(&client(), None, 100).expect_err("refused"),
            ToolError::NotApproved { .. }
        ));
    }

    #[test]
    fn the_index_tools_are_the_read_tools_that_need_the_index() {
        for tool in INDEX_TOOLS {
            assert!(READ_TOOLS.contains(tool), "{tool}");
        }
        assert!(!INDEX_TOOLS.contains(&"list_notes"));
        assert!(!INDEX_TOOLS.contains(&"read_note"));
    }

    #[test]
    fn opening_a_folder_that_is_not_there_fails_rather_than_creating_one() {
        let fixture = fixture();
        let missing = fixture.notes.join("nowhere");

        assert!(matches!(
            ToolHost::open(&missing, &fixture.db, Box::new(DenyAll)).expect_err("no folder"),
            ToolError::NotFound { .. }
        ));
        assert!(!missing.exists());
    }

    #[test]
    fn opening_an_index_that_is_not_there_creates_no_database() {
        let fixture = fixture();
        let host = host(&fixture);

        assert!(!host.has_index());
        assert!(!fixture.db.exists());
    }
}
