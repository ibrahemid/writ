//! The tool surface, in plain Rust.
//!
//! No `rmcp` type appears in any signature here: the protocol lives in
//! [`crate::server`] and this module is what an SDK bump does not touch. Every
//! method takes the calling client and puts it to the [`ConsentGate`] before it
//! opens anything, so a refusal costs no read (ADR-031 rule 3.2).
//!
//! What an allowed call may then do belongs to the note host. The gate's
//! verdict becomes a [`PermissionSet`] and the call runs through a handle
//! holding it, so the operation sits behind a check rather than after one
//! (ADR-032). What stays here is the wire: the argument checks a client can
//! fail, the clamp on how much one call answers with, the sentence a refusal is
//! spelled as, and the activity record, which names a tool and an actor the
//! host knows nothing about.
//!
//! Notes are read from the folder and facts about them from the index, which is
//! opened read-only: this process creates no database, runs no migration and
//! changes no row (ADR-031 rule 1.3). With the index absent or unreadable,
//! [`ToolHost::list_notes`] and [`ToolHost::read_note`] still answer from the
//! folder and the six index-derived tools return [`ToolError::IndexUnavailable`].
//!
//! The three write tools change a note's file and nothing else. They write
//! through the one facade a note's file is ever written by (ADR-032 section 4),
//! under `WriteOrigin::Mcp` and a policy that refuses with a copy, so a note
//! that changed since the client read it keeps what it holds and the client's
//! text lands beside it. There is no argument that turns that into an
//! overwrite.
//!
//! Nothing here stamps the app's ignore set, and that is load-bearing rather
//! than a gap. The stamp is how Writ tells its own writes apart from somebody
//! else's; a write from this process **is** somebody else's, and the running
//! app is meant to learn about it through the folder watcher and reconcile the
//! open tab (ADR-033). A stamped write would be swallowed. It is also the only
//! thing it could be: the ignore set lives in the app process and this one is
//! the client's child.

use std::path::{Path, PathBuf};

use writ_core::activity::{ActivityRecord, Actor};
use writ_core::hash::digest_from_hex;
use writ_core::notes::host::{Capability, HostError, NoteHost, PermissionSet};
use writ_core::notes::WriteOrigin;
use writ_storage::note_host::NoteHostImpl;
use writ_storage::paths::{file_name_only, relative_slug};

use crate::consent::{ClientId, ConsentGate, Decision};

/// Largest note a tool reads, in bytes (ADR-031 rule 4.8): the note host's
/// ceiling, which every surface reading a whole note is held to.
pub use writ_core::notes::host::MAX_NOTE_BYTES;

/// The extension a minted note's file carries.
const NOTE_EXTENSION: &str = "md";

/// Most notes or hits one call answers with, whatever the caller asked for.
pub const MAX_RESULTS: usize = 500;

/// The tools that only read, and the tools that change a note.
///
/// Both lists live in `writ-core` and are re-exported here: the server
/// registers them, the gate reads the split, and the settings row shows the
/// user the same names, so none of the three can drift
/// ([`writ_core::tools`]).
pub use writ_core::tools::{READ_TOOLS, WRITE_TOOLS};

/// The answers a tool hands back, which are the host's own shapes: one note in
/// the folder, a note's text, a search hit, a link, a backlink, a folder tag,
/// and where a write landed.
pub use writ_core::notes::host::{
    FolderTag, NoteBacklink, NoteContent, NoteHit as SearchResult, NoteLink, NoteSummary,
    WriteReceipt,
};

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

/// Where a renamed note went, and where it was.
///
/// The host answers with the file's length as well, which the activity record
/// takes and a client is not told: a rename changes no byte, so a length on the
/// wire would be a number with nothing to say.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RenameReceipt {
    /// The note's path now.
    pub path: String,
    /// The path it had before.
    pub previous_path: String,
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

