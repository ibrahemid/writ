//! Which entries have to go, and in what order they are given up.
//!
//! Three caps, applied in one order and never in another (spec 470): age
//! first, then the number of entries one note keeps, then the size of the
//! whole store. Age is the rule a person can predict, so it runs first and
//! the size cap only sees what age and the per-note cap left behind.
//!
//! One thing is held back from every pass. A note's newest entry is the last
//! text of that note the store holds, and a note that has been deleted has no
//! file to fall back on, so it is given up only when nothing else is left to
//! give. No note is emptied while another note still holds more than one
//! entry.
//!
//! When even that is not enough — the last entries alone are over the size
//! cap — the loss is unavoidable and the only question is whose. It falls on
//! the note holding the most bytes, oldest entry first, so a folder of small
//! notes is not emptied to keep one large one whole.

use std::collections::HashMap;
use std::time::SystemTime;

use super::{MAX_STORE_BYTES, MAX_VERSIONS_PER_NOTE, RETENTION};

/// What the planner needs to know about one entry.
///
/// No text and no path: pruning is arithmetic on when an entry was made and
/// what it costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionFacts {
    /// The entry, as the store names it.
    pub id: i64,
    /// The note it belongs to. Entries carrying the same value are one note's.
    pub note: i64,
    /// When it was made.
    pub at: SystemTime,
    /// What its text costs in bytes.
    pub bytes: u64,
}

/// What a pass over the store would retire.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PrunePlan {
    /// The entries to drop, oldest first.
    pub retire: Vec<i64>,
}

impl PrunePlan {
    /// Whether the plan asks for nothing.
    pub fn is_empty(&self) -> bool {
        self.retire.is_empty()
    }
}

/// Decides which of `entries` the store gives up.
///
/// Age, then the per-note cap, then the total size, in that order. A note's
/// newest entry survives every pass while any other note still holds more
/// than one, so no note is emptied to spare a note that is over its cap.
/// Once every note is down to its last entry and the store is still over
/// [`MAX_STORE_BYTES`], the note holding the most bytes gives up its oldest
/// entry, and again until the store fits: a cap that cannot be met is worse
/// than a note that loses its last version, and the note costing the most is
/// the one to ask first.
pub fn prune_plan(entries: &[VersionFacts], now: SystemTime) -> PrunePlan {
    let mut alive = vec![true; entries.len()];
    let mut held: HashMap<i64, usize> = HashMap::new();
    for entry in entries {
        *held.entry(entry.note).or_insert(0) += 1;
    }
    // Oldest first, and the id breaks a tie so two entries made in the same
    // millisecond are given up in the order they were made.
    let mut oldest_first: Vec<usize> = (0..entries.len()).collect();
    oldest_first.sort_by_key(|&i| (entries[i].at, entries[i].id));

    let mut retire: Vec<usize> = Vec::new();

    // Age. A note's last entry is not retired for being old: a note deleted
    // last year has nothing else left, and the file is not there to hold the
    // text instead.
    for &i in &oldest_first {
        if !is_older_than_retention(entries[i].at, now) {
            continue;
        }
        if held[&entries[i].note] <= 1 {
            continue;
        }
        take(i, entries, &mut alive, &mut held, &mut retire);
    }

    // The per-note cap, newest kept.
    let mut by_note: HashMap<i64, Vec<usize>> = HashMap::new();
    for &i in oldest_first.iter().rev() {
        if alive[i] {
            by_note.entry(entries[i].note).or_default().push(i);
        }
    }
    let mut over_cap: Vec<usize> = by_note
        .into_values()
        .flat_map(|newest_first| {
            newest_first
                .into_iter()
                .skip(MAX_VERSIONS_PER_NOTE)
                .collect::<Vec<_>>()
        })
        .collect();
    over_cap.sort_by_key(|&i| (entries[i].at, entries[i].id));
    for i in over_cap {
        take(i, entries, &mut alive, &mut held, &mut retire);
    }

    // Total size. Oldest first among the entries that are not the last their
    // note has, then, only if the store is still over, the last entries of
    // the notes holding the most bytes.
    let mut total: u64 = alive
        .iter()
        .zip(entries)
        .filter(|(alive, _)| **alive)
        .map(|(_, entry)| entry.bytes)
        .sum();
    let mut cursor = 0;
    while total > MAX_STORE_BYTES && cursor < oldest_first.len() {
        let i = oldest_first[cursor];
        cursor += 1;
        if !alive[i] || held[&entries[i].note] <= 1 {
            continue;
        }
        total -= entries[i].bytes;
        take(i, entries, &mut alive, &mut held, &mut retire);
    }
    while total > MAX_STORE_BYTES {
        let Some(note) = costliest_note(entries, &alive) else {
            break;
        };
        let Some(&i) = oldest_first
            .iter()
            .find(|&&i| alive[i] && entries[i].note == note)
        else {
            break;
        };
        total -= entries[i].bytes;
        take(i, entries, &mut alive, &mut held, &mut retire);
    }

    retire.sort_by_key(|&i| (entries[i].at, entries[i].id));
    PrunePlan {
        retire: retire.into_iter().map(|i| entries[i].id).collect(),
    }
}

