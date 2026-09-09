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
//! The three write tools change a note's file and nothing else. They write
//! through `writ_storage::guarded`, the one writer of a note's file
//! (ADR-032 section 4), under `WriteOrigin::Mcp` and
//! `ConflictPolicy::RefuseWithCopy`, so a note that changed since the client
//! read it keeps what it holds and the client's text lands beside it. There is
//! no argument that turns that into an overwrite.
//!
//! Nothing here stamps the app's ignore set, and that is load-bearing rather
//! than a gap. The stamp is how Writ tells its own writes apart from somebody
//! else's; a write from this process **is** somebody else's, and the running
//! app is meant to learn about it through the folder watcher and reconcile the
//! open tab (ADR-033). A stamped write would be swallowed. The `BeforeWrite`
//! hook is therefore `None` on every call this module makes, which is also the
//! only thing it could be: the ignore set lives in the app process and this
//! one is the client's child.
//!
//! The method bodies are shaped for U9 to lift onto `writ_plugin::host::NoteHost`
//! (ADR-032 section 3): the consent check is the first line and the rest of the
//! body is the operation, so the check can be replaced by a capability check
//! without the operation moving.

use std::path::{Path, PathBuf};

use writ_core::activity::{ActivityRecord, Actor};
use writ_core::hash::{digest_from_hex, digest_hex};
use writ_core::notes::containment::{resolve_for_containment, resolve_inside};
use writ_core::notes::guard::{is_not_downloaded, DiskState};
use writ_core::notes::WriteOrigin;
use writ_storage::buffer_store::{dataless_flags, read_disk_state};
use writ_storage::database::migrations::binary_schema_version;
use writ_storage::errors::StorageError;
use writ_storage::guarded::{
    create_note_guarded, write_note_guarded, ConflictPolicy, CreateNote, DiskRead, GuardedWrite,
    TakenName,
};
use writ_storage::notes_index::{self, BacklinkCertainty, NotesIndexStore};

use crate::consent::{ClientId, ConsentGate, Decision};

/// Largest note a tool reads, in bytes (ADR-031 rule 4.8).
pub const MAX_NOTE_BYTES: u64 = 2 * 1024 * 1024;

/// Most notes or hits one call answers with, whatever the caller asked for.
pub const MAX_RESULTS: usize = 500;

/// The extension `list_notes` counts as a note.
const NOTE_EXTENSION: &str = "md";

/// The tools that only read, and the tools that change a note.
///
/// Both lists live in `writ-core` and are re-exported here: the server
/// registers them, the gate reads the split, and the settings row shows the
/// user the same names, so none of the three can drift
/// ([`writ_core::tools`]).
pub use writ_core::tools::{READ_TOOLS, WRITE_TOOLS};

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
    #[error("{client} is not approved. Approve it in Writ's settings, under Connected programs.")]
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
    /// The text handed in is over [`MAX_NOTE_BYTES`].
    #[error("That is {bytes} bytes. A tool writes up to {MAX_NOTE_BYTES} bytes.")]
    TooMuchText {
        /// How many bytes the client sent.
        bytes: u64,
    },
    /// The note holds something other than what the client read, so the write
    /// was not made. The client's text is beside the note.
    #[error("{path} changed on disk after it was read, and was left as it is.{}",
        .conflict_copy.as_ref().map(|copy| format!(" What you sent is at {copy}.")).unwrap_or_default())]
    Conflict {
        /// The path as the client wrote it.
        path: String,
        /// The dated copy the client's text was written to, when one could be
        /// written.
        conflict_copy: Option<String>,
    },
    /// `expected_hash` is not the 64 hex characters a read hands back.
    #[error("expected_hash for {path} has to be the hash read_note returned.")]
    HashNotUnderstood {
        /// The path as the client wrote it.
        path: String,
    },
    /// The name handed in holds nothing a file can be called.
    #[error("{}", writ_core::notes::NAME_IS_EMPTY)]
    NameEmpty,
    /// A note of that name is already in the folder.
    #[error("{}", writ_core::notes::name_is_taken(name))]
    NameTaken {
        /// The name as the folder would spell it.
        name: String,
    },
    /// The file's bytes are not on this machine yet.
    #[error("{path} has not finished downloading to this machine.")]
    NotDownloaded {
        /// The path as the client wrote it.
        path: String,
    },
    /// The file is there and this process could not write it.
    #[error("{path} could not be written.")]
    Unwritable {
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
    /// SHA-256 of the file's bytes, in lowercase hex.
    ///
    /// This is what `write_note` takes as `expected_hash`: a client that reads
    /// a note, thinks, and writes it back hands this value over and the write
    /// is made only if the note still holds the text this hash names.
    pub hash: String,
    /// The whole file, frontmatter included.
    pub text: String,
}

/// Where a write landed and what the file holds afterwards.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WriteReceipt {
    /// The note's path, in the spelling every other tool takes back.
    pub path: String,
    /// The file's length in bytes.
    pub bytes: u64,
    /// SHA-256 of the file's bytes, the value the next write passes as
    /// `expected_hash`.
    pub hash: String,
}

/// Where a renamed note went, and where it was.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RenameReceipt {
    /// The note's path now.
    pub path: String,
    /// The path it had before.
    pub previous_path: String,
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
    writ_dir: PathBuf,
    index: Option<NotesIndexStore>,
    gate: Box<dyn ConsentGate>,
}

