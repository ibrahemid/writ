//! Work that runs one at a time per key, in the order it was asked for.
//!
//! A command that once ran on the main thread ran after every command sent
//! before it, so two saves of one note could never pass each other. Off the
//! main thread nothing orders them, and a save that finished last with the
//! older text would leave that text on disk (ADR-047). A [`KeyedQueue`] gives
//! each key that order back and nothing more: work under different keys never
//! waits on each other.
//!
//! The place in line is taken without blocking ([`KeyedQueue::ticket`]) and
//! waited out later ([`Ticket::wait`]), so a command can take its place on the
//! thread that received it and wait on the thread that does the work.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

/// One queue per key.
///
/// Cloning shares the queues. A key that has nothing waiting holds no memory.
#[derive(Clone, Default)]
pub struct KeyedQueue {
    shared: Arc<Shared>,
}

#[derive(Default)]
struct Shared {
    lanes: Mutex<HashMap<String, Lane>>,
    turned: Condvar,
}

/// The places handed out under one key.
#[derive(Default)]
struct Lane {
    /// The place the next ticket gets.
    next: u64,
    /// The place whose turn it is.
    serving: u64,
    /// Places whose ticket was dropped before its turn came.
    abandoned: BTreeSet<u64>,
}

impl KeyedQueue {
    /// An empty set of queues.
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes the next place in `key`'s queue. Never blocks.
    pub fn ticket(&self, key: &str) -> Ticket {
        let mut lanes = lock(&self.shared.lanes);
        let lane = lanes.entry(key.to_string()).or_default();
        let place = lane.next;
        lane.next += 1;
        Ticket {
            shared: Arc::clone(&self.shared),
            key: key.to_string(),
            place,
            done: false,
        }
    }

    /// Takes a place in `key`'s queue and waits for it.
    pub fn turn(&self, key: &str) -> Turn {
        self.ticket(key).wait()
    }

    /// How many tickets under `key` have not finished, the running one
    /// included.
    pub fn outstanding(&self, key: &str) -> u64 {
        lock(&self.shared.lanes).get(key).map_or(0, |lane| {
            lane.next - lane.serving - lane.abandoned.len() as u64
        })
    }
}

/// A place in one key's queue, not yet waited for.
///
/// Dropping it gives the place up, so the tickets behind it do not wait for a
/// turn nobody will take.
#[must_use = "a ticket holds a place in line; wait for it or drop it"]
pub struct Ticket {
    shared: Arc<Shared>,
    key: String,
    place: u64,
    done: bool,
}

impl Ticket {
    /// The key this ticket queues under.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Blocks until every ticket taken before this one under the same key has
    /// finished or been given up.
    pub fn wait(mut self) -> Turn {
        let mut lanes = lock(&self.shared.lanes);
        while lanes
            .get(&self.key)
            .is_some_and(|lane| lane.serving != self.place)
        {
            lanes = self
                .shared
                .turned
                .wait(lanes)
                .unwrap_or_else(PoisonError::into_inner);
        }
        drop(lanes);
        self.done = true;
        Turn {
            shared: Arc::clone(&self.shared),
            key: std::mem::take(&mut self.key),
        }
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        let mut lanes = lock(&self.shared.lanes);
        let Some(lane) = lanes.get_mut(&self.key) else {
            return;
        };
        if lane.serving == self.place {
            advance(&mut lanes, &self.key);
            self.shared.turned.notify_all();
        } else {
            lane.abandoned.insert(self.place);
        }
    }
}

/// The running turn under one key. The next ticket's turn starts when this is
/// dropped, including by a panic unwinding through its holder.
#[must_use = "dropping a turn ends it"]
pub struct Turn {
    shared: Arc<Shared>,
    key: String,
}

impl Turn {
    /// The key this turn runs under.
    pub fn key(&self) -> &str {
        &self.key
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        let mut lanes = lock(&self.shared.lanes);
        advance(&mut lanes, &self.key);
        drop(lanes);
        self.shared.turned.notify_all();
    }
}

/// Moves `key`'s queue past the running place and every given-up place after
/// it, and forgets the key once nothing is left in it.
fn advance(lanes: &mut HashMap<String, Lane>, key: &str) {
    let Some(lane) = lanes.get_mut(key) else {
        return;
    };
    lane.serving += 1;
    while lane.abandoned.remove(&lane.serving) {
        lane.serving += 1;
    }
    if lane.serving == lane.next {
        lanes.remove(key);
    }
}

/// The lanes hold no state a panic can leave half-written: every update is a
/// counter step or a set insert, done before anything else runs.
fn lock(lanes: &Mutex<HashMap<String, Lane>>) -> MutexGuard<'_, HashMap<String, Lane>> {
    lanes.lock().unwrap_or_else(PoisonError::into_inner)
}
