//! `Today's Note` (spec D1): one file a day, named for the local calendar day.

use std::sync::Mutex as StdMutex;

use chrono::{DateTime, Local, TimeZone, Utc};
use writ_core::startup::dated_note_name;
use writ_storage::notes_index;
use writ_tauri_lib::commands::notes::todays_note_inner;
use writ_tauri_lib::state::AppState;

// `AppState::initialize` reads `WRIT_DATA_DIR` and `WRIT_NOTES_DIR`, and the
// locale test writes `LC_ALL` and `LANG`, so every test here takes its turn.
static ENV_LOCK: StdMutex<()> = StdMutex::new(());

/// Starts the app against a data folder and a notes folder of its own.
fn launch(data_dir: &std::path::Path, notes_dir: &std::path::Path) -> AppState {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("WRIT_DATA_DIR", data_dir);
    std::env::set_var("WRIT_NOTES_DIR", notes_dir);
    let state = AppState::initialize().expect("app state");
    std::env::remove_var("WRIT_DATA_DIR");
    std::env::remove_var("WRIT_NOTES_DIR");
    state
}

/// A wall-clock time in the machine's own time zone, as an instant.
fn local_instant(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> DateTime<Utc> {
    Local
        .with_ymd_and_hms(year, month, day, hour, minute, second)
        .earliest()
        .expect("a local wall-clock time this test can name")
        .with_timezone(&Utc)
}

/// Every file directly inside the notes folder, sorted.
fn notes_folder_entries(state: &AppState) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(state.notes_root())
        .expect("the notes folder is there")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

/// The file a note was opened from.
fn opened_path(note: &writ_core::buffer::document::BufferDocument) -> std::path::PathBuf {
    std::path::PathBuf::from(note.source_path.clone().expect("a file"))
}

#[test]
fn twice_in_one_day_opens_the_same_note_and_makes_no_second_file() {
    let data = tempfile::tempdir().expect("tempdir");
    let notes = tempfile::tempdir().expect("tempdir");
    let state = launch(data.path(), notes.path());
    let now = local_instant(2026, 3, 18, 9, 30, 0);

    let first = todays_note_inner(&state, now).expect("today's note");
    let second = todays_note_inner(&state, now).expect("today's note again");

    assert_eq!(
        first.id, second.id,
        "the second call opens the note the first one made"
    );
    assert_eq!(
        notes_folder_entries(&state),
        vec![dated_note_name(now)],
        "one file for the day, not two"
    );
}

#[test]
fn the_file_name_is_the_date_whatever_the_locale_says() {
    let data = tempfile::tempdir().expect("tempdir");
    let notes = tempfile::tempdir().expect("tempdir");
    let state = launch(data.path(), notes.path());
    let now = local_instant(2026, 3, 18, 9, 30, 0);

    let restore = {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let before = (std::env::var("LC_ALL").ok(), std::env::var("LANG").ok());
        std::env::set_var("LC_ALL", "de_DE.UTF-8");
        std::env::set_var("LANG", "de_DE.UTF-8");
        before
    };

    let note = todays_note_inner(&state, now).expect("today's note");

    {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        match restore.0 {
            Some(value) => std::env::set_var("LC_ALL", value),
            None => std::env::remove_var("LC_ALL"),
        }
        match restore.1 {
            Some(value) => std::env::set_var("LANG", value),
            None => std::env::remove_var("LANG"),
        }
    }

    assert_eq!(
        opened_path(&note).file_name().and_then(|n| n.to_str()),
        Some("2026-03-18.md"),
        "the file name sorts, so it is the date and nothing else"
    );
}

#[test]
fn a_minute_either_side_of_midnight_makes_two_notes() {
    let data = tempfile::tempdir().expect("tempdir");
    let notes = tempfile::tempdir().expect("tempdir");
    let state = launch(data.path(), notes.path());

    let before = local_instant(2026, 3, 17, 23, 59, 59);
    let after = local_instant(2026, 3, 18, 0, 0, 1);

    let yesterday = todays_note_inner(&state, before).expect("yesterday's note");
    let today = todays_note_inner(&state, after).expect("today's note");

    assert_ne!(yesterday.id, today.id, "a new day is a new note");
    assert_eq!(
        notes_folder_entries(&state),
        vec!["2026-03-17.md".to_string(), "2026-03-18.md".to_string()],
    );
}

#[test]
fn a_note_that_is_already_there_is_opened_and_keeps_every_word() {
    let data = tempfile::tempdir().expect("tempdir");
    let notes = tempfile::tempdir().expect("tempdir");
    let state = launch(data.path(), notes.path());
    let now = local_instant(2026, 3, 18, 9, 30, 0);

    let path = state.notes_root().join(dated_note_name(now));
    let written = "# Standup\n\n- shipped the index\n";
    std::fs::write(&path, written).expect("a note written before Writ asked");

    let note = todays_note_inner(&state, now).expect("today's note");

    assert_eq!(
        opened_path(&note),
        path,
        "the note on disk is the one opened"
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("the note"),
        written,
        "opening a note never empties it"
    );
    assert_eq!(
        notes_folder_entries(&state),
        vec![dated_note_name(now)],
        "nothing was made beside it"
    );
}

#[test]
fn a_note_it_makes_is_in_the_index_with_no_walk_of_the_folder() {
    let data = tempfile::tempdir().expect("tempdir");
    let notes = tempfile::tempdir().expect("tempdir");
    let state = launch(data.path(), notes.path());
    let now = local_instant(2026, 3, 18, 9, 30, 0);

    todays_note_inner(&state, now).expect("today's note");

    let key = notes_index::index_key(&state.notes_root().join(dated_note_name(now)));
    assert!(
        state
            .notes_index
            .note_paths()
            .expect("the index answers")
            .contains(&key),
        "the note is searchable and linkable the moment it exists"
    );
}
