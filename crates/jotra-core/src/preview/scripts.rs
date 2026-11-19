//! What a file's preview-scripts opt-in covers, and what takes it away
//! (ADR-043).
//!
//! The opt-in is given to the document the user had in front of them, which
//! Jotra built from the bytes it last read from the file. It covers those bytes
//! and the bytes Jotra writes to the file since. Bytes Jotra comes to hold that it
//! did not write and the opt-in does not cover are another program's, so the
//! opt-in lapses before they reach the preview; a file edited and saved in Jotra
//! keeps it.
//!
//! Both sides of the comparison are digests Jotra already has: the one recorded
//! with the opt-in, and the one of the bytes Jotra last read from the file or
//! wrote to it. Deciding reads no file, so no check can wait on a file that
//! does not answer.

use crate::hash::Sha256Digest;

/// What checking an opt-in does to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptInStanding {
    /// Jotra holds bytes the opt-in covers, or none of the file at all on both
    /// sides, so the scripts may run.
    Stands,
    /// Jotra holds bytes of the file the opt-in does not cover, so it is
    /// forgotten.
    Lapses,
    /// Jotra holds none of the file's bytes to compare: the scripts stay off
    /// this time and the opt-in is kept for a check that has them.
    Unchecked,
}

/// Whether an opt-in still covers the bytes Jotra holds for its file.
///
/// `covered` is the digest of the bytes the opt-in covers: what Jotra held for
/// the file when the user opted in, or what Jotra last wrote to it. `None` is an
/// opt-in given while Jotra held none of the file's bytes, a file not written
/// yet, which covers none.
///
/// `seen` is the digest of the bytes Jotra holds for the file: what it last read
/// from it or wrote to it, or what a write of Jotra's found there just before
/// it landed. `None` when Jotra holds none.
pub fn opt_in_standing(covered: Option<Sha256Digest>, seen: Option<Sha256Digest>) -> OptInStanding {
    match (covered, seen) {
        (Some(covered), Some(seen)) if covered == seen => OptInStanding::Stands,
        (_, Some(_)) => OptInStanding::Lapses,
        (None, None) => OptInStanding::Stands,
        (Some(_), None) => OptInStanding::Unchecked,
    }
}

/// Whether an opt-in carries over a write Jotra just made to the file, so that
/// the digest of what Jotra wrote replaces the recorded one.
///
/// `found` is what the write's guard found at the path before the bytes
/// landed, `None` for no file. Only an opt-in that stands on it carries over: a
/// save of another program's bytes, or of a file whose bytes were never
/// compared, would otherwise pass them off as Jotra's.
pub fn opt_in_carries_over_write(
    covered: Option<Sha256Digest>,
    found: Option<Sha256Digest>,
) -> bool {
    opt_in_standing(covered, found) == OptInStanding::Stands
}
