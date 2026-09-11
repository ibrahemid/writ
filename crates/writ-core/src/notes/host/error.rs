//! Why a call to the note surface produced no answer.

use super::capability::Capability;

/// What the host answers instead of doing the work.
///
/// Every variant names a path, a name, a length or a capability and nothing
/// else: no note text reaches a consumer's error rendering, and neither does
/// the folder this machine keeps its notes in. A consumer turns these into the
/// sentences its own surface shows.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HostError {
    /// The set this host was opened with does not hold the capability the
    /// method checks. Answered before any path is resolved, any file is
    /// opened and any index is asked.
    #[error("this host does not hold {capability:?}")]
    NotPermitted {
        /// The capability the method checks.
        capability: Capability,
    },
    /// The path argument names something the notes folder does not hold.
    #[error("{path} is not in the notes folder")]
    OutsideNotesFolder {
        /// The path as the caller wrote it.
        path: String,
    },
    /// The index is absent, unreadable, or at another version.
    #[error("the note index is not readable")]
    IndexUnavailable,
    /// Nothing is at the path.
    #[error("there is no note at {path}")]
    NotFound {
        /// The path as the caller wrote it.
        path: String,
    },
    /// The file is over [`super::MAX_NOTE_BYTES`].
    #[error("{path} is {bytes} bytes")]
    TooLarge {
        /// The path as the caller wrote it.
        path: String,
        /// The file's length.
        bytes: u64,
    },
    /// The file is there and this process could not read it.
    #[error("{path} could not be read")]
    Unreadable {
        /// The path as the caller wrote it.
        path: String,
    },
    /// The file is there and what it holds is not UTF-8 text.
    #[error("{path} is not text")]
    NotText {
        /// The path as the caller wrote it.
        path: String,
    },
    /// The note holds something other than what the caller last saw, so the
    /// write was not made. The caller's text is beside the note.
    #[error("{path} changed on disk after it was read")]
    Conflict {
        /// The path as the caller wrote it.
        path: String,
        /// The dated copy the caller's text was written to, when one could be
        /// written.
        conflict_copy: Option<String>,
    },
    /// The name handed in holds nothing a file can be called.
    #[error("{}", crate::notes::NAME_IS_EMPTY)]
    NameEmpty,
    /// A note of that name is already in the folder.
    #[error("{}", crate::notes::name_is_taken(name))]
    NameTaken {
        /// The name as the folder would spell it.
        name: String,
    },
    /// The file's bytes are not on this machine yet.
    #[error("{path} has not finished downloading to this machine")]
    NotDownloaded {
        /// The path as the caller wrote it.
        path: String,
    },
    /// The file is there and this process could not write it.
    #[error("{path} could not be written")]
    Unwritable {
        /// The path as the caller wrote it.
        path: String,
    },
}
