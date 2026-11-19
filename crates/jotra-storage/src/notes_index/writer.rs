//! The writer connection, and the order writes take it in.

use std::ops::{Deref, DerefMut};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError, TryLockError};

/// The one writer connection, shared by the writes a person waits on and a
/// walk that takes it once per file.
///
/// A lock gives no turn to the thread that has waited longest, and a walk
/// lets go and asks again at once, so a save or Cmd+N waiting beside it could
/// lose the writer to the walk file after file. Every write but a walk's
/// therefore counts itself in before it waits ([`Writer::lock`]), and a walk
/// takes the writer only while that count is zero ([`Writer::lock_for_walk`]).
/// A waiting write is held up by the one thing each running walk is writing
/// when it arrives, and by any write that arrived before it.
pub(crate) struct Writer<T> {
    slot: Mutex<T>,
    /// Writes other than a walk's that are waiting for the slot or holding it.
    ahead: Mutex<usize>,
    /// Told when `ahead` falls to zero.
    clear: Condvar,
}

/// The writer, held by a write that goes ahead of a walk. It lets go of the
/// writer before it stops counting, so a walk told to go finds it free.
pub(crate) struct WriteGuard<'a, T> {
    slot: MutexGuard<'a, T>,
    _ahead: Ahead<'a, T>,
}

/// One count in a writer's `ahead`, taken back when dropped, a panic included.
struct Ahead<'a, T> {
    writer: &'a Writer<T>,
}

impl<T> Writer<T> {
    pub(crate) fn new(value: T) -> Self {
        Self {
            slot: Mutex::new(value),
            ahead: Mutex::new(0),
            clear: Condvar::new(),
        }
    }

    /// Takes the writer ahead of any walk: a save, Cmd+N, the notes watcher, a
    /// rename, a folder move.
    pub(crate) fn lock(&self) -> WriteGuard<'_, T> {
        let ahead = Ahead::count_in(self);
        WriteGuard {
            slot: recover(self.slot.lock()),
            _ahead: ahead,
        }
    }

    /// Takes the writer for one of a walk's writes, once no other write is
    /// waiting for it.
    pub(crate) fn lock_for_walk(&self) -> MutexGuard<'_, T> {
        let ahead = recover(self.ahead.lock());
        drop(recover(self.clear.wait_while(ahead, |ahead| *ahead > 0)));
        recover(self.slot.lock())
    }

    /// The writer if nothing holds it now.
    pub(crate) fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        match self.slot.try_lock() {
            Ok(slot) => Some(slot),
            Err(TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
            Err(TryLockError::WouldBlock) => None,
        }
    }

    /// How many writes are waiting for the writer or holding it, a walk's
    /// aside.
    #[cfg(test)]
    pub(crate) fn ahead(&self) -> usize {
        *recover(self.ahead.lock())
    }
}

impl<'a, T> Ahead<'a, T> {
    fn count_in(writer: &'a Writer<T>) -> Self {
        *recover(writer.ahead.lock()) += 1;
        Self { writer }
    }
}

impl<T> Drop for Ahead<'_, T> {
    fn drop(&mut self) {
        let mut ahead = recover(self.writer.ahead.lock());
        *ahead = ahead.saturating_sub(1);
        if *ahead == 0 {
            self.writer.clear.notify_all();
        }
    }
}

impl<T> Deref for WriteGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.slot
    }
}

impl<T> DerefMut for WriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.slot
    }
}

/// A poisoned lock is recovered rather than cascaded, as the store does for
/// every lock it holds: the index is derived data that a walk rebuilds.
fn recover<G>(result: Result<G, PoisonError<G>>) -> G {
    result.unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::{Ahead, Writer};

    /// Turns a walk takes after the first, one straight after another.
    const WALK_TURNS: usize = 50;

    /// A write that has counted itself in and not yet reached the writer is
    /// the moment a walk letting go and asking again would win: the writer is
    /// free. The walk does not take it until that write is done.
    #[test]
    fn a_walk_never_takes_the_writer_while_a_write_is_counted_in() {
        let writer = Arc::new(Writer::new(Vec::<&str>::new()));
        let counted_in = Ahead::count_in(&writer);
        let walk = {
            let writer = Arc::clone(&writer);
            std::thread::spawn(move || writer.lock_for_walk().push("walk"))
        };
        std::thread::sleep(Duration::from_millis(100));

        assert!(
            !walk.is_finished(),
            "the walk took the writer while a write was counted in"
        );
        writer
            .try_lock()
            .expect("the writer is free while the walk waits")
            .push("save");
        drop(counted_in);
        walk.join().expect("walk thread");
        assert_eq!(*writer.lock(), ["save", "walk"]);
    }

    /// A walk lets go of the writer and asks for it again at once, file after
    /// file. A save that started waiting during one file gets the writer
    /// before the walk's next: the walk's turn is not lost, it comes after.
    #[test]
    fn a_write_waiting_beside_a_walk_goes_before_the_walks_next_file() {
        let writer = Arc::new(Writer::new(Vec::<String>::new()));

        let mut first = writer.lock_for_walk();
        let save = {
            let writer = Arc::clone(&writer);
            std::thread::spawn(move || writer.lock().push("save".to_string()))
        };
        while writer.ahead() == 0 {
            std::thread::yield_now();
        }
        first.push("walk 0".to_string());
        drop(first);
        for turn in 1..=WALK_TURNS {
            writer.lock_for_walk().push(format!("walk {turn}"));
        }
        save.join().expect("save thread");

        let order = writer.lock().clone();
        let save_at = order
            .iter()
            .position(|entry| entry == "save")
            .expect("the save wrote");
        assert_eq!(
            save_at,
            1,
            "the save waited through {} of the walk's files: {order:?}",
            save_at - 1
        );
        assert_eq!(order.len(), WALK_TURNS + 2, "every turn wrote once");
    }

    /// Writes that are not a walk's never wait for each other's count: two in
    /// a row each take the writer, and a walk waiting behind them goes once
    /// both are done.
    #[test]
    fn a_walk_goes_once_every_write_ahead_of_it_is_done() {
        let writer = Arc::new(Writer::new(0usize));
        let first = writer.lock();
        let walk = {
            let writer = Arc::clone(&writer);
            std::thread::spawn(move || {
                let mut slot = writer.lock_for_walk();
                *slot += 10;
                *slot
            })
        };
        let second = {
            let writer = Arc::clone(&writer);
            std::thread::spawn(move || *writer.lock() += 1)
        };
        while writer.ahead() < 2 {
            std::thread::yield_now();
        }
        drop(first);
        second.join().expect("second write");

        assert_eq!(walk.join().expect("walk"), 11, "the walk went last");
        assert_eq!(writer.ahead(), 0);
    }

    /// A write that panics while it holds the writer still stops counting, so
    /// the walk is not left waiting for a write that will never finish.
    #[test]
    fn a_write_that_panics_does_not_hold_the_walk_up() {
        let writer = Arc::new(Writer::new(0usize));
        let panicked = {
            let writer = Arc::clone(&writer);
            std::thread::spawn(move || {
                let _held = writer.lock();
                panic!("a write failed while holding the writer");
            })
        };
        assert!(panicked.join().is_err());

        assert_eq!(writer.ahead(), 0);
        *writer.lock_for_walk() += 1;
        assert_eq!(*writer.lock(), 1);
    }

    #[test]
    fn try_lock_answers_without_waiting() {
        let writer = Writer::new(0usize);
        let held = writer.lock();
        assert!(writer.try_lock().is_none());
        drop(held);
        assert!(writer.try_lock().is_some());
    }
}
