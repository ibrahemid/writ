//! What a file's preview-scripts opt-in covers, and what takes it away
//! (ADR-043).
//!
//! The opt-in is given to the document the user had in front of them. It
//! covers the bytes the file held then and the bytes Writ writes to it since.
//! A file another program changed holds a document nobody opted in, so the
//! opt-in goes with the change; a file edited and saved in Writ keeps it. Two
//! checks enforce that: the watcher's report of a change to an open file
//! ([`opt_in_lost_by`]), and a comparison of the file's digest with the one
//! recorded with the opt-in ([`opt_in_standing`]), which also catches a change
//! made while no tab had the file open or while Writ was not running.

use crate::events::bus::WritEvent;
use crate::hash::Sha256Digest;
use crate::watcher::change_event::ExternalChange;

/// The buffer whose scripts opt-in `event` takes away: the one whose file
/// another program modified.
///
/// Writ's own writes never reach the bus as a change: the watcher drops them
/// against the ignore set and the disk state Writ recorded. A move keeps the
/// opt-in, because the bytes did not change, and a removal leaves nothing to
/// run; a file that appears at the path again arrives as a modification.
pub fn opt_in_lost_by(event: &WritEvent) -> Option<&str> {
    match event {
        WritEvent::BufferExternal {
            buffer_id,
            change: ExternalChange::Modified,
            ..
        } => Some(buffer_id),
        _ => None,
    }
}

/// What the file behind an opted-in document holds when the opt-in is checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileLook {
    /// The document has no file yet, or nothing is at its path.
    Absent,
    /// The file's bytes, by their SHA-256 digest.
    Holds(Sha256Digest),
    /// The file could not be read, or reading it would fetch bytes that are
    /// not on this machine.
    Unread,
}

/// What checking the file does to an opt-in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptInStanding {
    /// The file holds the bytes the opt-in covers, or none at all, so the
    /// scripts may run.
    Stands,
    /// The file holds bytes the opt-in does not cover, so it is forgotten.
    Lapses,
    /// The file could not be checked: the scripts stay off this time and the
    /// opt-in is kept for a check that can read the file.
    Unchecked,
}

/// Whether an opt-in still covers its file.
///
/// `recorded` is the digest of the bytes the opt-in covers: what the file held
/// when the user opted in, or what Writ last wrote to it. `None` is an opt-in
/// given before the file had any bytes, which covers none, so a file that
/// appears later holds bytes it does not cover. A document whose file is
/// absent shows the text Writ holds for it, which the opt-in covers.
pub fn opt_in_standing(recorded: Option<Sha256Digest>, look: FileLook) -> OptInStanding {
    match look {
        FileLook::Absent => OptInStanding::Stands,
        FileLook::Holds(digest) if recorded == Some(digest) => OptInStanding::Stands,
        FileLook::Holds(_) => OptInStanding::Lapses,
        FileLook::Unread => OptInStanding::Unchecked,
    }
}

/// Whether an opt-in carries over a write Writ is about to make to the file,
/// so that the digest of what Writ writes replaces the recorded one.
///
/// Only an opt-in that stands does. One whose file could not be checked does
/// not: recording the new digest would vouch for bytes nobody compared, and a
/// save would pass another program's change off as Writ's.
pub fn opt_in_carries_over_write(recorded: Option<Sha256Digest>, look: FileLook) -> bool {
    opt_in_standing(recorded, look) == OptInStanding::Stands
}
