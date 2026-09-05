//! A path that went empty, held for one more delivery window.
//!
//! A rename is two halves: the old path emptying and the new one filling. A
//! watcher delivers what it saw inside a window, and the two halves do not
//! have to land in the same one. `notify_debouncer_mini` closes a window on a
//! deadline set by its first event and never extends it, so a rename that
//! straddles that deadline arrives as an empty path in one delivery and a new
//! file in the next. Answering the first delivery on its own reads a move as a
//! deletion, which takes the tab off a file that is sitting one folder away
//! (ADR-033 §14).
//!
//! So a removal is not announced the moment it is seen. It is held, and every
//! later delivery is a chance to answer it: the file's id somewhere else, or
//! its bytes in the window that named them. Nothing answers by the deadline
//! and the removal is announced as it always was.
//!
//! Only a removal something could still answer is worth holding. With no id on
//! record and no digest of what the tab last read, no later delivery can say
//! anything the first one did not, and the wait would be latency for a
//! foregone conclusion.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::hash::Sha256Digest;
use crate::notes::guard::DiskState;
use crate::notes::identity::{
    classify_delete, classify_delete_by_content, DeleteVerdict, FileIdentity,
};

/// How long a removal is held before it is announced.
///
/// Twice the 500 ms debounce both watchers run on, so the delivery that would
/// carry the other half of a rename has a full window of its own to arrive in
/// after the one that carried the first half closed. Longer would leave a tab
/// writing into a file the user deleted; the wait is what a deletion costs
/// before the tab hears about it, and it stays well inside the window an
/// external change is allowed to take.
pub const DEFAULT_HOLD_WINDOW: Duration = Duration::from_millis(1000);

/// A removal waiting to be answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldRemoval {
    /// The note whose tab is on the file.
    pub note_id: String,
    /// The path that went empty.
    pub path: PathBuf,
    /// What the filesystem called the file, when anything did.
    pub identity: Option<FileIdentity>,
    /// What the tab last read from it, when it has read it.
    pub last: Option<DiskState>,
    /// The paths the delivery that carried the emptying named. The other half
    /// of a rename can be in it, and a later delivery is judged against both.
    pub batch: Vec<PathBuf>,
}

impl HeldRemoval {
    /// Whether a later delivery could answer this at all.
    ///
    /// An id that recognises its file elsewhere, or the bytes the tab last
    /// read: one of the two has to be on record, or every later delivery says
    /// exactly what the first one did.
    pub fn can_be_answered(&self) -> bool {
        self.identity.as_ref().is_some_and(FileIdentity::is_durable) || self.last.is_some()
    }

    /// Every path that could be holding the file now: the delivery the
    /// emptying arrived in, and the one being answered with. The path that
    /// went empty is never a candidate for itself.
    pub fn candidates(&self, batch: &[PathBuf]) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = Vec::new();
        for candidate in self.batch.iter().chain(batch.iter()) {
            if candidate == &self.path || out.contains(candidate) {
                continue;
            }
            out.push(candidate.clone());
        }
        out
    }
}

/// The removals this watcher is holding.
///
/// One per watcher thread: the hold outlives the batch it started in, which is
/// the whole point of it.
#[derive(Debug)]
pub struct PendingRemovals {
    window: Duration,
    held: Vec<(HeldRemoval, Instant)>,
}