/// The note whose live entries add up to the most bytes, and the lowest note
/// id of the notes that tie, so a plan does not depend on iteration order.
///
/// Asked once per entry the fallback gives up rather than once for the pass:
/// each answer changes what the next one is, and the fallback runs only when
/// the last entries alone are over the cap.
fn costliest_note(entries: &[VersionFacts], alive: &[bool]) -> Option<i64> {
    let mut bytes_by_note: HashMap<i64, u64> = HashMap::new();
    for (i, entry) in entries.iter().enumerate() {
        if alive[i] {
            *bytes_by_note.entry(entry.note).or_insert(0) += entry.bytes;
        }
    }
    let mut costliest: Option<(u64, i64)> = None;
    for (&note, &bytes) in &bytes_by_note {
        let takes_it = match costliest {
            None => true,
            Some((most, held)) => bytes > most || (bytes == most && note < held),
        };
        if takes_it {
            costliest = Some((bytes, note));
        }
    }
    costliest.map(|(_, note)| note)
}

/// Whether an entry made at `at` has outlived [`RETENTION`].
///
/// An entry stamped in the future is not old: a clock that moved, or a file
/// whose time came from another machine, is not a reason to drop a text.
fn is_older_than_retention(at: SystemTime, now: SystemTime) -> bool {
    now.duration_since(at)
        .map(|since| since > RETENTION)
        .unwrap_or(false)
}

