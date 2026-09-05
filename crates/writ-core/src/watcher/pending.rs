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
//! later delivery is a chance to answer it with the file's id somewhere else.
//! Nothing answers by the deadline and the removal is announced as it always
//! was.
//!
//! The id is the only thing a delivery answers with. Bytes say nothing about
//! where a file went: two notes can hold the same text, so a path that matches
//! on content is a path that might be somebody else's file, and the next save
//! writes over it (ADR-033 §12).
//!
//! Only a removal something could still answer is worth holding. With no id on
//! record that could recognise the file elsewhere, no later delivery can say
//! anything the first one did not, and the wait would be latency for a
//! foregone conclusion.
//!
//! The hold is a window in which nobody knows yet whether the note still has a
//! file, so nothing may write to the path it left. [`RemovalHolds`] is what
//! makes that enforceable from outside the watcher thread: a save asks whether
//! the note is held, waits for the answer, and then writes at the new path or
//! refuses (ADR-033 §14).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::notes::identity::{classify_delete, DeleteVerdict, FileIdentity};

/// How long a removal is held, for a watcher that delivers on `debounce`.
///
/// Twice the window it delivers on. A debounce window closes on a deadline set
/// by its first event, so the delivery that would carry the other half of a
/// rename can be a full window behind the one that carried the first half, and
/// a hold of one window would expire on that boundary. Longer would leave a tab
/// writing into a file that was deleted; the wait is what a deletion costs
/// before the tab hears about it.
pub fn hold_window(debounce: Duration) -> Duration {
    debounce * 2
}

/// What a held removal turned out to be, for anyone waiting on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoldAnswer {
    /// The file was found at this path, and the note's record names it now.
    Moved(PathBuf),
    /// Nothing answered by the deadline. The file is gone and the record says
    /// so, which is what makes a write to the path it left a refusal.
    Removed,
    /// The same file is back where it left, so nothing happened to it and the
    /// removal was never announced.
    Returned,
}

/// The removals a watcher is holding, readable from any thread.
///
/// A hold is the window in which the answer is not known yet, and the record
/// is deliberately left saying what it said before ([`PendingRemovals`]). That
/// is only safe while nothing writes to the path in the meantime: a save
/// landing inside the window recreates a file the person deleted, and against
/// a rename it puts a second file where there was one and the move is never
/// announced. So a save asks here first and waits.
///
/// **An answer is published only once the record agrees with it.** A waiter
/// released before the row has moved reads the old path out of the row and
/// writes there, which is the failure this exists to stop. The publisher is
/// therefore the caller that applied the record change, never [`Self::hold`]'s
/// counterpart inside `PendingRemovals`.
///
/// Two watchers can hold one note, each on its own window. The later deadline
/// wins, so a wait cannot end while a watcher is still holding, and the first
/// answer is the answer: both are watching the same file and the second says
/// what the first did.
#[derive(Debug, Default)]
pub struct RemovalHolds {
    held: Mutex<HashMap<String, Hold>>,
    answered: Condvar,
}

/// One note's hold: when a waiter gives up, the answer when there is one, and
/// how many waiters are still to read it.
#[derive(Debug)]
struct Hold {
    answer_by: Instant,
    answer: Option<HoldAnswer>,
    waiting: usize,
}

impl RemovalHolds {
    /// Nothing held.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records that `note_id`'s removal is being held, and that a waiter
    /// should give up at `answer_by`.
    ///
    /// The later `answer_by` wins, so the second watcher to hold the same note
    /// extends the wait rather than cutting it short. A note held again after
    /// an answer nobody read starts clean: the answer described the removal
    /// before it, not this one.
    pub fn hold(&self, note_id: &str, answer_by: Instant) {
        let mut held = self.lock();
        match held.get_mut(note_id) {
            Some(hold) => {
                hold.answer_by = hold.answer_by.max(answer_by);
                hold.answer = None;
            }
            None => {
                held.insert(
                    note_id.to_string(),
                    Hold {
                        answer_by,
                        answer: None,
                        waiting: 0,
                    },
                );
            }
        }
    }