impl PendingRemovals {
    /// Holds each removal for `window` before announcing it.
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            held: Vec::new(),
        }
    }

    /// How long a removal is held.
    pub fn window(&self) -> Duration {
        self.window
    }

    /// Whether anything is waiting.
    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }

    /// Whether this note's removal is waiting to be answered.
    pub fn holds(&self, note_id: &str) -> bool {
        self.held.iter().any(|(held, _)| held.note_id == note_id)
    }

    /// The removal this note is waiting on.
    pub fn held(&self, note_id: &str) -> Option<&HeldRemoval> {
        self.held
            .iter()
            .find(|(held, _)| held.note_id == note_id)
            .map(|(held, _)| held)
    }

    /// Every note with a removal waiting, oldest first.
    pub fn note_ids(&self) -> Vec<String> {
        self.held
            .iter()
            .map(|(held, _)| held.note_id.clone())
            .collect()
    }

    /// When the first removal is due to be announced, when one is waiting.
    ///
    /// The watcher thread waits on this rather than on the next event, so a
    /// removal is announced on its own schedule instead of when some unrelated
    /// change happens to arrive.
    pub fn deadline(&self) -> Option<Instant> {
        self.held.iter().map(|(_, at)| *at + self.window).min()
    }

    /// Holds `removal` from `now`, and answers whether it is held.
    ///
    /// `false` for a removal nothing could ever answer, which is the caller's
    /// to announce straight away. A note already waiting keeps the removal it
    /// is waiting on: the delivery that raised the second one is a chance to
    /// answer the first, not a new question.
    pub fn hold(&mut self, removal: HeldRemoval, now: Instant) -> bool {
        if !removal.can_be_answered() {
            return false;
        }
        if self.holds(&removal.note_id) {
            return true;
        }
        self.held.push((removal, now));
        true
    }

    /// Drops the removal this note was waiting on, because the file is back at
    /// the path it left.
    pub fn forget(&mut self, note_id: &str) -> Option<HeldRemoval> {
        let at = self
            .held
            .iter()
            .position(|(held, _)| held.note_id == note_id)?;
        Some(self.held.remove(at).0)
    }

    /// Where the file went, when this delivery says: the path carrying its id,
    /// or the path holding the bytes the tab last read from it.
    ///
    /// `probed` is what the filesystem calls each candidate; `digests` is what
    /// the candidates named by a delivery hold. Only the ids are read from the
    /// folder the file left, never the bytes: matching on content against a
    /// folder listing would land a tab on any note that happens to hold the
    /// same text ([`classify_delete_by_content`]).
    ///
    /// The removal stops being held the moment it is answered, so a hold that
    /// resolves is never announced by [`Self::expired`] afterwards.
    pub fn resolve(
        &mut self,
        note_id: &str,
        probed: &[(PathBuf, FileIdentity)],
        digests: &[(PathBuf, Sha256Digest)],
    ) -> Option<PathBuf> {
        let held = self.held(note_id)?;
        let by_id = held
            .identity
            .as_ref()
            .map(|before| classify_delete(before, probed));
        let to = match by_id {
            Some(DeleteVerdict::Moved(to)) => Some(to),
            _ => match held.last {
                Some(last) => match classify_delete_by_content(&last.hash, digests) {
                    DeleteVerdict::Moved(to) => Some(to),
                    DeleteVerdict::Removed | DeleteVerdict::ExternalModification => None,
                },
                None => None,
            },
        }?;
        self.forget(note_id);
        Some(to)
    }

    /// The removals nothing answered in time, which are the ones to announce.
    ///
    /// Called after [`Self::resolve`] has had this delivery: a removal the
    /// delivery answers is a move, however long it waited for the answer.
    pub fn expired(&mut self, now: Instant) -> Vec<HeldRemoval> {
        let window = self.window;
        let mut out = Vec::new();
        self.held.retain(|(held, at)| {
            if now.saturating_duration_since(*at) < window {
                return true;
            }
            out.push(held.clone());
            false
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::sha256_bytes;

    fn inode(ino: u64) -> FileIdentity {
        FileIdentity::Inode {
            dev: 1,
            ino,
            birth_ns: None,
        }
    }

    fn state_of(bytes: &[u8]) -> DiskState {
        DiskState {
            hash: sha256_bytes(bytes),
            size: bytes.len() as u64,
            mtime: None,
        }
    }

    fn vanished(
        path: &str,
        identity: Option<FileIdentity>,
        last: Option<DiskState>,
    ) -> HeldRemoval {
        HeldRemoval {
            note_id: "note-1".to_string(),
            path: PathBuf::from(path),
            identity,
            last,
            batch: vec![PathBuf::from(path)],
        }
    }

    #[test]
    fn a_file_found_again_by_its_id_in_the_next_delivery_is_a_move() {
        let mut pending = PendingRemovals::new(Duration::from_millis(1000));
        let now = Instant::now();
        assert!(pending.hold(vanished("/notes/a.md", Some(inode(7)), None), now));

        let next = vec![PathBuf::from("/notes/archive/a.md")];
        let candidates = pending.held("note-1").expect("held").candidates(&next);
        let probed = vec![(PathBuf::from("/notes/archive/a.md"), inode(7))];
        assert_eq!(
            pending.resolve("note-1", &probed, &[]),
            Some(PathBuf::from("/notes/archive/a.md"))
        );
        assert!(candidates.contains(&PathBuf::from("/notes/archive/a.md")));
        assert!(
            !pending.holds("note-1"),
            "an answered removal stops waiting"
        );
        assert!(
            pending.expired(now + Duration::from_secs(10)).is_empty(),
            "a move must not be announced as a deletion once its deadline passes"
        );
    }

    #[test]
    fn a_file_found_again_by_its_bytes_in_the_next_delivery_is_a_move() {
        let mut pending = PendingRemovals::new(Duration::from_millis(1000));
        let now = Instant::now();
        // The id on record was retired by a rewrite nobody reported, so no
        // candidate carries it and the bytes are what is left.
        assert!(pending.hold(
            vanished("/notes/a.md", Some(inode(7)), Some(state_of(b"text"))),
            now
        ));

        let probed = vec![(PathBuf::from("/notes/renamed.md"), inode(9))];
        let digests = vec![(PathBuf::from("/notes/renamed.md"), sha256_bytes(b"text"))];
        assert_eq!(
            pending.resolve("note-1", &probed, &digests),
            Some(PathBuf::from("/notes/renamed.md"))
        );
    }

    #[test]
    fn a_removal_nothing_answers_is_announced_when_its_deadline_passes() {
        let mut pending = PendingRemovals::new(Duration::from_millis(1000));
        let now = Instant::now();
        assert!(pending.hold(vanished("/notes/a.md", Some(inode(7)), None), now));

        // A delivery naming other files answers nothing, and the removal keeps
        // waiting rather than being announced early.
        let other = vec![PathBuf::from("/notes/b.md")];
        let probed = vec![(PathBuf::from("/notes/b.md"), inode(8))];
        assert_eq!(pending.resolve("note-1", &probed, &[]), None);
        assert!(
            pending
                .held("note-1")
                .expect("held")
                .candidates(&other)
                .len()
                == 1
        );
        assert!(pending.expired(now + Duration::from_millis(999)).is_empty());

        let announced = pending.expired(now + Duration::from_millis(1000));
        assert_eq!(announced.len(), 1);
        assert_eq!(announced[0].path, PathBuf::from("/notes/a.md"));
        assert!(pending.is_empty());
    }

    #[test]
    fn a_rewrite_and_a_rename_split_across_two_deliveries_is_still_a_move() {
        // The window that emptied the path carried the rewrite's temp file as
        // well; the rename landed in the next one. Neither delivery holds both
        // halves, and the id on record is the one the rewrite retired.
        let mut pending = PendingRemovals::new(Duration::from_millis(1000));
        let now = Instant::now();
        let first = HeldRemoval {
            note_id: "note-1".to_string(),
            path: PathBuf::from("/notes/a.md"),
            identity: Some(inode(7)),
            last: Some(state_of(b"text worth keeping")),
            batch: vec![
                PathBuf::from("/notes/a.md"),
                PathBuf::from("/notes/a.other-program-tmp"),
            ],
        };
        assert!(pending.hold(first, now));

        let second = vec![PathBuf::from("/notes/renamed.md")];
        let candidates = pending.held("note-1").expect("held").candidates(&second);
        assert_eq!(
            candidates,
            vec![
                PathBuf::from("/notes/a.other-program-tmp"),
                PathBuf::from("/notes/renamed.md")
            ],
            "both deliveries are searched, and never the path that went empty"
        );
        let digests = vec![(
            PathBuf::from("/notes/renamed.md"),
            sha256_bytes(b"text worth keeping"),
        )];
        assert_eq!(
            pending.resolve("note-1", &[], &digests),
            Some(PathBuf::from("/notes/renamed.md"))
        );
    }

    #[test]
    fn a_removal_nothing_could_ever_answer_is_not_held() {
        let mut pending = PendingRemovals::new(Duration::from_millis(1000));
        assert!(!pending.hold(vanished("/notes/a.md", None, None), Instant::now()));
        assert!(pending.is_empty(), "waiting would only add latency");
        assert_eq!(pending.deadline(), None);
    }

    #[test]
    fn an_id_that_cannot_recognise_its_file_elsewhere_is_not_worth_waiting_on() {
        let mut pending = PendingRemovals::new(Duration::from_millis(1000));
        let fallback = FileIdentity::Fallback {
            path: "/notes/a.md".to_string(),
            size: 4,
            mtime_ms: Some(1),
            hash: sha256_bytes(b"text"),
        };
        let removal = vanished("/notes/a.md", Some(fallback), None);
        assert!(!pending.hold(removal, Instant::now()));
    }

    #[test]
    fn a_second_report_of_one_removal_keeps_the_delivery_it_arrived_in() {
        let mut pending = PendingRemovals::new(Duration::from_millis(1000));
        let now = Instant::now();
        assert!(pending.hold(vanished("/notes/a.md", Some(inode(7)), None), now));
        let mut again = vanished("/notes/a.md", Some(inode(7)), None);
        again.batch = vec![PathBuf::from("/notes/b.md")];
        assert!(pending.hold(again, now + Duration::from_millis(400)));

        assert_eq!(pending.note_ids(), vec!["note-1".to_string()]);
        assert_eq!(
            pending.held("note-1").expect("held").batch,
            vec![PathBuf::from("/notes/a.md")]
        );
        assert_eq!(
            pending.deadline(),
            Some(now + Duration::from_millis(1000)),
            "the deadline is the first sighting's, not the last"
        );
    }

    #[test]
    fn a_file_back_at_its_own_path_is_forgotten_rather_than_announced() {
        let mut pending = PendingRemovals::new(Duration::from_millis(1000));
        let now = Instant::now();
        pending.hold(vanished("/notes/a.md", Some(inode(7)), None), now);
        assert_eq!(
            pending.forget("note-1").map(|held| held.path),
            Some(PathBuf::from("/notes/a.md"))
        );
        assert!(pending.expired(now + Duration::from_secs(10)).is_empty());
    }

    #[test]
    fn the_deadline_is_the_first_removal_waiting() {
        let mut pending = PendingRemovals::new(Duration::from_millis(1000));
        let now = Instant::now();
        pending.hold(vanished("/notes/a.md", Some(inode(7)), None), now);
        let mut second = vanished("/notes/b.md", Some(inode(8)), None);
        second.note_id = "note-2".to_string();
        pending.hold(second, now + Duration::from_millis(300));

        assert_eq!(pending.deadline(), Some(now + Duration::from_millis(1000)));
        let announced = pending.expired(now + Duration::from_millis(1100));
        assert_eq!(announced.len(), 1);
        assert_eq!(announced[0].note_id, "note-1");
        assert!(pending.holds("note-2"));
    }
}
