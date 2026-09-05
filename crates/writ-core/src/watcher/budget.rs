//! How much one debounce window is allowed to say.
//!
//! A folder people sync is not a folder that changes one file at a time. A
//! sync client catching up after a laptop was shut, a plugin another editor
//! left running, a `git checkout` over a notes folder: any of them rewrites
//! hundreds of files inside one debounce window, and naming each one costs an
//! IPC message the frontend has to receive, decode and act on.
//!
//! So the watcher gets a budget. Under it, every change is named, which is
//! what makes an ordinary edit arrive with its own path. Over it, the watcher
//! stops naming files and says only that the folder moved, once, and the
//! index walks it instead. The failure this removes is a five-hundred-message
//! burst; the cost is that a large catch-up is reported as a fact about the
//! folder rather than a list of files, which is all the index needed anyway.
//!
//! Policy only: no clock of its own, no filesystem, no events. The caller
//! passes the time and acts on the verdict.

use std::time::{Duration, Instant};

/// Everything one debounce window may emit about the folder, the sweep
/// included.
///
/// This is the whole ceiling and the only number to change. A window spends up
/// to `DEFAULT_EVENTS_PER_WINDOW - 1` naming individual files and the last slot
/// on the sweep that replaces every further change in it, so ten is ten: nine
/// named plus one sweep, never eleven.
///
/// **Per window, not per catch-up.** A catch-up spanning twenty windows can
/// still cost up to twenty times this, minus the sweeps the cooldown swallows
/// (`DEFAULT_SWEEP_COOLDOWN`): around 185 events for a twenty-window catch-up
/// on the default numbers, not 10. A per-catch-up cap is not something a
/// watcher can enforce, because nothing tells it a catch-up has ended — only
/// that no change has arrived for a while, which is the window it already has.
/// Refilling each window is what keeps an ordinary save named while a sync
/// client is still running in the background.
///
/// Nine named leaves room for what a person does — a save, the temp file
/// beside it, a couple of siblings an editor touched — while a sync catch-up is
/// over it on its first window.
///
/// It governs what the watcher says about the *folder*
/// (`WritEvent::NotesChanged` and `WritEvent::NotesSwept`). Telling an open tab
/// its own file changed is outside it, bounded instead by the number of open
/// tabs and deduplicated per batch.
pub const DEFAULT_EVENTS_PER_WINDOW: usize = 10;

/// Changes one window may name individually before it reports a sweep: the
/// ceiling less the slot the sweep itself takes.
pub const DEFAULT_NAMED_PER_WINDOW: usize = DEFAULT_EVENTS_PER_WINDOW - 1;

/// The debounce window the watchers coalesce into.
pub const DEFAULT_WINDOW: Duration = Duration::from_millis(500);

/// How long one sweep stands for.
///
/// A catch-up long enough to span many windows would otherwise report a sweep
/// in each of them, which is the burst again in slower motion. One marker per
/// cooldown bounds a catch-up of any length: a reconcile already walking the
/// folder covers everything that lands while it runs.
pub const DEFAULT_SWEEP_COOLDOWN: Duration = Duration::from_secs(2);

/// What the watcher may report for one change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emission {
    /// Report this change with its own path.
    Name,
    /// Report one sweep: too much moved in this window to name each file.
    Sweep,
    /// Report nothing. A sweep already standing covers this change.
    Drop,
}

/// Tracks what a watcher has already said, so a storm costs a bounded number
/// of events rather than one per file.
///
/// One budget per watcher thread. [`Self::admit`] is called once per change
/// that survived classification — never before, because a write Writ itself
/// made is not a change and must not spend anything.
#[derive(Debug)]
pub struct EmissionBudget {
    named_per_window: usize,
    window: Duration,
    sweep_cooldown: Duration,
    window_started: Option<Instant>,
    named_in_window: usize,
    last_sweep: Option<Instant>,
    dropped_since_sweep: bool,
}

impl Default for EmissionBudget {
    fn default() -> Self {
        Self::new()
    }
}

impl EmissionBudget {
    /// A budget on the defaults every watcher runs.
    pub fn new() -> Self {
        Self::with_limits(
            DEFAULT_NAMED_PER_WINDOW,
            DEFAULT_WINDOW,
            DEFAULT_SWEEP_COOLDOWN,
        )
    }

    /// A budget on limits of the caller's choosing, for tests and for a
    /// watcher whose debouncer runs on a different window.
    pub fn with_limits(
        named_per_window: usize,
        window: Duration,
        sweep_cooldown: Duration,
    ) -> Self {
        Self {
            named_per_window,
            window,
            sweep_cooldown,
            window_started: None,
            named_in_window: 0,
            last_sweep: None,
            dropped_since_sweep: false,
        }
    }

