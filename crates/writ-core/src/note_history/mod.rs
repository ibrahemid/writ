//! What of a note's past is worth keeping, and what is worth dropping.
//!
//! The file is the only copy of a note's text (ADR-028 §1), which is what
//! makes a version store worth having and also what bounds it: the newest
//! text is always the file's, so everything kept here is a text the file
//! stopped holding. Nothing in this module touches a filesystem or a
//! database. It answers four questions and `writ-storage` carries them out.
//!
//! Whether a text is worth an entry at all ([`is_versionable`]), whether one
//! is worth an entry *now* ([`should_capture`]), which note an entry belongs
//! to ([`VersionKey`]), and which entries have to go ([`prune_plan`]).
//!
//! This is not [`crate::history`], which is the list of recently closed tabs,
//! and not [`crate::recovery`], which is the crash snapshot.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use crate::hash::Sha256Digest;
use crate::notes::identity::FileIdentity;

mod prune;

pub use prune::{prune_plan, PrunePlan, VersionFacts};

/// How long an entry is kept before its age alone retires it.
///
/// The public copy says "up to 30 days" rather than 30 days, because the two
/// caps below can retire a younger entry (spec 470).
pub const RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// How many entries one note keeps.
pub const MAX_VERSIONS_PER_NOTE: usize = 200;

/// How large the store may grow, counting the bytes of every entry it holds.
pub const MAX_STORE_BYTES: u64 = 250 * 1024 * 1024;

/// How close together two captures of a note have to be for the second to be
/// merged into the first.
///
/// Autosave writes on a timer, so a minute of typing is a run of saves whose
/// intermediate texts nobody would ever ask for. The window is VS Code's, and
/// so is the reason for it.
pub const MERGE_WINDOW: Duration = Duration::from_secs(10);

/// The largest note that is versioned at all.
///
/// A note this size is not a note somebody typed, and keeping 200 of it is
/// how a version store turns into a disk problem (spec 470).
pub const MAX_NOTE_BYTES: u64 = 2 * 1024 * 1024;

/// Which note's history an entry belongs to.
///
/// Both halves are needed and neither is enough. A path alone loses a note
/// renamed inside the notes folder, which is the specific limitation this
/// beats: Obsidian's file recovery is keyed by path and a move breaks it. An
/// identity alone loses a note whose file was deleted, because there is
/// nothing left to read an id from, and it loses every note on a volume that
/// has no stable id to give.
///
/// `path` is the note's place inside the notes folder — `Ideas/Launch.md`,
/// never an absolute path. The folder can then be moved, or spelled two ways
/// by two callers, without the store noticing (ADR-031 §5.2, and the two
/// Windows spellings U7 spent a CI round on).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionKey {
    /// What the filesystem calls the file, when it will say.
    ///
    /// `None` where nothing could be read — a file that is already gone. A
    /// [`FileIdentity::Fallback`] is recorded as no identity at all
    /// ([`Self::durable_identity`]): it describes a file by its path and its
    /// contents, so matching on it would answer "the same file" for a file
    /// that merely looks like the one that was there.
    pub identity: Option<FileIdentity>,
    /// Where the note sits inside the notes folder.
    pub path: PathBuf,
}

impl VersionKey {
    /// A key for the note at `path` inside the notes folder, as `identity`
    /// describes it.
    pub fn new(identity: Option<FileIdentity>, path: PathBuf) -> Self {
        Self { identity, path }
    }

    /// The identity, if it is one that can recognise the same file elsewhere.
    ///
    /// A [`FileIdentity::Fallback`] answers `None`, which is what leaves such
    /// a note keyed by its path alone — the degraded answer
    /// [`FileIdentity::is_durable`] exists to signal.
    pub fn durable_identity(&self) -> Option<&FileIdentity> {
        self.identity.as_ref().filter(|id| id.is_durable())
    }
}

/// Whether a note of `bytes_len` bytes is versioned.
///
/// The ceiling is on the note, not on the store: a note over it is saved like
/// any other and simply keeps no history. Nothing is refused for being large.
pub fn is_versionable(bytes_len: u64) -> bool {
    bytes_len <= MAX_NOTE_BYTES
}