/// Marks one entry for retirement and takes it off its note's count.
fn take(
    i: usize,
    entries: &[VersionFacts],
    alive: &mut [bool],
    held: &mut HashMap<i64, usize>,
    retire: &mut Vec<usize>,
) {
    alive[i] = false;
    if let Some(count) = held.get_mut(&entries[i].note) {
        *count = count.saturating_sub(1);
    }
    retire.push(i);
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::time::Duration;

    const DAY: Duration = Duration::from_secs(24 * 60 * 60);
    const MB: u64 = 1024 * 1024;

    fn now() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(400 * 24 * 60 * 60)
    }

    fn entry(id: i64, note: i64, age: Duration, bytes: u64) -> VersionFacts {
        VersionFacts {
            id,
            note,
            at: now() - age,
            bytes,
        }
    }

    #[test]
    fn nothing_to_prune_is_an_empty_plan() {
        assert!(prune_plan(&[], now()).is_empty());
        assert!(prune_plan(&[entry(1, 1, DAY, 20 * 1024)], now()).is_empty());
    }

    #[test]
    fn an_entry_past_thirty_days_is_retired_by_age() {
        let entries = [
            entry(1, 1, 31 * DAY, 20 * 1024),
            entry(2, 1, 29 * DAY, 20 * 1024),
            entry(3, 1, DAY, 20 * 1024),
        ];
        assert_eq!(prune_plan(&entries, now()).retire, vec![1]);
    }

    #[test]
    fn an_entry_stamped_in_the_future_is_not_old() {
        let entries = [
            VersionFacts {
                id: 1,
                note: 1,
                at: now() + 40 * DAY,
                bytes: 20 * 1024,
            },
            entry(2, 1, DAY, 20 * 1024),
        ];
        assert!(prune_plan(&entries, now()).is_empty());
    }

    #[test]
    fn the_last_entry_a_note_has_is_not_retired_for_being_old() {
        let entries = [
            entry(1, 1, 400 * DAY, 20 * 1024),
            entry(2, 2, DAY, 20 * 1024),
            entry(3, 2, DAY, 20 * 1024),
        ];
        assert!(
            !prune_plan(&entries, now()).retire.contains(&1),
            "a note deleted a year ago keeps the text it was deleted with"
        );
    }

    #[test]
    fn a_note_over_its_cap_gives_up_its_oldest() {
        let entries: Vec<VersionFacts> = (0..MAX_VERSIONS_PER_NOTE as i64 + 3)
            .map(|i| entry(i + 1, 1, Duration::from_secs((300 - i) as u64), 1024))
            .collect();
        let plan = prune_plan(&entries, now());
        assert_eq!(plan.retire, vec![1, 2, 3], "the three oldest, in order");
    }

    #[test]
    fn the_cap_counts_only_what_age_left_behind() {
        let mut entries: Vec<VersionFacts> = (0..MAX_VERSIONS_PER_NOTE as i64)
            .map(|i| entry(i + 1, 1, Duration::from_secs((300 - i) as u64), 1024))
            .collect();
        entries.push(entry(999, 1, 40 * DAY, 1024));
        let plan = prune_plan(&entries, now());
        assert_eq!(
            plan.retire,
            vec![999],
            "the old one goes and the cap has nothing left to do"
        );
    }

    #[test]
    fn the_size_cap_takes_the_oldest_across_every_note() {
        let entries = [
            entry(1, 1, 3 * DAY, 100 * MB),
            entry(2, 2, 2 * DAY, 100 * MB),
            entry(3, 1, DAY, 100 * MB),
            entry(4, 2, Duration::from_secs(60), 60 * MB),
        ];
        assert_eq!(
            prune_plan(&entries, now()).retire,
            vec![1, 2],
            "age decides, not which note the entry belongs to"
        );
    }

    #[test]
    fn the_size_cap_empties_no_note_while_a_younger_one_holds_more_than_it_needs() {
        let entries = [
            // One note, one entry, the oldest thing in the store.
            entry(1, 1, 20 * DAY, 60 * MB),
            // Another note, three entries, every one of them younger.
            entry(2, 2, 3 * DAY, 100 * MB),
            entry(3, 2, 2 * DAY, 100 * MB),
            entry(4, 2, DAY, 100 * MB),
        ];
        let plan = prune_plan(&entries, now());
        assert_eq!(
            plan.retire,
            vec![2, 3],
            "the busy note pays before the quiet note loses everything"
        );
    }

    #[test]
    fn a_store_over_the_cap_with_nothing_left_to_give_takes_a_last_entry() {
        let entries = [
            entry(1, 1, 3 * DAY, 200 * MB),
            entry(2, 2, 2 * DAY, 200 * MB),
        ];
        assert_eq!(
            prune_plan(&entries, now()).retire,
            vec![1],
            "a cap that cannot be met is worse than a note losing its last text"
        );
    }

    #[test]
    fn the_note_holding_the_most_gives_up_its_last_entry_before_a_smaller_one_does() {
        // The oldest entry belongs to a note costing a kilobyte; the store is
        // over its cap because one other note holds nearly all of it.
        let entries = [entry(1, 1, 9 * DAY, 1_024), entry(2, 2, 1 * DAY, 260 * MB)];
        assert_eq!(
            prune_plan(&entries, now()).retire,
            vec![2],
            "the note costing the most is asked first, and the small note keeps its text"
        );
    }

    #[test]
    fn the_three_passes_run_in_their_stated_order() {
        let entries = [
            // Older than retention, and its note has company: age takes it.
            entry(1, 1, 40 * DAY, 10 * MB),
            entry(2, 1, 2 * DAY, 10 * MB),
            // The busy note: over neither cap on its own, over the store's.
            entry(3, 2, 5 * DAY, 150 * MB),
            entry(4, 2, 4 * DAY, 150 * MB),
        ];
        let plan = prune_plan(&entries, now());
        assert_eq!(plan.retire, vec![1, 3]);
    }

    #[test]
    fn every_note_keeps_at_most_its_cap_and_nothing_older_than_retention_but_its_last() {
        let mut entries = Vec::new();
        let mut id = 0;
        for note in 1..=3i64 {
            for age in 0..250u64 {
                id += 1;
                entries.push(entry(id, note, Duration::from_secs(age * 3600), 20 * 1024));
            }
        }
        let plan = prune_plan(&entries, now());
        let retired: std::collections::HashSet<i64> = plan.retire.iter().copied().collect();
        for note in 1..=3i64 {
            let kept = entries
                .iter()
                .filter(|e| e.note == note && !retired.contains(&e.id))
                .count();
            assert_eq!(kept, MAX_VERSIONS_PER_NOTE, "note {note}");
        }
    }
}
