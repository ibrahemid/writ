//! The record of a program that called while Connected programs was off.

use writ_core::activity::ClientId;
use writ_storage::off_calls::{note, note_at, path, read};

#[test]
fn a_folder_with_no_record_reads_as_none() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    assert!(read(dir.path()).is_none());
}

#[test]
fn the_record_holds_the_name_and_the_time_and_nothing_else() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let at = chrono::DateTime::parse_from_rfc3339("2026-10-15T09:00:00Z")
        .expect("time")
        .with_timezone(&chrono::Utc);
    let client = ClientId {
        name: "claude-code".to_string(),
        version: Some("2.0.1".to_string()),
    };

    note_at(dir.path(), &client, at).expect("written");

    let record = read(dir.path()).expect("a record");
    assert_eq!(record.name, "claude-code");
    assert_eq!(record.at, at);
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path(dir.path())).expect("read")).expect("json");
    let mut keys: Vec<&String> = value.as_object().expect("an object").keys().collect();
    keys.sort();
    assert_eq!(keys, ["at", "name"]);
}

#[test]
fn a_later_call_replaces_the_record() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    note(dir.path(), &ClientId::named("cursor-vscode")).expect("first");
    note(dir.path(), &ClientId::named("claude-ai")).expect("second");

    assert_eq!(read(dir.path()).expect("a record").name, "claude-ai");
}

#[test]
fn a_record_that_does_not_parse_reads_as_none() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    std::fs::write(path(dir.path()), b"{\"name\":").expect("seed");
    assert!(read(dir.path()).is_none());
}