    /// Publishes what the removal turned out to be, waking whoever is waiting.
    ///
    /// Call it after the record has been made to agree: the row moved, or the
    /// note marked removed. Nothing held for `note_id` is not a mistake — a
    /// move can be classified in the delivery it arrived in, with no hold ever
    /// taken — and answers nothing.
    pub fn answer(&self, note_id: &str, answer: HoldAnswer) {
        let mut held = self.lock();
        let Some(hold) = held.get_mut(note_id) else {
            return;
        };
        if hold.waiting == 0 {
            held.remove(note_id);
            return;
        }
        hold.answer = Some(answer);
        drop(held);
        self.answered.notify_all();
    }

    /// Waits for `note_id`'s held removal to be answered.
    ///
    /// `None` when the note is not held at all, which is every ordinary save,
    /// and when the deadline passed with no answer — a watcher that died
    /// holding one, where the caller is no worse off than it was before the
    /// hold existed. The wait is bounded by the deadline the hold was taken
    /// with, never by a poll.
    pub fn wait_for_answer(&self, note_id: &str) -> Option<HoldAnswer> {
        let mut held = self.lock();
        let answer_by = held.get(note_id)?.answer_by;
        held.get_mut(note_id)?.waiting += 1;
        let answer = loop {
            match held.get_mut(note_id) {
                Some(hold) => {
                    if let Some(answer) = hold.answer.clone() {
                        break Some(answer);
                    }
                }
                // Answered and dropped by a waiter that read it first.
                None => break None,
            }
            let left = answer_by.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break None;
            }
            held = self
                .answered
                .wait_timeout(held, left)
                .unwrap_or_else(|poisoned| {
                    tracing::error!(
                        location = "watcher::pending::wait_for_answer",
                        "recovered poisoned mutex"
                    );
                    poisoned.into_inner()
                })
                .0;
        };
        if let Some(hold) = held.get_mut(note_id) {
            hold.waiting -= 1;
            if hold.waiting == 0 && hold.answer.is_some() {
                held.remove(note_id);
            }
        }
        answer
    }

    /// Whether anything is held for `note_id`, for a caller that cannot wait.
    pub fn holds(&self, note_id: &str) -> bool {
        self.lock().contains_key(note_id)
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, Hold>> {
        self.held.lock().unwrap_or_else(|poisoned| {
            tracing::error!(
                location = "watcher::pending::RemovalHolds",
                "recovered poisoned mutex"
            );
            poisoned.into_inner()
        })
    }
}

/// A removal waiting to be answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldRemoval {
    /// The note whose tab is on the file.
    pub note_id: String,
    /// The path that went empty.
    pub path: PathBuf,
    /// What the filesystem called the file, when anything did.
    pub identity: Option<FileIdentity>,
    /// The paths the delivery that carried the emptying named. The other half
    /// of a rename can be in it, and a later delivery is judged against both.
    pub batch: Vec<PathBuf>,
}

impl HeldRemoval {
    /// Whether a later delivery could answer this at all.
    ///
    /// An id that recognises its file elsewhere has to be on record, or every
    /// later delivery says exactly what the first one did.
    pub fn can_be_answered(&self) -> bool {
        self.identity.as_ref().is_some_and(FileIdentity::is_durable)
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
    holds: Arc<RemovalHolds>,
}

impl PendingRemovals {
    /// Holds each removal for `window` before announcing it, where nothing
    /// outside this watcher can see the holds.
    pub fn new(window: Duration) -> Self {
        Self::publishing_to(window, Arc::new(RemovalHolds::new()))
    }

    /// The same, with the holds published where a save can wait on them.
    ///
    /// Every watcher in a process shares one [`RemovalHolds`], because a save
    /// asks about a note and not about a watcher.
    pub fn publishing_to(window: Duration, holds: Arc<RemovalHolds>) -> Self {
        Self {
            window,
            held: Vec::new(),
            holds,
        }
    }

    /// Where this watcher publishes its holds.
    pub fn holds(&self) -> &Arc<RemovalHolds> {
        &self.holds
    }

