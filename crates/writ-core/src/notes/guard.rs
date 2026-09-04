//! The write guard: whether a save may land on the file it is aimed at.
//!
//! The file on disk is the only copy of a note's text (ADR-028 §1), so a save
//! that writes over a change Writ never read loses that change with nothing
//! left to recover it from. The guard compares what Writ last saw on disk
//! against what is there now, and the decision is made here, on three digests
//! and nothing else. Reading the file, writing the dated copy of the losing
//! side and performing the write are `writ-storage`'s half.

use std::time::SystemTime;

use crate::hash::Sha256Digest;

/// What Writ last saw on disk for one note.
///
/// `hash` is the answer to every question about whether the file changed.
/// `size` and `mtime` are carried for diagnostics and for the cheap
/// short-circuit a caller may take before it reads a file back; neither is
/// ever the signal (see [`decide_save`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiskState {
    /// SHA-256 of the file's bytes.
    pub hash: Sha256Digest,
    /// The file's length in bytes.
    pub size: u64,
    /// The file's modification time, when the filesystem reports one.
    pub mtime: Option<SystemTime>,
}

/// What a save may do to the file it is aimed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveDecision {
    /// Nothing changed under us; write.
    Proceed,
    /// The file already holds the bytes being written. Nothing is written and
    /// the save succeeds with nothing said, whether the text has sat there
    /// since Writ last looked or somebody else landed the same edit first: a
    /// warning about a difference the user cannot see is worse than no check
    /// at all, and rewriting identical bytes moves the modification time and
    /// swaps the inode for a change a sync client then uploads.
    AlreadyIdentical,
    /// Disk changed under us and differs from the incoming content. The save
    /// would lose the change on disk.
    Refuse,
}

/// Decides what a save of `incoming_hash` may do.
///
/// `last_known` is what Writ recorded when it last read or wrote the file, and
/// `on_disk` is what the file holds now; `None` means, respectively, that Writ
/// has no record and that the file is gone.
///
/// mtime is never the signal: a touch, a sync round trip or a Time Machine
/// restore moves it without changing a byte, and size collides on any edit
/// that keeps the length. Only the digests decide.
///
/// A missing file proceeds, because the save recreates it, and so does a save
/// by a caller holding no record, because "has this changed since Writ last
/// looked" has no answer for a file Writ has not looked at.
///
/// The incoming digest is compared before the record is, so a save of text the
/// file already holds never writes. Asking "did it change under us" first
/// instead makes every Cmd+S on an untouched note replace the file with the
/// same bytes.
pub fn decide_save(
    last_known: Option<&DiskState>,
    on_disk: Option<&DiskState>,
    incoming_hash: Sha256Digest,
) -> SaveDecision {
    let Some(on_disk) = on_disk else {
        return SaveDecision::Proceed;
    };
    let Some(last_known) = last_known else {
        return SaveDecision::Proceed;
    };
    if on_disk.hash == incoming_hash {
        return SaveDecision::AlreadyIdentical;
    }
    if on_disk.hash == last_known.hash {
        return SaveDecision::Proceed;
    }
    SaveDecision::Refuse
}

/// macOS `SF_DATALESS`: the file's bytes are not on this machine.
///
/// The name and value are `sys/stat.h`'s. The constant is defined here rather
/// than read from a libc binding so the policy compiles and is testable on
/// every platform.
pub const SF_DATALESS: u32 = 0x4000_0000;

/// Whether the filesystem says the file's bytes have not been downloaded.
///
/// A file evicted by iCloud Drive still has a size and an mtime, and reading
/// it makes the provider daemon fetch it over the network. The guard would do
/// exactly that on every save (ADR-028 §5), so it asks first and refuses
/// rather than reading, writing, or guessing what the file holds.
///
/// `st_flags` is what the platform reports, or `None` where there is no such
/// flag; a platform without one never has a file that is not downloaded.
pub fn is_not_downloaded(st_flags: Option<u32>) -> bool {
    st_flags.is_some_and(|flags| flags & SF_DATALESS != 0)
}
