//! `mcp-pending.json`: one entry per waiting client, whatever the call volume.

use chrono::{Duration, TimeZone, Utc};
use writ_core::activity::ClientId;
use writ_storage::pending_clients::{self, note_calls_at};

fn at(minute: i64) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 9, 10, 0, 0).unwrap() + Duration::minutes(minute)
}

fn client() -> ClientId {
    ClientId {
        name: "Claude Code".to_string(),
        version: Some("1.2.3".to_string()),
    }
}

#[test]
fn a_client_never_seen_reads_as_nobody_waiting() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    assert!(pending_clients::read(dir.path()).is_empty());
}

#[test]
fn a_first_call_puts_the_client_in_the_file() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    note_calls_at(dir.path(), &client(), 1, at(0)).expect("note");

    let waiting = pending_clients::read(dir.path());
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].name, "Claude Code");
    assert_eq!(waiting[0].version.as_deref(), Some("1.2.3"));
    assert_eq!(waiting[0].first_seen, at(0));
    assert_eq!(waiting[0].calls, 1);
}

/// The volume a client can reach is capped by the gate's rate limit, not here;
/// what this pins is that repeated writes under one name never add an entry and
/// never grow the file. The 10,000-call case is
/// `a_client_refused_ten_thousand_times_is_recorded_once` in `writ-mcp`.
#[test]
fn writing_the_same_name_again_adds_no_entry_and_no_bytes() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    for minute in 0..200 {
        note_calls_at(dir.path(), &client(), 1, at(minute)).expect("note");
    }

    let waiting = pending_clients::read(dir.path());
    assert_eq!(waiting.len(), 1, "one entry per name, whatever the volume");
    assert_eq!(waiting[0].first_seen, at(0));
    assert_eq!(waiting[0].last_seen, at(199));
    assert_eq!(waiting[0].calls, 200);

    let size = std::fs::metadata(pending_clients::path(dir.path()))
        .expect("the file")
        .len();
    assert!(size < 4_096, "the file grew with the calls: {size} bytes");
}

#[test]
fn calls_counted_between_writes_are_carried_in() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    note_calls_at(dir.path(), &client(), 1, at(0)).expect("note");
    note_calls_at(dir.path(), &client(), 240, at(1)).expect("note");

    let waiting = pending_clients::read(dir.path());
    assert_eq!(waiting[0].calls, 241);
    assert_eq!(waiting[0].last_seen, at(1));
}

#[test]
fn forgetting_a_client_clears_its_entry() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    note_calls_at(dir.path(), &client(), 1, at(0)).expect("note");
    note_calls_at(dir.path(), &ClientId::named("Zed"), 1, at(0)).expect("note");

    pending_clients::forget(dir.path(), "Claude Code").expect("forget");

    let waiting = pending_clients::read(dir.path());
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].name, "Zed");
}

#[test]
fn forgetting_a_client_that_was_never_there_is_not_an_error() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    pending_clients::forget(dir.path(), "Nobody").expect("forget");
    assert!(pending_clients::read(dir.path()).is_empty());
}

#[test]
fn two_names_are_two_entries_oldest_first() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    note_calls_at(dir.path(), &ClientId::named("Zed"), 1, at(1)).expect("note");
    note_calls_at(dir.path(), &client(), 1, at(0)).expect("note");

    let waiting = pending_clients::read(dir.path());
    let names: Vec<&str> = waiting.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(names, ["Claude Code", "Zed"]);
}

#[test]
fn a_file_that_does_not_parse_reads_as_nobody_waiting() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    std::fs::write(pending_clients::path(dir.path()), "{ not json").expect("seed");

    assert!(pending_clients::read(dir.path()).is_empty());

    // And the next call replaces it rather than failing on it.
    note_calls_at(dir.path(), &client(), 1, at(0)).expect("note");
    assert_eq!(pending_clients::read(dir.path()).len(), 1);
}

#[test]
fn a_later_version_from_the_same_name_replaces_the_recorded_one() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    note_calls_at(dir.path(), &ClientId::named("Claude Code"), 1, at(0)).expect("note");
    note_calls_at(dir.path(), &client(), 1, at(1)).expect("note");

    let waiting = pending_clients::read(dir.path());
    assert_eq!(waiting[0].version.as_deref(), Some("1.2.3"));
}