    /// When a waiter on a removal held at `now` gives up.
    ///
    /// The deadline the removal is announced on, plus half a window for the
    /// announcing itself: the watcher thread wakes on the deadline, applies
    /// the removal to the record and publishes the answer, and a waiter that
    /// gave up an instant before that would write to the path in the moment
    /// between.
    fn answer_by(&self, now: Instant) -> Instant {
        now + self.window + self.window / 2
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
    pub fn is_holding(&self, note_id: &str) -> bool {
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
        // Published before the early return as well: the note is held either
        // way, and the second watcher to hold it is what says how long a save
        // has to wait.
        self.holds.hold(&removal.note_id, self.answer_by(now));
        if self.is_holding(&removal.note_id) {
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

    /// The path this delivery says the file went to, when one carries its id.
    ///
    /// `probed` is what the filesystem calls each candidate. The id is the only
    /// evidence a move is followed on, here as everywhere else: a candidate
    /// holding the same bytes is a candidate that might be a different note
    /// (ADR-033 §12).
    ///
    /// The removal stops being held the moment it is answered, so a hold that
    /// resolves is never announced by [`Self::expired`] afterwards.
    pub fn resolve(
        &mut self,
        note_id: &str,
        probed: &[(PathBuf, FileIdentity)],
    ) -> Option<PathBuf> {
        let held = self.held(note_id)?;
        let to = match held
            .identity
            .as_ref()
            .map(|before| classify_delete(before, probed))
        {
            Some(DeleteVerdict::Moved(to)) => to,
            _ => return None,
        };
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

    fn vanished(path: &str, identity: Option<FileIdentity>) -> HeldRemoval {
        HeldRemoval {
            note_id: "note-1".to_string(),
            path: PathBuf::from(path),
            identity,
            batch: vec![PathBuf::from(path)],
        }
    }

    #[test]
    fn a_note_nothing_holds_is_not_waited_on() {
        // Every ordinary save asks this question and has to get on with it.
        let holds = RemovalHolds::new();
        assert!(holds.wait_for_answer("note-1").is_none());
        assert!(!holds.holds("note-1"));
    }

    #[test]
    fn a_wait_ends_on_the_answer() {
        let holds = Arc::new(RemovalHolds::new());
        holds.hold("note-1", Instant::now() + Duration::from_secs(30));
        let answering = {
            let holds = Arc::clone(&holds);
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(50));
                holds.answer("note-1", HoldAnswer::Moved(PathBuf::from("/notes/b.md")));
            })
        };
        let asked_at = Instant::now();
        let answer = holds.wait_for_answer("note-1");
        let waited = asked_at.elapsed();
        answering.join().expect("the answering thread");
        assert_eq!(
            answer,
            Some(HoldAnswer::Moved(PathBuf::from("/notes/b.md")))
        );
        // Woken by the answer rather than by the deadline, which is 30s out.
        assert!(waited < Duration::from_secs(5), "waited {waited:?}");
        // The answer was read, so nothing is left for the next save to find.
        assert!(!holds.holds("note-1"));
    }

    #[test]
    fn a_wait_gives_up_at_the_deadline_the_hold_was_taken_with() {
        // A watcher that stopped while holding a removal leaves the caller no
        // worse off than it was before holds existed.
        let holds = RemovalHolds::new();
        holds.hold("note-1", Instant::now() + Duration::from_millis(50));
        let asked_at = Instant::now();
        assert_eq!(holds.wait_for_answer("note-1"), None);
        let waited = asked_at.elapsed();
        assert!(waited >= Duration::from_millis(50), "waited {waited:?}");
        assert!(waited < Duration::from_secs(5), "waited {waited:?}");
    }

    #[test]
    fn the_second_watcher_to_hold_a_note_says_how_long_the_wait_is() {
        // Both watchers can see one file, each on its own window. A wait that
        // ended on the shorter one would let a save through while the other
        // was still holding, which is the whole failure again.
        let holds = RemovalHolds::new();
        let now = Instant::now();
        holds.hold("note-1", now + Duration::from_millis(50));
        holds.hold("note-1", now + Duration::from_secs(30));
        let asked_at = Instant::now();
        let answered = std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(200));
                holds.answer("note-1", HoldAnswer::Removed);
            });
            holds.wait_for_answer("note-1")
        });
        assert_eq!(answered, Some(HoldAnswer::Removed));
        assert!(
            asked_at.elapsed() >= Duration::from_millis(200),
            "the wait ended on the first watcher's deadline"
        );
    }

    #[test]
    fn a_removal_held_by_a_watcher_is_published_for_a_save_to_wait_on() {
        // The registration is `PendingRemovals`' to publish; every answer is
        // published by whoever made the record agree with it.
        let mut pending = PendingRemovals::new(Duration::from_millis(1000));
        let holds = Arc::clone(pending.holds());
        assert!(pending.hold(vanished("/notes/a.md", Some(inode(7))), Instant::now()));
        assert!(holds.holds("note-1"));
        assert!(!holds.holds("note-2"));
    }

    #[test]
    fn a_file_found_again_by_its_id_in_the_next_delivery_is_a_move() {
        let mut pending = PendingRemovals::new(Duration::from_millis(1000));
        let now = Instant::now();
        assert!(pending.hold(vanished("/notes/a.md", Some(inode(7))), now));

        let next = vec![PathBuf::from("/notes/archive/a.md")];
        let candidates = pending.held("note-1").expect("held").candidates(&next);
        let probed = vec![(PathBuf::from("/notes/archive/a.md"), inode(7))];
        assert_eq!(
            pending.resolve("note-1", &probed),
            Some(PathBuf::from("/notes/archive/a.md"))
        );
        assert!(candidates.contains(&PathBuf::from("/notes/archive/a.md")));
        assert!(
            !pending.is_holding("note-1"),
            "an answered removal stops waiting"
        );
        assert!(
            pending.expired(now + Duration::from_secs(10)).is_empty(),
            "a move must not be announced as a deletion once its deadline passes"
        );
    }

    #[test]
    fn a_removal_nothing_answers_is_announced_when_its_deadline_passes() {
        let mut pending = PendingRemovals::new(Duration::from_millis(1000));
        let now = Instant::now();
        assert!(pending.hold(vanished("/notes/a.md", Some(inode(7))), now));

        // A delivery naming other files answers nothing, and the removal keeps
        // waiting rather than being announced early.
        let other = vec![PathBuf::from("/notes/b.md")];
        let probed = vec![(PathBuf::from("/notes/b.md"), inode(8))];
        assert_eq!(pending.resolve("note-1", &probed), None);
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
    fn both_deliveries_are_searched_and_never_the_path_that_went_empty() {
        // The window that emptied the path carried a rewrite's temp file as
        // well; the rename landed in the next one. Neither delivery holds both
        // halves, so a candidate from either is a candidate.
        let mut pending = PendingRemovals::new(Duration::from_millis(1000));
        let first = HeldRemoval {
            note_id: "note-1".to_string(),
            path: PathBuf::from("/notes/a.md"),
            identity: Some(inode(7)),
            batch: vec![
                PathBuf::from("/notes/a.md"),
                PathBuf::from("/notes/a.other-program-tmp"),
            ],
        };
        assert!(pending.hold(first, Instant::now()));

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
        let probed = vec![(PathBuf::from("/notes/renamed.md"), inode(7))];
        assert_eq!(
            pending.resolve("note-1", &probed),
            Some(PathBuf::from("/notes/renamed.md"))
        );
    }

    #[test]
    fn a_removal_nothing_could_ever_answer_is_not_held() {
        let mut pending = PendingRemovals::new(Duration::from_millis(1000));
        assert!(!pending.hold(vanished("/notes/a.md", None), Instant::now()));
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
        let removal = vanished("/notes/a.md", Some(fallback));
        assert!(!pending.hold(removal, Instant::now()));
    }

    #[test]
    fn a_second_report_of_one_removal_keeps_the_delivery_it_arrived_in() {
        let mut pending = PendingRemovals::new(Duration::from_millis(1000));
        let now = Instant::now();
        assert!(pending.hold(vanished("/notes/a.md", Some(inode(7))), now));
        let mut again = vanished("/notes/a.md", Some(inode(7)));
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
        pending.hold(vanished("/notes/a.md", Some(inode(7))), now);
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
        pending.hold(vanished("/notes/a.md", Some(inode(7))), now);
        let mut second = vanished("/notes/b.md", Some(inode(8)));
        second.note_id = "note-2".to_string();
        pending.hold(second, now + Duration::from_millis(300));

        assert_eq!(pending.deadline(), Some(now + Duration::from_millis(1000)));
        let announced = pending.expired(now + Duration::from_millis(1100));
        assert_eq!(announced.len(), 1);
        assert_eq!(announced[0].note_id, "note-1");
        assert!(pending.is_holding("note-2"));
    }
}
