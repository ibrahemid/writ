//! The first launch, through the startup path the app itself runs (spec O1):
//! the folder, the note, the one line, and what the note's first line may do
//! to the note's file name.

use std::sync::Mutex as StdMutex;

use chrono::Utc;
use writ_core::startup::dated_note_name;
use writ_tauri_lib::commands::first_run::{
    auto_retitle_note_inner, dismiss_first_run_hint_inner, first_run_state_inner, RetitleOutcome,
};
use writ_tauri_lib::commands::buffer::save_buffer_content_inner;
use writ_tauri_lib::first_run::open_first_note;
use writ_tauri_lib::state::AppState;

// `AppState::initialize` reads `WRIT_DATA_DIR` and `WRIT_NOTES_DIR`; the tests
// in this file all set them, so they take turns.
static ENV_LOCK: StdMutex<()> = StdMutex::new(());

/// Starts the app against `data_dir` the way `run()` does.
fn launch(data_dir: &std::path::Path) -> AppState {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("WRIT_DATA_DIR", data_dir);
    std::env::remove_var("WRIT_NOTES_DIR");
    let state = AppState::initialize().expect("app state");
    std::env::remove_var("WRIT_DATA_DIR");
    state
}

/// Every file and folder directly inside the notes folder, sorted.
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

#[test]
fn a_first_launch_creates_one_notes_folder_and_one_note_and_a_second_creates_neither() {
    let dir = tempfile::tempdir().expect("tempdir");

    let first = launch(dir.path());
    assert!(first.first_run, "no config file means a first launch");
    let notes_root = first.notes_root();
    assert!(
        notes_root.is_dir(),
        "the notes folder is created before use"
    );
    assert!(
        notes_folder_entries(&first).is_empty(),
        "nothing is in the folder before the note is made"
    );

    let note = open_first_note(&first).expect("the first launch opens a note");
    assert_eq!(
        notes_folder_entries(&first),
        vec![dated_note_name(Utc::now())],
        "exactly one note, named for today"
    );
    assert_eq!(
        note.source_path.as_deref().map(std::path::Path::new),
        Some(notes_root.join(dated_note_name(Utc::now())).as_path())
    );
    drop(first);

    // What a running app writes the moment anything persists: from here the
    // launch is not a first one.
    std::fs::write(dir.path().join("config.toml"), "").expect("config file");

    let second = launch(dir.path());
    assert!(!second.first_run, "a config file means a later launch");
    assert!(
        open_first_note(&second).is_none(),
        "a later launch mints nothing"
    );
    assert_eq!(
        notes_folder_entries(&second),
        vec![dated_note_name(Utc::now())],
        "the note from the first launch is the only one, and it is still there"
    );
    assert_eq!(
        second
            .store
            .lock()
            .unwrap()
            .list_by_status(writ_core::buffer::document::BufferStatus::Active)
            .expect("open notes")
            .len(),
        1,
        "one note, opened once"
    );
}

#[test]
fn the_first_launch_state_carries_the_hint_until_it_is_dismissed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = launch(dir.path());

    let before = first_run_state_inner(&state).expect("first-run state");
    assert!(before.first_run);
    assert!(!before.hint_dismissed);
    assert!(["Finder", "File Explorer", "Files"].contains(&before.file_manager.as_str()));

    dismiss_first_run_hint_inner(&state).expect("dismiss");
    let after = first_run_state_inner(&state).expect("first-run state");
    assert!(after.hint_dismissed, "the line does not come back");

    // Dismissed once is dismissed for good: it survives the launch that reads
    // the config back off disk.
    std::env::set_var("WRIT_DATA_DIR", dir.path());
    let relaunched = launch(dir.path());
    assert!(
        first_run_state_inner(&relaunched)
            .expect("first-run state")
            .hint_dismissed
    );
}

/// Types `text` into the note and lets the save land, the way the editor does.
fn type_into(state: &AppState, note: &writ_core::buffer::document::BufferDocument, text: &str) {
    save_buffer_content_inner(state, &note.id, text).expect("save");
}

#[test]
fn a_first_line_renames_the_note_it_was_typed_in() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = launch(dir.path());
    let note = open_first_note(&state).expect("a note");
    type_into(&state, &note, "Grocery list\n\nmilk\n");

    let outcome = auto_retitle_note_inner(&state, &note.id).expect("retitle");
    let RetitleOutcome::Renamed { note: renamed } = outcome else {
        panic!("a note nothing has touched is renamed");
    };
    assert_eq!(
        notes_folder_entries(&state),
        vec!["Grocery list.md".to_string()]
    );
    assert_eq!(renamed.title, "Grocery list.md");

    // Once. The note is no longer watched, so a later first line leaves the
    // file where the person can now find it.
    type_into(&state, &renamed, "Something else\n");
    assert!(matches!(
        auto_retitle_note_inner(&state, &renamed.id).expect("retitle"),
        RetitleOutcome::Skipped
    ));
}

#[test]
fn a_note_something_else_has_touched_is_offered_the_rename_instead() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = launch(dir.path());
    let note = open_first_note(&state).expect("a note");
    let path = std::path::PathBuf::from(note.source_path.clone().expect("a file"));

    type_into(&state, &note, "# Grocery list\n");
    state.retitle_watch.changed_outside(&path);

    let outcome = auto_retitle_note_inner(&state, &note.id).expect("retitle");
    let RetitleOutcome::Ask { title } = outcome else {
        panic!("a note something else has touched is asked about, not renamed");
    };
    assert_eq!(title, "Grocery list");
    assert_eq!(
        notes_folder_entries(&state),
        vec![dated_note_name(Utc::now())],
        "nothing moved"
    );
}

#[test]
fn a_note_with_no_first_line_yet_is_left_alone() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = launch(dir.path());
    let note = open_first_note(&state).expect("a note");

    assert!(matches!(
        auto_retitle_note_inner(&state, &note.id).expect("retitle"),
        RetitleOutcome::Skipped
    ));
    assert_eq!(
        notes_folder_entries(&state),
        vec![dated_note_name(Utc::now())]
    );
}