    /// Decides what may be reported for one change observed at `now`.
    ///
    /// The window rolls on the first change after it expires rather than on a
    /// timer, so a quiet folder holds no state and the budget is whole again
    /// the moment anything happens after a pause.
    pub fn admit(&mut self, now: Instant) -> Emission {
        match self.window_started {
            Some(started) if now.duration_since(started) < self.window => {}
            _ => {
                self.window_started = Some(now);
                self.named_in_window = 0;
            }
        }

        if self.named_in_window < self.named_per_window {
            self.named_in_window += 1;
            return Emission::Name;
        }

        let sweep_due = match self.last_sweep {
            None => true,
            Some(last) => now.duration_since(last) >= self.sweep_cooldown,
        };
        if sweep_due {
            self.last_sweep = Some(now);
            self.dropped_since_sweep = false;
            return Emission::Sweep;
        }
        self.dropped_since_sweep = true;
        Emission::Drop
    }

    /// When a sweep is owed for changes this budget dropped, or `None`.
    ///
    /// A change that arrives while a sweep is still standing is dropped,
    /// because the sweep before it already told the index to walk the folder.
    /// That holds only while more changes keep arriving: a burst that ends
    /// inside the cooldown left its last changes covered by a walk that
    /// started before them, and the index would sit stale until something else
    /// happened in the folder — possibly not this session.
    ///
    /// So a dropped change owes a sweep, due when the cooldown that swallowed
    /// it runs out. The caller waits until then and takes it with
    /// [`Self::take_owed_sweep`]. A sweep the budget emits in the meantime
    /// covers those changes and clears the debt.
    pub fn owed_sweep_at(&self) -> Option<Instant> {
        match (self.dropped_since_sweep, self.last_sweep) {
            (true, Some(last)) => Some(last + self.sweep_cooldown),
            _ => None,
        }
    }

