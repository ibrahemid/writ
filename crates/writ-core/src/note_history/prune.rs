//! Which entries have to go, and in what order they are given up.
//!
//! Three caps, applied in one order and never in another (spec 470): age
//! first, then the number of entries one note keeps, then the size of the
//! whole store. Age is the rule a person can predict, so it runs first and
//! the size cap only sees what age and the per-note cap left behind.
//!
//! The per-note cap thins rather than cuts (ADR-031 §5.6). Cutting from the
//! oldest end gave a long session of autosaves the whole cap, and the first
//! text to go was the one from before the session. So entries are put in
//! time buckets that widen with age, and a note over its cap first gives up
//! the entries that share a bucket with a newer one, then the rest of the
//! ordinary ones, then the entries that mark something: the first and last
//! of each session, the first of each day, and a text that something other
//! than a run of saves set aside. The newest entry, the first of the newest
//! session, and the last entry before that session are never given up to
//! the cap.
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

use std::collections::{HashMap, HashSet};
use std::time::{Duration, SystemTime};

use super::{MAX_STORE_BYTES, MAX_VERSIONS_PER_NOTE, RETENTION, SESSION_GAP};

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
    /// Whether something other than a run of saves made the entry: a text a
    /// write was about to replace, or a write that was not the editor's. No
    /// later save of a run can stand in for it.
    pub set_aside: bool,
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

    // The per-note cap, by thinning.
    let mut by_note: HashMap<i64, Vec<usize>> = HashMap::new();
    for &i in &oldest_first {
        if alive[i] {
            by_note.entry(entries[i].note).or_default().push(i);
        }
    }
    let mut over_cap: Vec<usize> = by_note
        .values()
        .flat_map(|note| thin(note, entries, now))
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

/// How much an entry is worth keeping when its note is over the cap, least
/// first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Worth {
    /// It shares a time bucket with a newer entry of its note.
    Redundant,
    /// The newest entry of its bucket.
    Ordinary,
    /// It marks something: a session's first or last, a day's first, or a
    /// text set aside.
    Marker,
    /// The newest entry, the newest session's first, or the last entry before
    /// that session. The cap never takes one.
    Pinned,
}

/// The entries one note over [`MAX_VERSIONS_PER_NOTE`] gives up, as indexes
/// into `entries`.
///
/// `note` holds the note's live entries, oldest first. The least worth goes
/// first and, within one [`Worth`], the oldest.
fn thin(note: &[usize], entries: &[VersionFacts], now: SystemTime) -> Vec<usize> {
    let excess = note.len().saturating_sub(MAX_VERSIONS_PER_NOTE);
    if excess == 0 {
        return Vec::new();
    }
    let at = |k: usize| entries[note[k]].at;
    let mut worth = vec![Worth::Ordinary; note.len()];
    let mut raise = |k: usize, to: Worth| worth[k] = worth[k].max(to);

    let mut session_starts = vec![0];
    for k in 1..note.len() {
        if gap(at(k - 1), at(k)) >= SESSION_GAP {
            session_starts.push(k);
        }
    }
    for (n, &start) in session_starts.iter().enumerate() {
        let end = session_starts.get(n + 1).map_or(note.len(), |&next| next) - 1;
        raise(start, Worth::Marker);
        raise(end, Worth::Marker);
    }
    let mut last_day = None;
    for k in 0..note.len() {
        let day = seconds_since_epoch(at(k)) / DAY_SECONDS;
        if last_day != Some(day) {
            raise(k, Worth::Marker);
            last_day = Some(day);
        }
        if entries[note[k]].set_aside {
            raise(k, Worth::Marker);
        }
    }
    let newest_session = *session_starts.last().unwrap_or(&0);
    raise(note.len() - 1, Worth::Pinned);
    raise(newest_session, Worth::Pinned);
    if newest_session > 0 {
        raise(newest_session - 1, Worth::Pinned);
    }

    let mut seen: HashSet<(u64, u64)> = HashSet::new();
    for k in (0..note.len()).rev() {
        let first_in_bucket = seen.insert(bucket_of(at(k), now));
        if !first_in_bucket && worth[k] == Worth::Ordinary {
            worth[k] = Worth::Redundant;
        }
    }

    let mut candidates: Vec<usize> = (0..note.len())
        .filter(|&k| worth[k] != Worth::Pinned)
        .collect();
    candidates.sort_by_key(|&k| (worth[k], at(k), entries[note[k]].id));
    candidates
        .into_iter()
        .take(excess)
        .map(|k| note[k])
        .collect()
}

