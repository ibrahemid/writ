//! What a file's preview-scripts opt-in covers, and what takes it away
//! (ADR-043).
//!
//! The opt-in is given to the document the user had in front of them, which
//! Writ built from the bytes it last read from the file. It covers those bytes
//! and the bytes Writ writes to the file since. Bytes Writ comes to hold that it
//! did not write and the opt-in does not cover are another program's, so the
//! opt-in lapses before they reach the preview; a file edited and saved in Writ
//! keeps it.
//!
//! Both sides of the comparison are digests Writ already has: the one recorded
//! with the opt-in, and the one of the bytes Writ last read from the file or
//! wrote to it. Deciding reads no file, so no check can wait on a file that
//! does not answer.

use crate::hash::Sha256Digest;

/// What checking an opt-in does to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptInStanding {
    /// Writ holds bytes the opt-in covers, or none of the file at all on both
    /// sides, so the scripts may run.
    Stands,
    /// Writ holds bytes of the file the opt-in does not cover, so it is
    /// forgotten.
    Lapses,
    /// Writ holds none of the file's bytes to compare: the scripts stay off
    /// this time and the opt-in is kept for a check that has them.
    Unchecked,
}

/// Whether an opt-in still covers the bytes Writ holds for its file.
///
/// `covered` is the digest of the bytes the opt-in covers: what Writ held for
/// the file when the user opted in, or what Writ last wrote to it. `None` is an
/// opt-in given while Writ held none of the file's bytes, a file not written
/// yet, which covers none.
///
/// `seen` is the digest of the bytes Writ holds for the file: what it last read
/// from it or wrote to it, or what a write of Writ's found there just before
/// it landed. `None` when Writ holds none.
pub fn opt_in_standing(covered: Option<Sha256Digest>, seen: Option<Sha256Digest>) -> OptInStanding {
    match (covered, seen) {
        (Some(covered), Some(seen)) if covered == seen => OptInStanding::Stands,
        (_, Some(_)) => OptInStanding::Lapses,
        (None, None) => OptInStanding::Stands,
        (Some(_), None) => OptInStanding::Unchecked,
    }
}

/// Whether an opt-in carries over a write Writ just made to the file, so that
/// the digest of what Writ wrote replaces the recorded one.
///
/// `found` is what the write's guard found at the path before the bytes
/// landed, `None` for no file. Only an opt-in that stands on it carries over: a
/// save of another program's bytes, or of a file whose bytes were never
/// compared, would otherwise pass them off as Writ's.
pub fn opt_in_carries_over_write(
    covered: Option<Sha256Digest>,
    found: Option<Sha256Digest>,
) -> bool {
    opt_in_standing(covered, found) == OptInStanding::Stands
}