/// Whether a text the store has just been handed earns an entry of its own.
///
/// Two reasons not to keep it. It is the text the newest entry already holds,
/// so keeping it again would cost a row for nothing — an idle save storm, a
/// tab reopened on a file nothing touched, a reload of a file that came back
/// the way it went. Or the newest entry is younger than [`MERGE_WINDOW`], so
/// this text is one keystroke's worth of difference from it, which is the
/// intermediate state of a run of autosaves rather than a version of the note.
///
/// `last_entry_at` is when the newest entry for this note was made, and
/// `last_hash` is what it holds. Both are `None` for a note with no history
/// yet.
///
/// A caller capturing a text the file is about to stop holding passes
/// `last_entry_at: None`, whatever the newest entry's age. The merge window
/// exists to collapse a run of saves, each of which leaves its text in the
/// file; a text that is being replaced by somebody else's has no successor to
/// be merged into, and merging it away is losing it. The hash rule still
/// applies there, because a text the store already holds is held.
pub fn should_capture(
    last_entry_at: Option<SystemTime>,
    now: SystemTime,
    content_hash: Sha256Digest,
    last_hash: Option<Sha256Digest>,
) -> bool {
    if last_hash == Some(content_hash) {
        return false;
    }
    match last_entry_at {
        Some(at) => now
            .duration_since(at)
            .map(|since| since >= MERGE_WINDOW)
            // A clock that went backwards between two captures leaves no
            // window to measure, and the entry is kept: a text with nowhere
            // else to be is worth more than a row.
            .unwrap_or(true),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::hash::sha256_bytes;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn fallback(path: &str) -> FileIdentity {
        FileIdentity::Fallback {
            path: path.to_string(),
            size: 12,
            mtime_ms: Some(1),
            hash: sha256_bytes(b"a note"),
        }
    }

    fn inode(ino: u64) -> FileIdentity {
        FileIdentity::Inode {
            dev: 1,
            ino,
            birth_ns: None,
        }
    }

    #[test]
    fn the_first_text_a_note_ever_had_is_kept() {
        assert!(should_capture(None, at(0), sha256_bytes(b"one"), None));
    }

    #[test]
    fn a_hundred_saves_inside_the_window_leave_one_entry() {
        let first = at(1_000);
        let mut kept = 1;
        for save in 1..100u64 {
            let text = format!("draft {save}");
            if should_capture(
                Some(first),
                first + Duration::from_millis(save * 100),
                sha256_bytes(text.as_bytes()),
                Some(sha256_bytes(b"draft 0")),
            ) {
                kept += 1;
            }
        }
        assert_eq!(kept, 1, "a run of saves inside the window is one entry");
    }

    #[test]
    fn a_save_after_the_window_is_a_second_entry() {
        let first = at(1_000);
        assert!(should_capture(
            Some(first),
            first + Duration::from_secs(11),
            sha256_bytes(b"the eleventh second"),
            Some(sha256_bytes(b"the first")),
        ));
    }

    #[test]
    fn the_window_closes_on_its_own_boundary() {
        let first = at(1_000);
        assert!(!should_capture(
            Some(first),
            first + Duration::from_millis(9_999),
            sha256_bytes(b"just inside"),
            Some(sha256_bytes(b"the first")),
        ));
        assert!(should_capture(
            Some(first),
            first + MERGE_WINDOW,
            sha256_bytes(b"just outside"),
            Some(sha256_bytes(b"the first")),
        ));
    }

    #[test]
    fn the_text_the_newest_entry_holds_is_never_kept_twice() {
        let text = sha256_bytes(b"nothing changed");
        assert!(!should_capture(Some(at(0)), at(9_000), text, Some(text)));
        assert!(
            !should_capture(None, at(9_000), text, Some(text)),
            "an idle save storm costs nothing however long it runs"
        );
    }

    #[test]
    fn a_text_that_is_about_to_be_replaced_is_never_merged_away() {
        let first = at(1_000);
        assert!(should_capture(
            None,
            first + Duration::from_secs(1),
            sha256_bytes(b"what the file held before somebody else wrote it"),
            Some(sha256_bytes(b"what writ last saved")),
        ));
    }

    #[test]
    fn a_clock_that_went_backwards_keeps_the_text() {
        assert!(should_capture(
            Some(at(9_000)),
            at(1_000),
            sha256_bytes(b"after"),
            Some(sha256_bytes(b"before")),
        ));
    }

    #[test]
    fn a_note_over_two_megabytes_is_not_versioned() {
        assert!(is_versionable(0));
        assert!(is_versionable(20 * 1024));
        assert!(is_versionable(MAX_NOTE_BYTES));
        assert!(!is_versionable(MAX_NOTE_BYTES + 1));
        assert!(!is_versionable(3 * 1024 * 1024));
    }

    #[test]
    fn a_key_with_no_stable_id_is_keyed_by_its_path_alone() {
        let key = VersionKey::new(
            Some(fallback("/notes/Launch.md")),
            PathBuf::from("Launch.md"),
        );
        assert!(
            key.durable_identity().is_none(),
            "a fallback id describes a file rather than naming one"
        );
        assert_eq!(key.path, PathBuf::from("Launch.md"));
    }

    #[test]
    fn a_key_with_a_stable_id_offers_it() {
        let key = VersionKey::new(Some(inode(7)), PathBuf::from("Ideas/Launch.md"));
        assert_eq!(key.durable_identity(), Some(&inode(7)));
    }

    #[test]
    fn a_key_for_a_file_that_is_gone_has_no_identity_at_all() {
        let key = VersionKey::new(None, PathBuf::from("Launch.md"));
        assert!(key.durable_identity().is_none());
    }
}