const DAY_SECONDS: u64 = 24 * 60 * 60;

/// The time bucket an entry made at `at` falls in, as (width, index). The
/// width grows with age: a minute within the last hour, ten minutes within
/// the day, an hour within the week, a day after that.
fn bucket_of(at: SystemTime, now: SystemTime) -> (u64, u64) {
    let age = now.duration_since(at).unwrap_or(Duration::ZERO);
    let width = if age <= Duration::from_secs(60 * 60) {
        60
    } else if age <= Duration::from_secs(DAY_SECONDS) {
        10 * 60
    } else if age <= Duration::from_secs(7 * DAY_SECONDS) {
        60 * 60
    } else {
        DAY_SECONDS
    };
    (width, seconds_since_epoch(at) / width)
}

/// How long after `earlier` an entry made at `later` came, and nothing for a
/// clock that went backwards.
fn gap(earlier: SystemTime, later: SystemTime) -> Duration {
    later.duration_since(earlier).unwrap_or(Duration::ZERO)
}

fn seconds_since_epoch(at: SystemTime) -> u64 {
    at.duration_since(SystemTime::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0)
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
            set_aside: false,
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
                set_aside: false,
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
    fn a_note_over_its_cap_thins_its_oldest_minutes_and_keeps_its_sessions_first() {
        let entries: Vec<VersionFacts> = (0..MAX_VERSIONS_PER_NOTE as i64 + 3)
            .map(|i| entry(i + 1, 1, Duration::from_secs((300 - i) as u64), 1024))
            .collect();
        let plan = prune_plan(&entries, now());
        assert_eq!(
            plan.retire,
            vec![2, 3, 4],
            "the oldest that share a minute with a newer one; the session's first stays"
        );
    }

    /// One note: an entry from an earlier session, then a save every ten
    /// seconds for `minutes`.
    fn a_long_session(minutes: u64) -> Vec<VersionFacts> {
        let saves = minutes * 6;
        let mut entries = vec![entry(
            1,
            1,
            Duration::from_secs(saves * 10 + 3 * 3600),
            1024,
        )];
        for save in 0..=saves {
            entries.push(entry(
                save as i64 + 2,
                1,
                Duration::from_secs((saves - save) * 10),
                1024,
            ));
        }
        entries
    }

    #[test]
    fn the_entry_from_before_a_forty_minute_session_is_kept() {
        let entries = a_long_session(40);
        assert!(entries.len() > MAX_VERSIONS_PER_NOTE);
        let plan = prune_plan(&entries, now());
        assert!(
            !plan.retire.contains(&1),
            "the text from before the session"
        );
        assert!(!plan.retire.contains(&2), "the session's first save");
        assert_eq!(entries.len() - plan.retire.len(), MAX_VERSIONS_PER_NOTE);
    }

    #[test]
    fn a_long_session_keeps_one_entry_a_minute_before_it_gives_up_any() {
        let entries = a_long_session(30);
        let plan = prune_plan(&entries, now());
        let retired: HashSet<i64> = plan.retire.iter().copied().collect();
        let mut minutes: HashMap<(u64, u64), usize> = HashMap::new();
        for kept in entries.iter().filter(|e| !retired.contains(&e.id)) {
            *minutes.entry(bucket_of(kept.at, now())).or_insert(0) += 1;
        }
        for facts in &entries {
            assert!(
                minutes.contains_key(&bucket_of(facts.at, now())),
                "every minute of the session still has an entry"
            );
        }
    }

    #[test]
    fn a_text_set_aside_outlasts_the_ordinary_entries_around_it() {
        let mut entries = a_long_session(40);
        // Somewhere in the middle of the session, a program's write replaced
        // the text and the save after it took the bucket.
        entries[120].set_aside = true;
        let set_aside = entries[120].id;
        let plan = prune_plan(&entries, now());
        assert!(!plan.retire.contains(&set_aside));
    }

    #[test]
    fn markers_go_only_once_nothing_ordinary_is_left() {
        // An entry a day for 230 days inside retention cannot happen, so the
        // same shape with hours: every entry its own session, every one a
        // marker, and the oldest go first.
        let entries: Vec<VersionFacts> = (0..MAX_VERSIONS_PER_NOTE as i64 + 5)
            .map(|i| entry(i + 1, 1, Duration::from_secs((300 - i) as u64 * 3600), 1024))
            .collect();
        let plan = prune_plan(&entries, now());
        assert_eq!(plan.retire, vec![1, 2, 3, 4, 5]);
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
