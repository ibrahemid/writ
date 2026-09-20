//! The first launch, through the path the app itself runs (ADR-041 §3): the
//! folder, the format the screen records, the file, the one line, and what the
//! file's first line may do to the file's name.

use std::sync::Mutex as StdMutex;

use writ_core::config::FileExtension;
use writ_tauri_lib::commands::buffer::save_buffer_content_inner;
use writ_tauri_lib::commands::first_run::{
    auto_retitle_note_inner, dismiss_first_run_hint_inner, first_run_state_inner, RetitleOutcome,
};
use writ_tauri_lib::first_run::finish_first_run_inner;
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

/// Starts the app against a notes folder that is already there, which is what
/// a returning person has.
fn launch_with_notes(data_dir: &std::path::Path, notes_dir: &std::path::Path) -> AppState {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("WRIT_DATA_DIR", data_dir);
    std::env::set_var("WRIT_NOTES_DIR", notes_dir);
    let state = AppState::initialize().expect("app state");
    std::env::remove_var("WRIT_DATA_DIR");
    std::env::remove_var("WRIT_NOTES_DIR");
    state
}

/// Answers the first-launch screen with plain text, which is its default.
fn finish(state: &AppState) -> Option<writ_core::buffer::document::BufferDocument> {
    finish_first_run_inner(state, FileExtension::Txt).expect("the screen is answered")
}

/// The config file a launch writes for itself.
fn config_file(data_dir: &std::path::Path) -> std::path::PathBuf {
    data_dir.join("config.toml")
}

/// The file a note was opened from.
fn opened_path(note: &writ_core::buffer::document::BufferDocument) -> std::path::PathBuf {
    std::path::PathBuf::from(note.source_path.clone().expect("a file"))
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
fn a_first_launch_creates_one_notes_folder_and_one_file_and_a_second_creates_neither() {
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
        "nothing is in the folder before the screen is answered"
    );
    assert!(
        !config_file(dir.path()).is_file(),
        "nothing is written until the screen is answered"
    );

    let note = finish(&first).expect("the first launch opens a file");
    assert_eq!(
        notes_folder_entries(&first),
        vec!["Untitled.txt".to_string()],
        "exactly one file, untitled, in the format that was chosen"
    );
    assert_eq!(
        note.source_path.as_deref().map(std::path::Path::new),
        Some(notes_root.join("Untitled.txt").as_path())
    );
    assert!(
        config_file(dir.path()).is_file(),
        "the answer records the launch"
    );
    drop(first);

    let second = launch(dir.path());
    assert!(!second.first_run, "a config file means a later launch");
    assert!(finish(&second).is_none(), "a later launch mints nothing");
    assert_eq!(
        notes_folder_entries(&second),
        vec!["Untitled.txt".to_string()],
        "the file from the first launch is the only one, and it is still there"
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
        "one file, opened once"
    );
}

#[test]
fn the_format_the_screen_was_answered_with_is_written_and_is_what_gets_minted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = launch(dir.path());

    let note = finish_first_run_inner(&state, FileExtension::Md)
        .expect("the screen is answered")
        .expect("the first launch opens a file");

    assert_eq!(
        notes_folder_entries(&state),
        vec!["Untitled.md".to_string()],
        "Markdown was chosen, so the file is Markdown"
    );
    assert_eq!(opened_path(&note).file_name(), Some("Untitled.md".as_ref()));

    let written = std::fs::read_to_string(config_file(dir.path())).expect("the config");
    assert_eq!(
        writ_core::config::FilesConfig::from_config_text(&written).default_extension,
        FileExtension::Md,
        "the answer is on disk for the next launch to read"
    );
    assert_eq!(
        state.default_extension(),
        FileExtension::Md,
        "and in the copy every command answers from"
    );
}

#[test]
fn answering_the_screen_twice_mints_one_file_and_records_one_answer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = launch(dir.path());

    let first = finish(&state).expect("the first launch opens a file");
    let written = std::fs::read_to_string(config_file(dir.path())).expect("the config");

    let second =
        finish_first_run_inner(&state, FileExtension::Md).expect("the second call is answered");

    assert!(
        second.is_none(),
        "the launch has already been answered, so there is nothing to open"
    );
    assert_eq!(
        notes_folder_entries(&state),
        vec!["Untitled.txt".to_string()],
        "no second file beside the one the answer minted"
    );
    assert_eq!(
        std::fs::read_to_string(config_file(dir.path())).expect("the config"),
        written,
        "and the format the second call carried is not written over the first"
    );
    assert_eq!(state.default_extension(), FileExtension::Txt);
    assert_eq!(
        opened_path(&first).file_name(),
        Some("Untitled.txt".as_ref())
    );
}