impl std::fmt::Debug for ToolHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolHost")
            .field("notes_root", &self.notes_root)
            .field("writ_dir", &self.writ_dir)
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
        writ_dir: &Path,
        gate: Box<dyn ConsentGate>,
    ) -> Result<Self, ToolError> {
        let resolved = resolve_for_containment(notes_root)
            .filter(|root| root.is_dir())
            .ok_or_else(|| ToolError::NotFound {
                path: notes_root.display().to_string(),
            })?;
        Ok(Self {
            notes_root: resolved,
            writ_dir: writ_dir.to_path_buf(),
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
            hash: writ_core::hash::sha256_hex(text.as_bytes()),
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
    /// Replaces the text of the note at `path`.
    ///
    /// `expected_hash` is the `hash` [`ToolHost::read_note`] handed back. Given
    /// it, the write is made only while the note still holds that text: a note
    /// somebody edited in between keeps what it holds, the text handed in is
    /// put beside it as a dated copy, and [`ToolError::Conflict`] names the
    /// copy (ADR-028 section 5). Omitted, the write is made against whatever
    /// the file holds now, and keeping a stale read from landing on a newer
    /// note is then the client's own business. There is no third option: no
    /// argument to this method overwrites a note the guard held back.
    ///
    /// The bytes land as they were handed in. Nothing reflows the file, so
    /// frontmatter comes back out the way it went in. Text the note already
    /// holds is not written again: the receipt says what the file holds and
    /// the modification time does not move, which is what keeps a sync client
    /// from uploading a change nobody made.
    pub fn write_note(
        &self,
        client: &ClientId,
        path: &str,
        content: &str,
        expected_hash: Option<&str>,
    ) -> Result<WriteReceipt, ToolError> {
        let allowed = self.allow(client, "write_note");
        let written =
            allowed.and_then(|()| self.replace_text(client, path, content, expected_hash));
        self.record(
            client,
            "write_note",
            &self.logged_path(
                written
                    .as_ref()
                    .map_or(path, |receipt| receipt.path.as_str()),
            ),
            Some(content.len() as u64),
            decision_of(&written),
        );
        written
    }

    /// Mints a note called `name` in the notes folder.
    ///
    /// The name is sanitised into a filename the way every other surface
    /// sanitises one ([`writ_core::notes::sanitize_title`]), so a name that
    /// spells a path is a name and not a path. A note of that name already in
    /// the folder is answered with [`ToolError::NameTaken`] and left alone.
    /// Already in the folder is the facade's reading of it, which folds a name
    /// to NFC and lowercase, so `launch.md` holds the name `Launch` on a
    /// volume that tells the two apart as readily as on one that does not.
    ///
    /// A minted file is LF, whatever the text handed in carries. That is the
    /// one place a write tool does not land the bytes verbatim, and it applies
    /// only to a file that has no line-ending convention yet because it has
    /// never existed.
    pub fn create_note(
        &self,
        client: &ClientId,
        name: &str,
        content: &str,
    ) -> Result<WriteReceipt, ToolError> {
        let allowed = self.allow(client, "create_note");
        let created = allowed.and_then(|()| self.mint_note(client, name, content));
        self.record(
            client,
            "create_note",
            &created.as_ref().map_or_else(
                |_| minted_slug(name),
                |receipt| self.logged_path(&receipt.path),
            ),
            Some(content.len() as u64),
            decision_of(&created),
        );
        created
    }

    /// Renames the note at `path` to `new_name`, inside the folder it is in.
    ///
    /// **No link in any other note is rewritten.** A rename that updates the
    /// links pointing at a note is an offer made to the user, with the count
    /// in front of them and a way back afterwards; a bulk rewrite of a folder
    /// on a program's say-so is the thing that offer exists to prevent
    /// (ADR-031 rule 4.7). A client that renames a note leaves the links that
    /// named it pointing at the old name.
    ///
    /// The rename goes through the same guard a write does, so a note whose
    /// bytes are not on this machine is left where it is rather than pulled
    /// down.
    pub fn rename_note(
        &self,
        client: &ClientId,
        path: &str,
        new_name: &str,
    ) -> Result<RenameReceipt, ToolError> {
        let allowed = self.allow(client, "rename_note");
        let renamed = allowed.and_then(|()| self.move_name(client, path, new_name));
        self.record(
            client,
            "rename_note",
            &self.logged_path(
                renamed
                    .as_ref()
                    .map_or(path, |(receipt, _)| receipt.path.as_str()),
            ),
            renamed.as_ref().ok().map(|(_, bytes)| *bytes),
            decision_of(&renamed),
        );
        renamed.map(|(receipt, _)| receipt)
    }

    /// [`ToolHost::write_note`] past the gate.
    fn replace_text(
        &self,
        client: &ClientId,
        path: &str,
        content: &str,
        expected_hash: Option<&str>,
    ) -> Result<WriteReceipt, ToolError> {
        self.text_fits(content)?;
        let file = self.note_file(path)?;
        // Asked before the read below, because the read is what would pull an
        // evicted file down (ADR-028 section 5). The guard asks the same
        // question, and asking it here is what lets the state below be read
        // once and handed over rather than read again inside it.
        if is_not_downloaded(dataless_flags(&file)) {
            return Err(ToolError::NotDownloaded {
                path: path.to_string(),
            });
        }
        let on_disk = read_disk_state(&file).map_err(|_| ToolError::Unreadable {
            path: path.to_string(),
        })?;
        // With a hash from the client, only its digest is read out of this:
        // `decide_save` compares digests and never the length or the
        // modification time, and a client that read the note over the wire
        // knows neither of those two about the file it read. Without one, what
        // the file holds now stands in, which is the same thing as having no
        // expectation and also lets a write of the text the note already holds
        // be recognised and skipped.
        let last_known = match expected_hash {
            Some(hex) => Some(DiskState {
                hash: digest_from_hex(hex).ok_or_else(|| ToolError::HashNotUnderstood {
                    path: path.to_string(),
                })?,
                size: 0,
                mtime: None,
            }),
            None => on_disk,
        };
        let outcome = write_note_guarded(
            GuardedWrite {
                target: &file,
                bytes: content.as_bytes(),
                last_known,
                // Read once, above, from the same bytes the digests here
                // describe.
                on_disk: DiskRead::Read(on_disk),
                dataless: None,
                origin: self.origin(client),
                on_conflict: ConflictPolicy::RefuseWithCopy,
                history: None,
            },
            // No ignore stamp: this write is meant to reach the running app as
            // somebody else's, through the folder watcher (ADR-033).
            None,
        )
        .map_err(|error| write_error(path, error))?;
        Ok(WriteReceipt {
            path: notes_index::index_key(&file),
            bytes: outcome.disk_state.size,
            hash: digest_hex(outcome.disk_state.hash),
        })
    }

    /// [`ToolHost::create_note`] past the gate.
    fn mint_note(
        &self,
        client: &ClientId,
        name: &str,
        content: &str,
    ) -> Result<WriteReceipt, ToolError> {
        self.text_fits(content)?;
        let stem = writ_core::notes::sanitize_title(name).ok_or(ToolError::NameEmpty)?;
        // The dedupe is what a person who asked for a new note wants and the
        // wrong answer for a program: a client that asked for `Launch` and got
        // `Launch 2` has put its text in a note it did not name. The facade
        // holds the folding rule for what "taken" means, so it is asked rather
        // than second-guessed: a check here against the exact path would be
        // the filesystem's answer, and a case-sensitive volume folds nothing.
        let minted = create_note_guarded(
            CreateNote {
                notes_root: &self.notes_root,
                stem: &stem,
                content,
                origin: self.origin(client),
                on_taken_name: TakenName::Refuse,
                history: None,
            },
            None,
        )
        .map_err(|error| write_error(name, error))?;
        // Read back rather than hashed here: the facade lands a minted note as
        // LF, so what the client handed in is not always what the file holds.
        let state =
            read_disk_state(&minted)
                .ok()
                .flatten()
                .ok_or_else(|| ToolError::Unwritable {
                    path: name.to_string(),
                })?;
        Ok(WriteReceipt {
            path: notes_index::index_key(&minted),
            bytes: state.size,
            hash: digest_hex(state.hash),
        })
    }

    /// [`ToolHost::rename_note`] past the gate, with the file's length for the
    /// record.
    fn move_name(
        &self,
        client: &ClientId,
        path: &str,
        new_name: &str,
    ) -> Result<(RenameReceipt, u64), ToolError> {
        let file = self.note_file(path)?;
        // Maps a separator to a space, so a new name that spells a path names
        // a file in the folder the note is already in and cannot walk out of
        // it.
        let stem = writ_core::notes::rename_stem(&file, new_name).ok_or(ToolError::NameEmpty)?;
        // Asked before the read below, because the read is what would pull an
        // evicted file down (ADR-028 section 5).
        if is_not_downloaded(dataless_flags(&file)) {
            return Err(ToolError::NotDownloaded {
                path: path.to_string(),
            });
        }
        let last_known = read_disk_state(&file).map_err(|_| ToolError::Unreadable {
            path: path.to_string(),
        })?;
        let bytes = last_known.map_or(0, |state| state.size);
        let moved = writ_storage::note_ops::rename_note(
            &file,
            &stem,
            last_known,
            self.origin(client),
            // No ignore stamp, for the reason `replace_text` gives.
            None,
        )
        .map_err(|error| write_error(path, error))?;
        Ok((
            RenameReceipt {
                path: notes_index::index_key(&moved),
                previous_path: notes_index::index_key(&file),
            },
            bytes,
        ))
    }

    /// The origin every write from this host carries.
    fn origin(&self, client: &ClientId) -> WriteOrigin {
        WriteOrigin::Mcp {
            client: client.name.clone(),
        }
    }

    /// Holds an incoming text to the same ceiling a read is held to.
    fn text_fits(&self, content: &str) -> Result<(), ToolError> {
        let bytes = content.len() as u64;
        if bytes > MAX_NOTE_BYTES {
            return Err(ToolError::TooMuchText { bytes });
        }
        Ok(())
    }

    /// The spelling of a note the activity log takes: relative to the notes
    /// folder, forward slashes, whichever way the call went.
    ///
    /// The user reads one list, so a note is spelled one way in it. An allowed
    /// write naming `Projects/Writ.md` beside a refused one naming
    /// `/Users/…/Notes/Projects/Writ.md` is the same note written twice as far
    /// as anybody reading can tell. It also keeps the folder this machine
    /// keeps its notes in out of a file the user may hand to somebody.
    ///
    /// Pure, and it stays pure: a refused call opens no file, so nothing here
    /// resolves, stats or lists anything. A path the folder does not hold is
    /// left as the client wrote it, because there is no note to name.
    fn logged_path(&self, path: &str) -> String {
        let given = Path::new(path);
        let candidate = if given.is_absolute() {
            given.to_path_buf()
        } else {
            self.notes_root.join(given)
        };
        relative_slug(&self.notes_root, &candidate).unwrap_or_else(|| path.to_string())
    }

    /// Appends what one write did to the activity log.
    ///
    /// The gate has already recorded the call it decided on, naming the client
    /// and the tool. This is the line that also names the note and the length,
    /// which the gate never sees. A data folder that cannot be written to is
    /// not a reason to answer differently: the write already happened or
    /// already did not.
    fn record(
        &self,
        client: &ClientId,
        tool: &str,
        path: &str,
        bytes: Option<u64>,
        decision: Decision,
    ) {
        let mut record = ActivityRecord::now(Actor::from(client), tool, decision).with_path(path);
        if let Some(bytes) = bytes {
            record = record.with_bytes(bytes);
        }
        let _ = writ_storage::activity_log::append(&self.writ_dir, &record);
    }

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

/// What the log says about a call that reached the operation.
fn decision_of<T>(result: &Result<T, ToolError>) -> Decision {
    match result {
        Ok(_) => Decision::Allow,
        Err(_) => Decision::Refuse,
    }
}

/// The note a `create_note` call names, in the log's spelling.
///
/// A refused call has no minted file to name, so the name is spelled the way
/// the file would have been: the log says `Ship it.md` whether the note was
/// minted or turned down, rather than a path one time and a bare name the
/// next. A name that sanitises to nothing is left as it was written.
fn minted_slug(name: &str) -> String {
    writ_core::notes::sanitize_title(name)
        .map(|stem| format!("{stem}.{NOTE_EXTENSION}"))
        .unwrap_or_else(|| name.to_string())
}

/// A storage refusal as the tool's own, naming the path the client wrote.
///
/// Every message a client sees names a path, a name or a length. The digest
/// the guard carries and the folder it names are the app's own spellings of
/// this machine, and neither is in the answer.
fn write_error(path: &str, error: StorageError) -> ToolError {
    match error {
        StorageError::SourceChangedOnDisk { conflict_copy, .. } => ToolError::Conflict {
            path: path.to_string(),
            conflict_copy,
        },
        StorageError::SourceNotDownloaded { .. } => ToolError::NotDownloaded {
            path: path.to_string(),
        },
        StorageError::NoteNameEmpty => ToolError::NameEmpty,
        StorageError::NoteNameTaken { name, .. } => ToolError::NameTaken { name },
        _ => ToolError::Unwritable {
            path: path.to_string(),
        },
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

    /// A notes folder, the data folder beside it, and the path the index would
    /// live at.
    struct Fixture {
        _dir: TempDir,
        notes: PathBuf,
        db: PathBuf,
        writ: PathBuf,
    }

    fn fixture() -> Fixture {
        let dir = TempDir::new().expect("temp dir");
        let notes = dir.path().join("notes");
        std::fs::create_dir_all(&notes).expect("notes folder");
        Fixture {
            db: dir.path().join("writ.db"),
            writ: dir.path().to_path_buf(),
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
            &fixture.writ,
            Box::new(EnabledReads::new(true)),
        )
        .expect("host")
    }

    /// A host on the gate production runs, over a folder whose settings grant
    /// the test client `read` and `write` as stated.
    ///
    /// The real gate rather than a fixture, because the write half's own tests
    /// are about what the user granted and about what the log then says, and a
    /// double that grants everything and records nothing answers neither.
    fn approved_host(fixture: &Fixture, read: bool, write: bool) -> ToolHost {
        std::fs::write(
            fixture.writ.join("config.toml"),
            format!(
                "[mcp]\nenabled = true\n\n[[mcp.approved_clients]]\nname = \"Test Client\"\nread = {read}\nwrite = {write}\n"
            ),
        )
        .expect("seed the settings file");
        ToolHost::open(
            &fixture.notes,
            &fixture.db,
            &fixture.writ,
            Box::new(crate::consent::ConfigGate::new(&fixture.writ)),
        )
        .expect("host")
    }

    /// Every activity record the folder holds, newest first.
    fn records(fixture: &Fixture) -> Vec<writ_core::activity::ActivityRecord> {
        writ_storage::activity_log::read_recent(&fixture.writ, 100)
    }

    /// The records a tool wrote, which are the ones naming a note.
    ///
    /// Two records land for one write call: the gate writes the client, the
    /// tool and the decision for every call it decides on, and the tool then
    /// writes the fuller line, naming the note and the length the gate never
    /// sees. That is U5's shape, not this unit's; a test that wants both lines
    /// reads [`records`]. This is the tool's own half.
    fn records_a_tool_wrote(fixture: &Fixture) -> Vec<writ_core::activity::ActivityRecord> {
        records(fixture)
            .into_iter()
            .filter(|record| record.path.is_some())
            .collect()
    }

    fn client() -> ClientId {
        ClientId::named("Test Client")
    }

    /// How many notes the folder holds, top level.
    fn notes_in(fixture: &Fixture) -> usize {
        std::fs::read_dir(&fixture.notes)
            .expect("read the folder")
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "md"))
            .count()
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
    fn the_refusal_names_the_client_and_where_it_is_approved() {
        let message = ToolError::NotApproved {
            client: "Claude Code".to_string(),
            tool: "read_note".to_string(),
        }
        .to_string();

        assert!(message.contains("Claude Code"));
        assert!(message.contains("Connected programs"));
        // Direction-neutral: the same refusal answers a write tool.
        assert!(!message.contains("read"));
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
            &fixture.writ,
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
        let host = ToolHost::open(
            &fixture.notes,
            &fixture.db,
            &fixture.writ,
            Box::new(DenyAll),
        )
        .expect("host");

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
            ToolHost::open(&missing, &fixture.db, &fixture.writ, Box::new(DenyAll))
                .expect_err("no folder"),
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

    // The write half. Every test below names one acceptance criterion of the
    // unit that added the three write tools.

    #[test]
    fn a_write_from_an_approved_client_lands_byte_exactly_and_returns_the_new_hash() {
        let fixture = fixture();
        let note = write_note(&fixture, "Launch.md", "before\n");
        let host = approved_host(&fixture, true, true);
        let text = "after\r\nwith its own line endings\r\n";

        let receipt = host
            .write_note(&client(), "Launch.md", text, None)
            .expect("the write is made");

        assert_eq!(std::fs::read(&note).expect("read back"), text.as_bytes());
        assert_eq!(receipt.path, key(&note));
        assert_eq!(receipt.bytes, text.len() as u64);
        assert_eq!(receipt.hash, writ_core::hash::sha256_hex(text.as_bytes()));
    }

    #[test]
    fn a_write_from_a_read_only_client_is_not_made_and_one_record_says_so() {
        let fixture = fixture();
        let note = write_note(&fixture, "Launch.md", "before\n");
        let host = approved_host(&fixture, true, false);
        let text = "after\n";

        let refusal = host
            .write_note(&client(), "Launch.md", text, None)
            .expect_err("a client approved to read does not write");

        assert!(matches!(refusal, ToolError::NotApproved { .. }));
        assert_eq!(std::fs::read(&note).expect("read back"), b"before\n");
        let named = records_a_tool_wrote(&fixture);
        assert_eq!(named.len(), 1, "one record names the note: {named:?}");
        assert_eq!(named[0].decision, Decision::Refuse);
        assert_eq!(named[0].action, "write_note");
        assert_eq!(named[0].bytes, Some(text.len() as u64));
    }

    #[test]
    fn a_write_against_a_note_changed_underneath_is_not_made_and_the_text_lands_beside_it() {
        let fixture = fixture();
        let note = write_note(&fixture, "Launch.md", "as it was read\n");
        let host = approved_host(&fixture, true, true);
        let read = host.read_note(&client(), "Launch.md").expect("read");
        std::fs::write(&note, "as somebody else left it\n").expect("edit underneath");

        let refusal = host
            .write_note(
                &client(),
                "Launch.md",
                "what the client sent\n",
                Some(&read.hash),
            )
            .expect_err("a note changed underneath is left alone");

        let ToolError::Conflict { conflict_copy, .. } = refusal else {
            panic!("expected a conflict, got {refusal:?}");
        };
        let copy = PathBuf::from(conflict_copy.expect("the text is kept beside the note"));
        assert!(copy.is_file(), "{} is not there", copy.display());
        assert_eq!(
            std::fs::read_to_string(&copy).expect("read the copy"),
            "what the client sent\n"
        );
        assert_eq!(
            std::fs::read_to_string(&note).expect("read the note"),
            "as somebody else left it\n"
        );
    }

    #[test]
    fn a_write_to_a_path_outside_the_notes_folder_opens_nothing() {
        let fixture = fixture();
        let outside = fixture.writ.join("outside.md");
        std::fs::write(&outside, "not a note of this folder\n").expect("seed");
        let host = approved_host(&fixture, true, true);

        let refusal = host
            .write_note(&client(), "../outside.md", "overwritten\n", None)
            .expect_err("a path out of the folder is not written");

        assert!(matches!(refusal, ToolError::OutsideNotesFolder { .. }));
        assert_eq!(
            std::fs::read_to_string(&outside).expect("read back"),
            "not a note of this folder\n"
        );
        for entry in std::fs::read_dir(&fixture.writ).expect("read the folder") {
            let name = entry.expect("an entry").file_name();
            assert!(
                !name.to_string_lossy().contains("conflict"),
                "{name:?} is a copy of what was sent, so the path was opened before it was turned down"
            );
        }
    }

    #[test]
    fn create_note_with_a_taken_name_leaves_the_note_that_is_there_alone() {
        let fixture = fixture();
        let note = write_note(&fixture, "Launch.md", "the note that is there\n");
        let host = approved_host(&fixture, true, true);

        let refusal = host
            .create_note(&client(), "Launch", "the note a client asked for\n")
            .expect_err("a taken name is answered rather than deduped");

        assert!(
            matches!(&refusal, ToolError::NameTaken { name } if name == "Launch.md"),
            "{refusal:?}"
        );
        assert_eq!(
            std::fs::read_to_string(&note).expect("read back"),
            "the note that is there\n"
        );
        assert!(
            !fixture.notes.join("Launch 2.md").exists(),
            "nothing is minted under a name the client did not ask for"
        );
    }

    #[test]
    fn create_note_is_refused_when_the_folder_holds_the_name_in_another_case() {
        // The exact path is the filesystem's answer to "is this taken", and a
        // case-sensitive volume answers no. The folder's own answer folds, and
        // that is the one the note is minted under, so it is the one asked.
        let fixture = fixture();
        let note = write_note(&fixture, "launch.md", "the note that is there\n");
        let host = approved_host(&fixture, true, true);

        let refusal = host
            .create_note(&client(), "Launch", "the note a client asked for\n")
            .expect_err("a name the folder holds in another case is taken");

        assert!(
            matches!(&refusal, ToolError::NameTaken { name } if name == "Launch.md"),
            "{refusal:?}"
        );
        assert_eq!(
            std::fs::read_to_string(&note).expect("read back"),
            "the note that is there\n"
        );
        assert_eq!(notes_in(&fixture), 1, "a second note was minted");
    }

    #[test]
    fn create_note_is_refused_when_the_folder_holds_the_name_spelled_another_way() {
        // The same name written with a combining accent rather than a single
        // character. The fold the mint runs reads them as one name.
        let fixture = fixture();
        let note = write_note(&fixture, "Cafe\u{301}.md", "the note that is there\n");
        let host = approved_host(&fixture, true, true);

        let refusal = host
            .create_note(&client(), "Caf\u{e9}", "the note a client asked for\n")
            .expect_err("a name the folder holds another spelling of is taken");

        // The name in the error is the one the client asked for, spelled the
        // way it asked for it, and it is a file name rather than a path:
        // `name_is_taken` puts it in front of a reader.
        assert!(
            matches!(&refusal, ToolError::NameTaken { name } if name == "Caf\u{e9}.md"),
            "{refusal:?}"
        );
        assert_eq!(
            std::fs::read_to_string(&note).expect("read back"),
            "the note that is there\n"
        );
        assert_eq!(notes_in(&fixture), 1, "a second note was minted");
    }

    #[test]
    fn create_note_mints_the_note_and_answers_with_where_it_went() {
        let fixture = fixture();
        let host = approved_host(&fixture, true, true);

        let receipt = host
            .create_note(&client(), "Ship it", "# Ship it\n")
            .expect("the note is minted");

        let minted = fixture.notes.join("Ship it.md");
        assert!(minted.is_file());
        assert_eq!(receipt.path, key(&minted));
        assert_eq!(
            std::fs::read_to_string(&minted).expect("read back"),
            "# Ship it\n"
        );
        assert_eq!(receipt.bytes, "# Ship it\n".len() as u64);
    }

    #[test]
    fn a_minted_note_is_lf_whatever_the_client_sent() {
        let fixture = fixture();
        let host = approved_host(&fixture, true, true);

        let receipt = host
            .create_note(&client(), "Ship it", "one\r\ntwo\r\n")
            .expect("minted");

        let minted = fixture.notes.join("Ship it.md");
        assert_eq!(
            std::fs::read(&minted).expect("read back"),
            b"one\ntwo\n",
            "a file that never existed has no line-ending convention to keep"
        );
        assert_eq!(receipt.hash, writ_core::hash::sha256_hex(b"one\ntwo\n"));
        assert_eq!(receipt.bytes, 8);
    }

    #[test]
    fn a_name_that_spells_a_path_names_a_note_in_the_folder() {
        let fixture = fixture();
        let host = approved_host(&fixture, true, true);

        let receipt = host
            .create_note(&client(), "../../escaped", "text\n")
            .expect("minted");

        assert!(receipt.path.starts_with(&key(&fixture.notes)));
        assert!(!fixture.writ.join("escaped.md").exists());
    }

    #[test]
    fn rename_note_moves_the_file_and_rewrites_no_link_in_any_other_note() {
        let fixture = fixture();
        let note = write_note(&fixture, "Launch.md", "# Launch\n");
        let linking = write_note(&fixture, "Index.md", "see [[Launch]] for the date\n");
        let before = std::fs::read(&linking).expect("read the linking note");
        let host = approved_host(&fixture, true, true);

        let receipt = host
            .rename_note(&client(), "Launch.md", "Ship")
            .expect("the note is renamed");

        let moved = fixture.notes.join("Ship.md");
        assert!(moved.is_file());
        assert!(!note.exists());
        assert_eq!(receipt.path, key(&moved));
        assert_eq!(receipt.previous_path, key(&note));
        assert_eq!(
            std::fs::read(&linking).expect("read it back"),
            before,
            "a client's rename rewrites no link in any other note"
        );
    }

    #[test]
    fn a_rename_to_a_name_the_folder_holds_leaves_both_notes_alone() {
        let fixture = fixture();
        let note = write_note(&fixture, "Launch.md", "# Launch\n");
        let taken = write_note(&fixture, "Ship.md", "# Ship\n");
        let host = approved_host(&fixture, true, true);

        let refusal = host
            .rename_note(&client(), "Launch.md", "Ship")
            .expect_err("a taken name is not written over");

        // A rename builds its own error, so it is worth saying here too that
        // what reaches `name_is_taken` is a file name and not a path.
        assert!(
            matches!(&refusal, ToolError::NameTaken { name } if name == "Ship.md"),
            "{refusal:?}"
        );
        assert_eq!(std::fs::read_to_string(&note).expect("read"), "# Launch\n");
        assert_eq!(std::fs::read_to_string(&taken).expect("read"), "# Ship\n");
    }

    #[test]
    fn a_new_name_that_spells_a_path_renames_inside_the_folder() {
        let fixture = fixture();
        write_note(&fixture, "Launch.md", "# Launch\n");
        let host = approved_host(&fixture, true, true);

        let receipt = host
            .rename_note(&client(), "Launch.md", "../../escaped")
            .expect("renamed");

        assert!(
            receipt.path.starts_with(&key(&fixture.notes)),
            "{receipt:?}"
        );
        assert!(!fixture.writ.join("escaped.md").exists());
    }

    #[test]
    fn frontmatter_round_trips_unchanged_through_write_note() {
        let fixture = fixture();
        let text = "---\ntitle:  Launch\ntags: [a,  b]\n# a comment\n---\n\nbody\n";
        let note = write_note(&fixture, "Launch.md", text);
        let host = approved_host(&fixture, true, true);

        let read = host.read_note(&client(), "Launch.md").expect("read");
        let edited = read.text.replace("body\n", "body, one word longer\n");
        host.write_note(&client(), "Launch.md", &edited, Some(&read.hash))
            .expect("the write is made");

        let after = std::fs::read_to_string(&note).expect("read back");
        let (frontmatter, _) = after.split_once("\n---\n").expect("a closing marker");
        assert_eq!(
            frontmatter,
            "---\ntitle:  Launch\ntags: [a,  b]\n# a comment"
        );
        assert!(after.ends_with("body, one word longer\n"));
    }

    #[test]
    fn the_hash_read_note_returns_is_the_one_write_note_takes() {
        let fixture = fixture();
        write_note(&fixture, "Launch.md", "before\n");
        let host = approved_host(&fixture, true, true);

        let read = host.read_note(&client(), "Launch.md").expect("read");
        let receipt = host
            .write_note(&client(), "Launch.md", "after\n", Some(&read.hash))
            .expect("the hash a read handed back is accepted");

        let again = host.read_note(&client(), "Launch.md").expect("read again");
        assert_eq!(again.hash, receipt.hash);
    }

    #[test]
    fn an_expected_hash_that_is_not_a_hash_is_answered_and_writes_nothing() {
        let fixture = fixture();
        let note = write_note(&fixture, "Launch.md", "before\n");
        let host = approved_host(&fixture, true, true);

        let refusal = host
            .write_note(&client(), "Launch.md", "after\n", Some("not a hash"))
            .expect_err("a hash that cannot be read is not treated as no hash at all");

        assert!(matches!(refusal, ToolError::HashNotUnderstood { .. }));
        assert_eq!(
            std::fs::read_to_string(&note).expect("read back"),
            "before\n"
        );
    }

    #[test]
    fn a_write_of_the_text_the_note_already_holds_answers_with_the_hash_it_has() {
        let fixture = fixture();
        let note = write_note(&fixture, "Launch.md", "the same text\n");
        let before = std::fs::metadata(&note).expect("metadata").modified().ok();
        let host = approved_host(&fixture, true, true);

        let receipt = host
            .write_note(&client(), "Launch.md", "the same text\n", None)
            .expect("identical text is not a conflict");

        assert_eq!(
            receipt.hash,
            writ_core::hash::sha256_hex(b"the same text\n")
        );
        assert_eq!(
            std::fs::metadata(&note).expect("metadata").modified().ok(),
            before,
            "nothing is rewritten, so a sync client has nothing to upload"
        );
    }

    #[test]
    fn a_write_that_lands_appends_one_record_naming_the_note_and_its_length() {
        let fixture = fixture();
        write_note(&fixture, "Launch.md", "before\n");
        let host = approved_host(&fixture, true, true);

        host.write_note(&client(), "Launch.md", "after\n", None)
            .expect("the write is made");

        let named = records_a_tool_wrote(&fixture);
        assert_eq!(named.len(), 1, "{named:?}");
        assert_eq!(named[0].action, "write_note");
        assert_eq!(named[0].decision, Decision::Allow);
        assert_eq!(named[0].path.as_deref(), Some(Path::new("Launch.md")));
        assert_eq!(named[0].bytes, Some("after\n".len() as u64));
    }

    #[test]
    fn the_log_spells_a_note_the_same_way_whichever_way_the_call_went() {
        let fixture = fixture();
        write_note(&fixture, "Projects/Writ.md", "before\n");
        // The gate reads one approvals file, so the refused calls are made
        // first and the approval is granted after them.
        let read_only = approved_host(&fixture, true, false);
        read_only
            .write_note(&client(), "Projects/Writ.md", "after\n", None)
            .expect_err("not approved to write");
        read_only
            .create_note(&client(), "Ship it", "text\n")
            .expect_err("not approved to write");

        let allowed = approved_host(&fixture, true, true);
        allowed
            .write_note(&client(), "Projects/Writ.md", "after\n", None)
            .expect("write");
        allowed
            .create_note(&client(), "Ship it", "text\n")
            .expect("create");
        allowed
            .rename_note(&client(), "Projects/Writ.md", "Landed")
            .expect("rename");

        let mut spelled: Vec<String> = records_a_tool_wrote(&fixture)
            .into_iter()
            .filter_map(|record| Some(record.path?.to_string_lossy().into_owned()))
            .collect();
        spelled.reverse();
        assert_eq!(
            spelled,
            vec![
                "Projects/Writ.md",
                "Ship it.md",
                "Projects/Writ.md",
                "Ship it.md",
                "Projects/Landed.md",
            ],
            "a note is spelled one way in the log, folder-relative, allowed or not"
        );
    }

    #[test]
    fn one_write_call_leaves_the_gates_line_and_the_tools_line() {
        // U5's shape, spelled out rather than filtered: the gate records every
        // call it decides on, and the tool then records the note and the
        // length the gate never sees. A test that reads only the tool's half
        // reads `records_a_tool_wrote`.
        let fixture = fixture();
        let note = write_note(&fixture, "Launch.md", "before\n");
        let host = approved_host(&fixture, true, true);

        host.write_note(&client(), "Launch.md", "after\n", None)
            .expect("write");

        let all = records(&fixture);
        assert_eq!(all.len(), 2, "{all:?}");
        assert_eq!(all[0].action, "write_note");
        assert_eq!(all[0].decision, Decision::Allow);
        assert_eq!(all[0].path.as_deref(), Some(Path::new("Launch.md")));
        assert_eq!(all[0].bytes, Some("after\n".len() as u64));
        assert_eq!(all[1].action, "write_note");
        assert_eq!(all[1].decision, Decision::Allow);
        assert_eq!(all[1].path, None, "the gate never sees the note");
        assert_eq!(all[1].bytes, None);
        assert_eq!(std::fs::read_to_string(&note).expect("read"), "after\n");
    }

    #[test]
    fn every_write_tool_records_what_it_did() {
        let fixture = fixture();
        write_note(&fixture, "Launch.md", "before\n");
        let host = approved_host(&fixture, true, true);

        host.write_note(&client(), "Launch.md", "after\n", None)
            .expect("write");
        host.create_note(&client(), "Ship it", "text\n")
            .expect("create");
        host.rename_note(&client(), "Launch.md", "Landed")
            .expect("rename");

        let actions: Vec<String> = records_a_tool_wrote(&fixture)
            .into_iter()
            .map(|record| record.action)
            .collect();
        assert_eq!(actions, vec!["rename_note", "create_note", "write_note"]);
    }

    #[test]
    fn a_create_and_a_rename_from_a_read_only_client_change_nothing() {
        let fixture = fixture();
        let note = write_note(&fixture, "Launch.md", "# Launch\n");
        let host = approved_host(&fixture, true, false);

        assert!(matches!(
            host.create_note(&client(), "Ship it", "text\n")
                .expect_err("not approved to write"),
            ToolError::NotApproved { .. }
        ));
        assert!(matches!(
            host.rename_note(&client(), "Launch.md", "Landed")
                .expect_err("not approved to write"),
            ToolError::NotApproved { .. }
        ));
        assert!(note.is_file());
        assert!(!fixture.notes.join("Ship it.md").exists());
        assert!(!fixture.notes.join("Landed.md").exists());
    }

    #[test]
    fn text_over_the_ceiling_is_not_written() {
        let fixture = fixture();
        let note = write_note(&fixture, "Launch.md", "before\n");
        let host = approved_host(&fixture, true, true);
        let long = "x".repeat(MAX_NOTE_BYTES as usize + 1);

        assert!(matches!(
            host.write_note(&client(), "Launch.md", &long, None)
                .expect_err("over the ceiling"),
            ToolError::TooMuchText { .. }
        ));
        assert!(matches!(
            host.create_note(&client(), "Long", &long)
                .expect_err("over the ceiling"),
            ToolError::TooMuchText { .. }
        ));
        assert_eq!(
            std::fs::read_to_string(&note).expect("read back"),
            "before\n"
        );
    }

    #[test]
    fn a_write_to_a_note_that_is_not_there_mints_nothing() {
        let fixture = fixture();
        let host = approved_host(&fixture, true, true);

        assert!(matches!(
            host.write_note(&client(), "Missing.md", "text\n", None)
                .expect_err("write_note replaces a note, it does not mint one"),
            ToolError::NotFound { .. }
        ));
        assert!(!fixture.notes.join("Missing.md").exists());
    }

    #[test]
    fn a_deny_all_gate_writes_nothing() {
        let fixture = fixture();
        let note = write_note(&fixture, "Launch.md", "before\n");
        let host = ToolHost::open(
            &fixture.notes,
            &fixture.db,
            &fixture.writ,
            Box::new(DenyAll),
        )
        .expect("host");

        assert!(matches!(
            host.write_note(&client(), "Launch.md", "after\n", None)
                .expect_err("nobody is approved"),
            ToolError::NotApproved { .. }
        ));
        assert_eq!(
            std::fs::read_to_string(&note).expect("read back"),
            "before\n"
        );
    }
}
