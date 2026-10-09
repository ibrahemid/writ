//! The one surface a program reaches a note through (ADR-032).
//!
//! Two consumers hold this: the tool surface a connected program calls
//! (`writ-mcp`) and the chat pane in the app. Both go through the same methods,
//! so what a note can be asked for, and what it costs to ask, is decided once.
//!
//! There is no ambient authority. Every method checks a [`Capability`] against
//! the [`capability::PermissionSet`] the implementation was opened with, as its
//! first line, and a call without the matching capability answers
//! [`HostError::NotPermitted`] before it resolves a path, stats a file or asks
//! the index. A consumer whose set holds no write capability has no code path
//! to a write.
//!
//! The three writes carry a [`crate::notes::WriteOrigin`] naming the consumer,
//! and they do not write themselves: they go through the one guarded facade, so
//! a note changed since the caller last saw it keeps what it holds and the text
//! handed in lands beside it. What the caller last saw is a [`LastKnown`] the
//! caller has to name: a digest, the text this host last saw the note hold, or
//! [`LastKnown::Overwrite`]. A write that replaces whatever the note holds is
//! that third value and nothing else, so a consumer cannot make one by leaving
//! an argument out (ADR-032 section 4).
//!
//! The implementation is `writ_storage::note_host::NoteHostImpl`, which owns
//! the folder walk, the index and the facade. This module is the declaration
//! and nothing else, which is what keeps `writ-storage` and `writ-plugin`
//! pointing at `writ-core` and not at each other.

/// What a consumer is allowed to ask for.
pub mod capability;
/// Why a call produced no answer.
pub mod error;
/// What the surface answers with.
pub mod types;

pub use capability::{Capability, PermissionSet};
pub use error::HostError;
pub use types::{
    FolderTag, NoteBacklink, NoteContent, NoteFacts, NoteHit, NoteLink, NoteSummary, RenameReceipt,
    WriteReceipt,
};

use crate::hash::Sha256Digest;
use crate::notes::WriteOrigin;

/// Largest note this surface reads, in bytes (ADR-031 rule 4.8).
pub const MAX_NOTE_BYTES: u64 = 2 * 1024 * 1024;

/// What a write expects the note to hold before it lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastKnown {
    /// The digest of the text the caller last saw the note hold: the `hash` a
    /// read or an earlier write handed back. The write lands only while the
    /// note still holds that text.
    Hash(Sha256Digest),
    /// The text this host last saw the note hold: what its own
    /// [`NoteHost::read_note`] handed back, or what its own write or mint left
    /// there. The write lands only while the note still holds that text, and a
    /// note this host has not seen is refused with [`HostError::HashRequired`].
    ///
    /// Every handle derived from one host sees the same notes, so a read on one
    /// call and a write on the next compare against each other.
    LastSeen,
    /// Whatever the note holds now. The caller asked to replace it without a
    /// comparison, and says so by passing this.
    Overwrite,
}

/// The notes folder, as a program is allowed to see it.
pub trait NoteHost {
    /// Every note in the folder, path-ordered, truncated to `limit`.
    ///
    /// `prefix` is matched against the path relative to the folder and against
    /// the path this method hands back, so both spellings of a folder list it
    /// and nothing else. Answers from the folder, not the index: the file is
    /// the only copy (ADR-028 section 1).
    fn list_notes(&self, prefix: Option<&str>, limit: usize)
        -> Result<Vec<NoteSummary>, HostError>;

    /// The whole file at `path`, frontmatter included.
    fn read_note(&self, path: &str) -> Result<NoteContent, HostError>;

    /// What the file at `path` is called and how long it is, without reading
    /// its text.
    ///
    /// A dialog that asks to send a note states the bytes the send will read,
    /// and asking costs no note text.
    fn note_summary(&self, path: &str) -> Result<NoteSummary, HostError>;

    /// Up to `limit` notes whose text matches `query`.
    fn search_notes(&self, query: &str, limit: usize) -> Result<Vec<NoteHit>, HostError>;

    /// Every link written in the note at `path`.
    fn note_links(&self, path: &str) -> Result<Vec<NoteLink>, HostError>;

    /// Every link in another note that points at the note at `path`.
    fn note_backlinks(&self, path: &str) -> Result<Vec<NoteBacklink>, HostError>;

    /// The properties and tags of the note at `path`.
    fn note_facts(&self, path: &str) -> Result<NoteFacts, HostError>;

    /// Every tag in the folder, with the number of notes carrying each.
    fn folder_tags(&self) -> Result<Vec<FolderTag>, HostError>;

    /// Replaces the text of the note at `path`.
    ///
    /// `last_known` is what the caller last saw the note hold. For
    /// [`LastKnown::Hash`] and [`LastKnown::LastSeen`] the write is made only
    /// while the note still holds that text: a note somebody edited in between
    /// keeps what it holds, the text handed in is put beside it as a dated
    /// copy, and [`HostError::Conflict`] names the copy. A note this host has
    /// not seen answers [`LastKnown::LastSeen`] with
    /// [`HostError::HashRequired`] and is left alone. [`LastKnown::Overwrite`]
    /// is made against whatever the file holds now.
    ///
    /// The bytes land as they were handed in. Text the note already holds is
    /// not written again, so the modification time does not move.
    fn write_note(
        &self,
        path: &str,
        content: &str,
        last_known: LastKnown,
        origin: WriteOrigin,
    ) -> Result<WriteReceipt, HostError>;

    /// Mints a note called `name` in the notes folder.
    ///
    /// The name is sanitised into a filename, so a name that spells a path is a
    /// name and not a path. A note of that name already in the folder is
    /// answered with [`HostError::NameTaken`] and left alone.
    fn create_note(
        &self,
        name: &str,
        content: &str,
        origin: WriteOrigin,
    ) -> Result<WriteReceipt, HostError>;

    /// Renames the note at `path` to `new_name`, inside the folder it is in.
    ///
    /// No link in any other note is rewritten. A rename that updates the links
    /// pointing at a note is an offer made to the user, and a bulk rewrite on a
    /// program's say-so is the thing that offer exists to prevent (ADR-031 rule
    /// 4.7).
    fn rename_note(
        &self,
        path: &str,
        new_name: &str,
        origin: WriteOrigin,
    ) -> Result<RenameReceipt, HostError>;
}