#[test]
fn a_run_that_could_not_record_the_answer_can_be_answered_again() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = launch(dir.path());

    // The config has nowhere to land: a folder is sitting on its path. The
    // screen stays up, and the answer it is still holding is taken on the
    // retry once the way is clear.
    let config = config_file(dir.path());
    std::fs::create_dir_all(&config).expect("a folder where the config goes");
    assert!(finish_first_run_inner(&state, FileExtension::Md).is_err());
    assert!(
        notes_folder_entries(&state).is_empty(),
        "a run that recorded nothing mints nothing"
    );
    std::fs::remove_dir_all(&config).expect("the way is clear");

    let note = finish_first_run_inner(&state, FileExtension::Md)
        .expect("the screen is answered")
        .expect("the retry opens a file");

    assert_eq!(opened_path(&note).file_name(), Some("Untitled.md".as_ref()));
    assert_eq!(state.default_extension(), FileExtension::Md);
}

#[test]
fn a_later_launch_records_nothing_and_leaves_the_format_alone() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = launch(dir.path());
    finish(&first).expect("the first launch opens a file");
    drop(first);

    let second = launch(dir.path());
    assert!(!second.first_run);
    let before = std::fs::read_to_string(config_file(dir.path())).expect("the config");

    assert!(finish_first_run_inner(&second, FileExtension::Md)
        .expect("a later launch answers with nothing")
        .is_none());
    assert_eq!(
        std::fs::read_to_string(config_file(dir.path())).expect("the config"),
        before,
        "a later launch writes nothing"
    );
    assert_eq!(second.default_extension(), FileExtension::Txt);
    assert_eq!(
        notes_folder_entries(&second),
        vec!["Untitled.txt".to_string()],
        "and mints nothing"
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
fn a_first_line_renames_the_file_it_was_typed_in_and_keeps_its_format() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = launch(dir.path());
    let note = finish(&state).expect("a file");
    type_into(&state, &note, "Grocery list\n\nmilk\n");

    let outcome = auto_retitle_note_inner(&state, &note.id).expect("retitle");
    let RetitleOutcome::Renamed { note: renamed } = outcome else {
        panic!("a file nothing has touched is renamed");
    };
    assert_eq!(
        notes_folder_entries(&state),
        vec!["Grocery list.txt".to_string()],
        "the stem changes and the format does not"
    );
    assert_eq!(renamed.title, "Grocery list.txt");

    // Once. The file is no longer watched, so a later first line leaves it
    // where the person can now find it.
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
    let note = finish_first_run_inner(&state, FileExtension::Md)
        .expect("the screen is answered")
        .expect("a file");
    let path = opened_path(&note);

    type_into(&state, &note, "# Grocery list\n");
    state.retitle_watch.changed_outside(&path);

    let outcome = auto_retitle_note_inner(&state, &note.id).expect("retitle");
    let RetitleOutcome::Ask { title } = outcome else {
        panic!("a note something else has touched is asked about, not renamed");
    };
    assert_eq!(title, "Grocery list");
    assert_eq!(
        notes_folder_entries(&state),
        vec!["Untitled.md".to_string()],
        "nothing moved"
    );

    // Taking the offer lands the same file name the unasked rename would
    // have, and it carries the links with it: the offer is only reached
    // because something outside Writ already touched the note, which is when
    // a link naming it can exist.
    let journal = state.notes_root().join("Journal.md");
    std::fs::write(&journal, "see [[Untitled]]\n").expect("a note that links to it");
    state
        .notes_index
        .reconcile(&state.notes_root(), &|| false, &|_| false)
        .expect("index");

    let outcome = writ_tauri_lib::commands::notes::rename_note_with_links_inner(
        &state,
        &path.to_string_lossy(),
        &title,
        true,
    )
    .expect("rename");

    assert_eq!(
        outcome.updated, 1,
        "the note that linked to it was rewritten"
    );
    assert_eq!(
        std::fs::read_to_string(&journal).expect("the linking note"),
        "see [[Grocery list]]\n"
    );
    assert_eq!(
        notes_folder_entries(&state),
        vec!["Grocery list.md".to_string(), "Journal.md".to_string()]
    );
}

#[test]
fn a_file_with_no_first_line_yet_is_left_alone() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = launch(dir.path());
    let note = finish(&state).expect("a file");

    assert!(matches!(
        auto_retitle_note_inner(&state, &note.id).expect("retitle"),
        RetitleOutcome::NotYet
    ));
    assert_eq!(
        notes_folder_entries(&state),
        vec!["Untitled.txt".to_string()]
    );
}

#[test]
fn a_file_that_opens_with_frontmatter_keeps_its_name() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = launch(dir.path());
    let note = finish(&state).expect("a file");

    type_into(&state, &note, "---\ntitle: Grocery list\n---\n\nmilk\n");

    // Not `NotYet`: the fence is the first line for good, so nothing is
    // gained by reading the file again on every later save.
    assert!(matches!(
        auto_retitle_note_inner(&state, &note.id).expect("retitle"),
        RetitleOutcome::Skipped
    ));
    assert_eq!(
        notes_folder_entries(&state),
        vec!["Untitled.txt".to_string()]
    );
}

