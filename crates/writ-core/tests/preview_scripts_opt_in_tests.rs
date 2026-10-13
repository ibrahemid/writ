//! A file another program modified loses its preview-scripts opt-in; a move, a
//! removal and every other event leave it alone. An opt-in stands while the
//! file holds the bytes it covers, and a save carries it over only then
//! (ADR-043).

use writ_core::events::bus::WritEvent;
use writ_core::hash::Sha256Digest;
use writ_core::preview::scripts::{
    opt_in_carries_over_write, opt_in_lost_by, opt_in_standing, FileLook, OptInStanding,
};
use writ_core::watcher::change_event::ExternalChange;

const COVERED: Sha256Digest = [1; 32];
const OTHER: Sha256Digest = [2; 32];

fn external(change: ExternalChange) -> WritEvent {
    WritEvent::BufferExternal {
        buffer_id: "buf-1".to_string(),
        path: "/downloads/report.html".to_string(),
        change,
        new_path: None,
        disk_hash: Some("hash".to_string()),
    }
}

#[test]
fn a_file_another_program_modified_loses_its_opt_in() {
    assert_eq!(
        opt_in_lost_by(&external(ExternalChange::Modified)),
        Some("buf-1")
    );
}

#[test]
fn a_move_or_a_removal_keeps_it() {
    assert_eq!(opt_in_lost_by(&external(ExternalChange::Moved)), None);
    assert_eq!(opt_in_lost_by(&external(ExternalChange::Removed)), None);
}

#[test]
fn other_events_keep_it() {
    for event in [
        WritEvent::BufferOpened {
            id: "buf-1".to_string(),
            title: "report.html".to_string(),
        },
        WritEvent::ConfigChanged {
            keys: vec!["preview.run_scripts".to_string()],
        },
        WritEvent::NotesChanged {
            path: "/notes/report.html".to_string(),
            removed: false,
        },
    ] {
        assert_eq!(opt_in_lost_by(&event), None, "{event:?}");
    }
}

#[test]
fn an_opt_in_stands_while_the_file_holds_the_bytes_it_covers() {
    assert_eq!(
        opt_in_standing(Some(COVERED), FileLook::Holds(COVERED)),
        OptInStanding::Stands
    );
    assert!(opt_in_carries_over_write(
        Some(COVERED),
        FileLook::Holds(COVERED)
    ));
}

#[test]
fn an_opt_in_lapses_when_the_file_holds_other_bytes() {
    assert_eq!(
        opt_in_standing(Some(COVERED), FileLook::Holds(OTHER)),
        OptInStanding::Lapses
    );
    assert!(!opt_in_carries_over_write(
        Some(COVERED),
        FileLook::Holds(OTHER)
    ));
}

#[test]
fn an_opt_in_given_before_the_file_had_bytes_covers_none() {
    assert_eq!(
        opt_in_standing(None, FileLook::Absent),
        OptInStanding::Stands
    );
    assert_eq!(
        opt_in_standing(None, FileLook::Holds(COVERED)),
        OptInStanding::Lapses
    );
}

#[test]
fn a_file_that_is_gone_keeps_the_opt_in() {
    assert_eq!(
        opt_in_standing(Some(COVERED), FileLook::Absent),
        OptInStanding::Stands
    );
    assert!(opt_in_carries_over_write(Some(COVERED), FileLook::Absent));
}

#[test]
fn a_file_that_cannot_be_read_runs_nothing_and_carries_nothing_over_a_save() {
    assert_eq!(
        opt_in_standing(Some(COVERED), FileLook::Unread),
        OptInStanding::Unchecked
    );
    assert!(!opt_in_carries_over_write(Some(COVERED), FileLook::Unread));
}