/// What an allowed read tool may ask the host for.
///
/// One set for every read, because U5 approves a direction and not a tool: a
/// client approved to read may call any of the eight.
pub fn read_permissions() -> PermissionSet {
    [
        Capability::ListNotes,
        Capability::ReadNote,
        Capability::SearchNotes,
        Capability::ReadIndex,
    ]
    .into_iter()
    .collect()
}

/// What an allowed write tool may ask the host for.
pub fn write_permissions() -> PermissionSet {
    [
        Capability::WriteNote,
        Capability::CreateNote,
        Capability::RenameNote,
    ]
    .into_iter()
    .collect()
}

/// The notes folder and the index over it, behind a consent gate.
pub struct ToolHost {
    writ_dir: PathBuf,
    host: NoteHostImpl<'static>,
    gate: Box<dyn ConsentGate>,
}

impl std::fmt::Debug for ToolHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolHost")
            .field("notes_root", &self.host.notes_root())
            .field("writ_dir", &self.writ_dir)
            .field("index", &self.host.has_index())
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
    /// The host is opened holding nothing. Each call derives its set from the
    /// gate's verdict, so there is no handle in this process that may write
    /// before a client has been approved to.
    pub fn open(
        notes_root: &Path,
        db_path: &Path,
        writ_dir: &Path,
        gate: Box<dyn ConsentGate>,
    ) -> Result<Self, ToolError> {
        let host = NoteHostImpl::open(notes_root, Some(db_path), PermissionSet::default())
            .map_err(|_| ToolError::NotFound {
                path: notes_root.display().to_string(),
            })?;
        Ok(Self {
            writ_dir: writ_dir.to_path_buf(),
            host,
            gate,
        })
    }

    /// The folder every path argument is checked against.
    pub fn notes_root(&self) -> &Path {
        self.host.notes_root()
    }

    /// Whether the index answered when the host was opened.
    pub fn has_index(&self) -> bool {
        self.host.has_index()
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
        let host = self.permit(client, "list_notes")?;
        host.list_notes(prefix, limit.min(MAX_RESULTS))
            .map_err(|error| tool_error(client, "list_notes", error))
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
        let host = self.permit(client, "search_notes")?;
        host.search_notes(query, limit.min(MAX_RESULTS))
            .map_err(|error| tool_error(client, "search_notes", error))
    }

    /// The whole file at `path`, frontmatter included.
    pub fn read_note(&self, client: &ClientId, path: &str) -> Result<NoteContent, ToolError> {
        let host = self.permit(client, "read_note")?;
        host.read_note(path)
            .map_err(|error| tool_error(client, "read_note", error))
    }

    /// Every link written in the note at `path`.
    pub fn note_links(&self, client: &ClientId, path: &str) -> Result<Vec<NoteLink>, ToolError> {
        let host = self.permit(client, "note_links")?;
        host.note_links(path)
            .map_err(|error| tool_error(client, "note_links", error))
    }

    /// Every link in another note that points at the note at `path`.
    pub fn note_backlinks(
        &self,
        client: &ClientId,
        path: &str,
    ) -> Result<Vec<NoteBacklink>, ToolError> {
        let host = self.permit(client, "note_backlinks")?;
        host.note_backlinks(path)
            .map_err(|error| tool_error(client, "note_backlinks", error))
    }

    /// The frontmatter properties of the note at `path`.
    pub fn note_properties(
        &self,
        client: &ClientId,
        path: &str,
    ) -> Result<Vec<NoteProperty>, ToolError> {
        let host = self.permit(client, "note_properties")?;
        let facts = host
            .note_facts(path)
            .map_err(|error| tool_error(client, "note_properties", error))?;
        Ok(facts
            .properties
            .into_iter()
            .map(|(name, value)| NoteProperty { name, value })
            .collect())
    }

    /// The tags written in the note at `path`.
    pub fn note_tags(&self, client: &ClientId, path: &str) -> Result<Vec<NoteTag>, ToolError> {
        let host = self.permit(client, "note_tags")?;
        let facts = host
            .note_facts(path)
            .map_err(|error| tool_error(client, "note_tags", error))?;
        Ok(facts
            .tags
            .into_iter()
            .map(|(tag, line)| NoteTag { tag, line })
            .collect())
    }

    /// Every tag in the folder, with the number of notes carrying each.
    pub fn folder_tags(&self, client: &ClientId) -> Result<Vec<FolderTag>, ToolError> {
        let host = self.permit(client, "folder_tags")?;
        host.folder_tags()
            .map_err(|error| tool_error(client, "folder_tags", error))
    }

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
        let permitted = self.permit(client, "write_note");
        let written = permitted
            .and_then(|host| self.replace_text(&host, client, path, content, expected_hash));
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
        let permitted = self.permit(client, "create_note");
        let created = permitted.and_then(|host| self.mint_note(&host, client, name, content));
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
        let permitted = self.permit(client, "rename_note");
        let renamed = permitted.and_then(|host| self.move_name(&host, client, path, new_name));
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
        host: &NoteHostImpl<'_>,
        client: &ClientId,
        path: &str,
        content: &str,
        expected_hash: Option<&str>,
    ) -> Result<WriteReceipt, ToolError> {
        self.text_fits(content)?;
        // Read as a digest and no further: a client that read the note over the
        // wire knows neither the length nor the modification time of the file
        // it read, and the guard compares digests.
        let last_known = match expected_hash {
            Some(hex) => {
                Some(
                    digest_from_hex(hex).ok_or_else(|| ToolError::HashNotUnderstood {
                        path: path.to_string(),
                    })?,
                )
            }
            None => None,
        };
        host.write_note(path, content, last_known, self.origin(client))
            .map_err(|error| tool_error(client, "write_note", error))
    }

    /// [`ToolHost::create_note`] past the gate.
    fn mint_note(
        &self,
        host: &NoteHostImpl<'_>,
        client: &ClientId,
        name: &str,
        content: &str,
    ) -> Result<WriteReceipt, ToolError> {
        self.text_fits(content)?;
        host.create_note(name, content, self.origin(client))
            .map_err(|error| tool_error(client, "create_note", error))
    }

    /// [`ToolHost::rename_note`] past the gate, with the file's length for the
    /// record.
    fn move_name(
        &self,
        host: &NoteHostImpl<'_>,
        client: &ClientId,
        path: &str,
        new_name: &str,
    ) -> Result<(RenameReceipt, u64), ToolError> {
        let moved = host
            .rename_note(path, new_name, self.origin(client))
            .map_err(|error| tool_error(client, "rename_note", error))?;
        Ok((
            RenameReceipt {
                path: moved.path,
                previous_path: moved.previous_path,
            },
            moved.bytes,
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
    /// logged by its file name alone: there is no note to name, and the rest of
    /// what a client sent is a machine's folder layout in a file the user may
    /// hand to somebody.
    fn logged_path(&self, path: &str) -> String {
        let root = self.host.notes_root();
        let given = Path::new(path);
        let candidate = if given.is_absolute() {
            given.to_path_buf()
        } else {
            root.join(given)
        };
        relative_slug(root, &candidate).unwrap_or_else(|| file_name_only(path))
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

    /// Called first by every method: the gate decides once, and an allowed call
    /// gets a handle holding what its direction may ask for.
    ///
    /// A refusal never reaches the host, so it opens no file and runs no query.
    /// `Pending` refuses too: U5 is what turns it into a row the user can act
    /// on.
    fn permit(&self, client: &ClientId, tool: &str) -> Result<NoteHostImpl<'static>, ToolError> {
        self.allow(client, tool)?;
        let held = match writ_core::tools::is_write_tool(tool) {
            true => write_permissions(),
            false => read_permissions(),
        };
        Ok(self.host.with_permissions(held))
    }

    /// The gate's verdict on this client calling this tool.
    fn allow(&self, client: &ClientId, tool: &str) -> Result<(), ToolError> {
        match self.gate.decide(client, tool) {
            Decision::Allow => Ok(()),
            Decision::Refuse | Decision::Pending => Err(ToolError::NotApproved {
                client: client.name.clone(),
                tool: tool.to_string(),
            }),
        }
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

/// A host refusal as the tool's own, naming the path the client wrote.
///
/// Every message a client sees names a path, a name or a length. The digest the
/// guard carries and the folder it names are the app's own spellings of this
/// machine, and neither is in the answer.
///
/// A text that is not UTF-8 and a file that would not open are one sentence
/// here, because they are one situation to a client: the note did not come
/// back. `NotPermitted` is the gate's answer spelled the gate's way, which is
/// the only way it can arise: a set is derived from a verdict and never from an
/// argument.
fn tool_error(client: &ClientId, tool: &str, error: HostError) -> ToolError {
    match error {
        HostError::NotPermitted { .. } => ToolError::NotApproved {
            client: client.name.clone(),
            tool: tool.to_string(),
        },
        HostError::OutsideNotesFolder { path } => ToolError::OutsideNotesFolder { path },
        HostError::IndexUnavailable => ToolError::IndexUnavailable,
        HostError::NotFound { path } => ToolError::NotFound { path },
        HostError::TooLarge { path, bytes } => ToolError::TooLarge { path, bytes },
        HostError::Unreadable { path } | HostError::NotText { path } => {
            ToolError::Unreadable { path }
        }
        HostError::Conflict {
            path,
            conflict_copy,
        } => ToolError::Conflict {
            path,
            conflict_copy,
        },
        HostError::NameEmpty => ToolError::NameEmpty,
        HostError::NameTaken { name } => ToolError::NameTaken { name },
        HostError::NotDownloaded { path } => ToolError::NotDownloaded { path },
        HostError::Unwritable { path } => ToolError::Unwritable { path },
    }
}

// The tests below read the index directly, to check a tool's answer against the
// rows it came from.
#[cfg(test)]
use writ_storage::notes_index::{self, NotesIndexStore};

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
    fn a_root_and_a_file_that_differ_only_in_punctuation_still_meet() {
        // Neither spelling is one a caller writes by hand: they are what two
        // resolutions of the same folder hand back.
        assert_eq!(
            relative_slug(Path::new("/notes/"), Path::new("/notes/./Projects/Writ.md")),
            Some("Projects/Writ.md".to_string())
        );
    }

    #[test]
    fn a_file_outside_the_folder_is_logged_by_its_name_alone() {
        let fixture = fixture();
        let outside = fixture.writ.join("outside.md");
        std::fs::write(&outside, "not a note of this folder\n").expect("seed");
        let host = approved_host(&fixture, true, true);

        host.write_note(&client(), &outside.to_string_lossy(), "overwritten\n", None)
            .expect_err("a path out of the folder is not written");

        assert_eq!(
            records_a_tool_wrote(&fixture)[0].path.as_deref(),
            Some(Path::new("outside.md")),
            "the log names the file, never the folder layout of the machine it is on"
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_verbatim_root_and_a_plain_file_are_the_same_folder() {
        // What Windows actually hands the two sides: `resolve_for_containment`
        // canonicalises the root and keeps `\\?\`, `notes_index::index_key`
        // canonicalises the file and drops it.
        assert_eq!(
            relative_slug(
                Path::new(r"\\?\C:\notes"),
                Path::new(r"C:\notes\Projects\Writ.md")
            ),
            Some("Projects/Writ.md".to_string())
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_root_and_a_file_that_separate_their_names_differently_still_meet() {
        assert_eq!(
            relative_slug(
                Path::new(r"C:\notes"),
                Path::new("C:/notes/Projects/Writ.md")
            ),
            Some("Projects/Writ.md".to_string())
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