#[test]
fn a_launch_nobody_typed_in_still_records_itself_and_keeps_the_hint() {
    let dir = tempfile::tempdir().expect("tempdir");

    let first = launch(dir.path());
    finish(&first).expect("the first launch opens a file");
    drop(first);

    // Quit before the first keystroke: the config is there, and the one line
    // is still undismissed because nothing dismissed it.
    assert!(config_file(dir.path()).is_file());
    let second = launch(dir.path());
    let state = first_run_state_inner(&second).expect("first-run state");
    assert!(!state.first_run, "a config file means a later launch");
    assert!(!state.hint_dismissed, "nothing dismissed the line");
    assert!(finish(&second).is_none(), "a later launch mints nothing");
    assert_eq!(
        notes_folder_entries(&second),
        vec!["Untitled.txt".to_string()],
        "one file, from the launch that made it"
    );
}

#[test]
fn a_folder_of_plain_text_is_a_folder_with_work_in_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let notes = tempfile::tempdir().expect("notes dir");
    let existing = notes.path().join("notes.txt");
    std::fs::write(&existing, "My existing notes\n").expect("a plain-text file");

    // Somebody who deleted the config: the launch reads as a first one, and
    // the folder is not empty.
    let state = launch_with_notes(dir.path(), notes.path());
    assert!(state.first_run);
    let note = finish(&state).expect("the file that is already there");

    assert_eq!(
        notes_folder_entries(&state),
        vec!["notes.txt".to_string()],
        "nothing was minted"
    );
    assert_eq!(
        std::fs::read_to_string(&existing).expect("the file"),
        "My existing notes\n",
        "and nothing was written over"
    );
    assert_eq!(opened_path(&note).file_name(), existing.file_name());

    // A file Writ did not mint is never renamed from its own first line: only
    // minting arms the watch.
    assert!(state.retitle_watch.answer(&opened_path(&note)).is_none());
}

#[test]
fn a_folder_that_already_holds_files_opens_the_newest_one() {
    let dir = tempfile::tempdir().expect("tempdir");
    let notes = tempfile::tempdir().expect("notes dir");
    let older = notes.path().join("Older.md");
    let newer = notes.path().join("Newer.md");
    std::fs::write(&older, "Last month\n").expect("the older note");
    std::fs::write(&newer, "Yesterday\n").expect("the newer note");
    let an_hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    std::fs::File::options()
        .write(true)
        .open(&older)
        .expect("the older note")
        .set_modified(an_hour_ago)
        .expect("an older timestamp");

    let state = launch_with_notes(dir.path(), notes.path());
    let note = finish(&state).expect("the newest file");

    assert_eq!(opened_path(&note).file_name(), newer.file_name());
    assert_eq!(
        notes_folder_entries(&state),
        vec!["Newer.md".to_string(), "Older.md".to_string()],
        "nothing was minted"
    );
}

#[test]
fn a_rename_the_guard_refuses_leaves_the_file_answerable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = launch(dir.path());
    let note = finish(&state).expect("a file");
    let path = opened_path(&note);

    // The name the first line asks for is already taken, so the rename is
    // refused rather than made.
    let taken = state.notes_root().join("Grocery list.txt");
    std::fs::write(&taken, "somebody else's file\n").expect("the file in the way");
    type_into(&state, &note, "Grocery list\n");
    assert!(auto_retitle_note_inner(&state, &note.id).is_err());

    // The refusal answered nothing, so the file is still watched and the next
    // save still gets its name.
    assert!(state.retitle_watch.answer(&path).is_some());
    std::fs::remove_file(&taken).expect("the file in the way goes");
    assert!(matches!(
        auto_retitle_note_inner(&state, &note.id).expect("retitle"),
        RetitleOutcome::Renamed { .. }
    ));
    assert_eq!(
        notes_folder_entries(&state),
        vec!["Grocery list.txt".to_string()]
    );
}

#[test]
fn a_launch_that_finds_open_tabs_leaves_them_to_the_frontend() {
    let dir = tempfile::tempdir().expect("tempdir");
    let notes = tempfile::tempdir().expect("notes dir");
    std::fs::write(notes.path().join("Other.md"), "Something else\n").expect("a note");

    let first = launch_with_notes(dir.path(), notes.path());
    let opened = finish(&first).expect("the note that is already there");
    assert_eq!(opened_path(&opened).file_name(), Some("Other.md".as_ref()));
    drop(first);

    // The config goes, the database stays: the launch reads as a first one
    // again, but the row from the last session is still open, so the frontend
    // restores it and nothing here opens anything.
    std::fs::remove_file(config_file(dir.path())).expect("the config goes");
    let second = launch_with_notes(dir.path(), notes.path());
    assert!(second.first_run);
    assert!(
        finish(&second).is_none(),
        "the tabs the last session left are the launch's answer"
    );
    assert_eq!(
        notes_folder_entries(&second),
        vec!["Other.md".to_string()],
        "nothing was minted"
    );
}