    /// Takes the owed sweep once it is due, which spends it: the debt is
    /// cleared and the cooldown starts again, so one silence after a burst
    /// costs exactly one follow-up sweep however many changes it swallowed.
    pub fn take_owed_sweep(&mut self, now: Instant) -> bool {
        match self.owed_sweep_at() {
            Some(due) if now >= due => {
                self.last_sweep = Some(now);
                self.dropped_since_sweep = false;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The verdicts for `count` changes all observed at `now`.
    fn burst(budget: &mut EmissionBudget, count: usize, now: Instant) -> Vec<Emission> {
        (0..count).map(|_| budget.admit(now)).collect()
    }

    fn counts(verdicts: &[Emission]) -> (usize, usize, usize) {
        (
            verdicts.iter().filter(|v| **v == Emission::Name).count(),
            verdicts.iter().filter(|v| **v == Emission::Sweep).count(),
            verdicts.iter().filter(|v| **v == Emission::Drop).count(),
        )
    }

    #[test]
    fn a_folder_changing_one_file_at_a_time_names_every_one() {
        let mut budget = EmissionBudget::new();
        let start = Instant::now();

        for step in 0..20 {
            let now = start + DEFAULT_WINDOW * step;
            assert_eq!(budget.admit(now), Emission::Name);
        }
    }

    #[test]
    fn a_catch_up_rewriting_five_hundred_files_costs_ten_events() {
        // The case this exists for: a sync client pulling a folder that moved
        // on while the laptop was shut. Five hundred files arrive inside one
        // debounce window, and the frontend must not receive five hundred
        // messages about it.
        let mut budget = EmissionBudget::new();
        let verdicts = burst(&mut budget, 500, Instant::now());
        let (named, swept, dropped) = counts(&verdicts);

        assert_eq!(named, DEFAULT_NAMED_PER_WINDOW);
        assert_eq!(swept, 1);
        assert_eq!(dropped, 500 - DEFAULT_NAMED_PER_WINDOW - 1);
        assert_eq!(
            named + swept,
            DEFAULT_EVENTS_PER_WINDOW,
            "the sweep takes the last slot of the window's ceiling, not one past it"
        );
    }

    #[test]
    fn a_catch_up_spanning_many_windows_costs_the_ceiling_once_per_window() {
        // The ceiling is per window. A catch-up long enough to span twenty of
        // them costs twenty windows' worth, less the sweeps the cooldown
        // swallows — not one window's worth for the whole catch-up. Pinned
        // exactly, because the arithmetic is the part that is easy to assume.
        let mut budget = EmissionBudget::new();
        let start = Instant::now();
        let windows = 20u32;
        let (mut named, mut swept) = (0usize, 0usize);

        for window in 0..windows {
            let now = start + DEFAULT_WINDOW * window;
            for verdict in burst(&mut budget, 500, now) {
                match verdict {
                    Emission::Name => named += 1,
                    Emission::Sweep => swept += 1,
                    Emission::Drop => {}
                }
            }
        }

        // Every window refills, so an ordinary save stays named while a sync
        // client is still catching up in the background.
        assert_eq!(named, DEFAULT_NAMED_PER_WINDOW * windows as usize);
        // Ten seconds of storm at one sweep per two-second cooldown.
        assert_eq!(swept, 5);
        assert_eq!(named + swept, 185);
    }

    #[test]
    fn the_sweep_comes_after_the_named_changes_rather_than_instead_of_them() {
        // Order matters: the first few paths in a storm are still named, so a
        // person's own save inside a busy window keeps its path.
        let mut budget = EmissionBudget::new();
        let verdicts = burst(&mut budget, 12, Instant::now());

        assert!(verdicts[..DEFAULT_NAMED_PER_WINDOW]
            .iter()
            .all(|v| *v == Emission::Name));
        assert_eq!(verdicts[DEFAULT_NAMED_PER_WINDOW], Emission::Sweep);
    }

    #[test]
    fn the_next_window_names_changes_again() {
        let mut budget = EmissionBudget::new();
        let start = Instant::now();
        burst(&mut budget, 500, start);

        assert_eq!(budget.admit(start + DEFAULT_WINDOW), Emission::Name);
    }

    #[test]
    fn a_catch_up_spanning_many_windows_sweeps_once_per_cooldown() {
        // A slow catch-up must not turn into one sweep per window, which is
        // the burst again at a lower rate. Twenty windows of 500 files each,
        // ten seconds of filesystem noise.
        let mut budget = EmissionBudget::new();
        let start = Instant::now();
        let mut sweeps = 0;

        for window in 0..20 {
            let now = start + DEFAULT_WINDOW * window;
            for verdict in burst(&mut budget, 500, now) {
                if verdict == Emission::Sweep {
                    sweeps += 1;
                }
            }
        }

        // Ten seconds of storm at one sweep per two-second cooldown.
        assert_eq!(sweeps, 5);
    }

    #[test]
    fn a_window_exactly_at_the_cap_names_everything_and_sweeps_nothing() {
        let mut budget = EmissionBudget::new();
        let verdicts = burst(&mut budget, DEFAULT_NAMED_PER_WINDOW, Instant::now());
        let (named, swept, dropped) = counts(&verdicts);

        assert_eq!((named, swept, dropped), (DEFAULT_NAMED_PER_WINDOW, 0, 0));
    }

    #[test]
    fn a_quiet_folder_holds_no_spent_budget_from_an_old_storm() {
        let mut budget = EmissionBudget::new();
        let start = Instant::now();
        burst(&mut budget, 500, start);

        let after = start + Duration::from_secs(60);
        assert_eq!(budget.admit(after), Emission::Name);
    }

    #[test]
    fn limits_of_the_callers_choosing_are_honoured() {
        let mut budget =
            EmissionBudget::with_limits(1, Duration::from_millis(100), Duration::from_secs(10));
        let start = Instant::now();

        assert_eq!(budget.admit(start), Emission::Name);
        assert_eq!(budget.admit(start), Emission::Sweep);
        assert_eq!(budget.admit(start), Emission::Drop);
    }

    #[test]
    fn a_folder_that_says_nothing_more_owes_no_sweep() {
        let mut budget = EmissionBudget::new();
        let start = Instant::now();

        assert_eq!(budget.owed_sweep_at(), None, "nothing has happened yet");
        burst(&mut budget, DEFAULT_NAMED_PER_WINDOW, start);
        assert_eq!(budget.owed_sweep_at(), None, "every change was named");

        assert_eq!(budget.admit(start), Emission::Sweep);
        assert_eq!(
            budget.owed_sweep_at(),
            None,
            "the sweep covers what it swept"
        );
    }

    #[test]
    fn a_burst_that_ends_inside_the_cooldown_still_gets_its_sweep() {
        // The hole this closes: the changes after the sweep were dropped
        // because a sweep was standing, and then nothing else happened. The
        // walk that sweep started had already passed those files, so without a
        // second sweep the index stayed stale until the folder next changed.
        let mut budget = EmissionBudget::new();
        let start = Instant::now();

        let verdicts = burst(&mut budget, 500, start);
        let (_, swept, dropped) = counts(&verdicts);
        assert_eq!(swept, 1);
        assert!(dropped > 0);

        let due = budget.owed_sweep_at().expect("a sweep is owed");
        assert_eq!(due, start + DEFAULT_SWEEP_COOLDOWN);
        assert!(
            !budget.take_owed_sweep(due - Duration::from_millis(1)),
            "the sweep it would follow is still standing"
        );

        assert!(budget.take_owed_sweep(due));
        assert_eq!(
            budget.owed_sweep_at(),
            None,
            "one silence costs one follow-up sweep, whatever it swallowed"
        );
    }

    #[test]
    fn a_storm_that_keeps_going_owes_nothing_extra_at_the_end() {
        // Sweeps the storm itself paid for cover its own changes; only the
        // ones after the last of them are owed anything.
        let mut budget = EmissionBudget::new();
        let start = Instant::now();
        for tick in 0..10 {
            burst(&mut budget, 50, start + Duration::from_secs(tick));
        }

        let due = budget.owed_sweep_at().expect("a sweep is owed");
        assert!(budget.take_owed_sweep(due));
        assert!(
            !budget.take_owed_sweep(due + DEFAULT_SWEEP_COOLDOWN),
            "the storm's own sweeps are not owed again"
        );
    }
}
