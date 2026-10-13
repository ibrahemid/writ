//! `Today's Note` (spec D1): one file a day, named for the local calendar day.

use std::sync::Mutex as StdMutex;

use chrono::{DateTime, Local, TimeZone, Utc};
use writ_core::buffer::document::BufferDocument;
use writ_core::config::FileExtension;
use writ_core::file_ops::FileOpenMode;
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

/// The dated name in the format a launch with no config mints in.
fn dated_name(now: DateTime<Utc>) -> String {
    dated_note_name(now, FileExtension::Txt)
}

/// Today's file as the tab it opens into.
fn todays_tab(state: &AppState, now: DateTime<Utc>) -> BufferDocument {
    todays_note_inner(state, now)
        .expect("today's file")
        .opened
        .doc
        .expect("today's file opens into a tab")
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

    let first = todays_tab(&state, now);
    let second = todays_tab(&state, now);

    assert_eq!(
        first.id, second.id,
        "the second call opens the note the first one made"
    );
    assert_eq!(
        notes_folder_entries(&state),
        vec![dated_name(now)],
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

    let note = todays_tab(&state, now);

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
        Some("2026-03-18.txt"),
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

    let yesterday = todays_tab(&state, before);
    let today = todays_tab(&state, after);

    assert_ne!(yesterday.id, today.id, "a new day is a new note");
    assert_eq!(
        notes_folder_entries(&state),
        vec!["2026-03-17.txt".to_string(), "2026-03-18.txt".to_string()],
    );
}

#[test]
fn a_note_that_is_already_there_is_opened_and_keeps_every_word() {
    let data = tempfile::tempdir().expect("tempdir");
    let notes = tempfile::tempdir().expect("tempdir");
    let state = launch(data.path(), notes.path());
    let now = local_instant(2026, 3, 18, 9, 30, 0);

    let path = state.notes_root().join(dated_name(now));
    let written = "# Standup\n\n- shipped the index\n";
    std::fs::write(&path, written).expect("a note written before Writ asked");

    let note = todays_tab(&state, now);

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
        vec![dated_name(now)],
        "nothing was made beside it"
    );
}

#[test]
fn a_note_it_makes_is_in_the_index_with_no_walk_of_the_folder() {
    let data = tempfile::tempdir().expect("tempdir");
    let notes = tempfile::tempdir().expect("tempdir");
    let state = launch(data.path(), notes.path());
    let now = local_instant(2026, 3, 18, 9, 30, 0);

    todays_tab(&state, now);

    let key = notes_index::index_key(&state.notes_root().join(dated_name(now)));
    assert!(
        state
            .notes_index
            .note_paths()
            .expect("the index answers")
            .contains(&key),
        "the note is searchable and linkable the moment it exists"
    );
}

#[test]
fn a_folder_wearing_todays_name_is_reported_rather_than_worked_around() {
    let data = tempfile::tempdir().expect("tempdir");
    let notes = tempfile::tempdir().expect("tempdir");
    let state = launch(data.path(), notes.path());
    let now = local_instant(2026, 3, 18, 9, 30, 0);

    let name = dated_name(now);
    std::fs::create_dir(state.notes_root().join(&name)).expect("a folder in the way");

    todays_note_inner(&state, now).expect_err("today's name is not free");
    todays_note_inner(&state, now).expect_err("and asking again does not free it");

    assert_eq!(
        notes_folder_entries(&state),
        vec![name],
        "no second file for the day, however often it is asked for"
    );
}

#[test]
fn a_day_already_written_in_the_other_format_is_opened_rather_than_made_again() {
    let data = tempfile::tempdir().expect("tempdir");
    let notes = tempfile::tempdir().expect("tempdir");
    let state = launch(data.path(), notes.path());
    let now = local_instant(2026, 9, 21, 9, 30, 0);

    // The config says plain text; the day already has a Markdown note, from
    // before the format was changed.
    assert_eq!(state.default_extension(), FileExtension::Txt);
    let markdown = state
        .notes_root()
        .join(dated_note_name(now, FileExtension::Md));
    std::fs::write(&markdown, "# Standup\n").expect("the day's note");

    let note = todays_tab(&state, now);

    assert_eq!(
        opened_path(&note).file_name().and_then(|n| n.to_str()),
        Some("2026-09-21.md"),
        "the day's note is the one that is there"
    );
    assert_eq!(
        notes_folder_entries(&state),
        vec!["2026-09-21.md".to_string()],
        "never two files for one day"
    );
}

#[test]
fn a_day_with_no_note_yet_is_minted_in_the_configured_format() {
    let data = tempfile::tempdir().expect("tempdir");
    let notes = tempfile::tempdir().expect("tempdir");
    let state = launch(data.path(), notes.path());
    let now = local_instant(2026, 9, 21, 9, 30, 0);

    todays_tab(&state, now);

    assert_eq!(
        notes_folder_entries(&state),
        vec!["2026-09-21.txt".to_string()]
    );
}

#[test]
fn a_folder_wearing_the_other_formats_name_blocks_nothing() {
    let data = tempfile::tempdir().expect("tempdir");
    let notes = tempfile::tempdir().expect("tempdir");
    let state = launch(data.path(), notes.path());
    let now = local_instant(2026, 9, 21, 9, 30, 0);

    // The config says plain text, and something has left a folder at the
    // Markdown name. That name is only somewhere to read the day from, so the
    // day still gets its note.
    std::fs::create_dir(
        state
            .notes_root()
            .join(dated_note_name(now, FileExtension::Md)),
    )
    .expect("a folder in the way");

    let note = todays_tab(&state, now);

    assert_eq!(
        opened_path(&note).file_name().and_then(|n| n.to_str()),
        Some("2026-09-21.txt")
    );
    assert_eq!(
        notes_folder_entries(&state),
        vec!["2026-09-21.md".to_string(), "2026-09-21.txt".to_string()],
        "the folder in the way is still there and the day has its note"
    );
}

// A day's file over the size Writ opens without asking is the same question
// File > Open File asks: its path and size, and nothing read or registered.
#[test]
fn a_day_whose_file_is_over_fifty_mib_answers_the_question() {
    use std::io::Write;

    let data = tempfile::tempdir().expect("tempdir");
    let notes = tempfile::tempdir().expect("tempdir");
    let state = launch(data.path(), notes.path());
    let now = local_instant(2026, 3, 18, 9, 30, 0);

    let path = state.notes_root().join(dated_name(now));
    let mut file = std::fs::File::create(&path).expect("today's file");
    file.write_all(&b"a line of the day\n".repeat(512))
        .expect("text head");
    let size = writ_core::file_ops::THRESHOLD_LARGE_BYTES + 1;
    file.set_len(size).expect("extend");

    let today = todays_note_inner(&state, now).expect("an answer");

    assert_eq!(today.path, path.to_string_lossy());
    assert_eq!(today.opened.mode, FileOpenMode::LargeFileConfirm);
    assert_eq!(today.opened.size_bytes, size);
    assert!(
        today.opened.doc.is_none(),
        "nothing opens before the answer"
    );
}

// A day's file over the size Writ opens at all is refused as data, with the
// code the frontend words.
#[test]
fn a_day_whose_file_is_over_five_hundred_mib_is_refused() {
    let data = tempfile::tempdir().expect("tempdir");
    let notes = tempfile::tempdir().expect("tempdir");
    let state = launch(data.path(), notes.path());
    let now = local_instant(2026, 3, 18, 9, 30, 0);

    let path = state.notes_root().join(dated_name(now));
    std::fs::File::create(&path)
        .expect("today's file")
        .set_len(writ_core::file_ops::THRESHOLD_MAX_BYTES + 1)
        .expect("extend");

    let today = todays_note_inner(&state, now).expect("an answer");

    let FileOpenMode::Refused { reason } = &today.opened.mode else {
        panic!("expected a refusal, got {:?}", today.opened.mode);
    };
    assert!(reason.starts_with(writ_core::file_ops::ERR_OPEN_TOO_LARGE));
    assert!(today.opened.doc.is_none());
}

// A day that has no file yet is made, and the answer is its row and path.
#[test]
fn a_day_with_no_file_answers_the_file_it_made() {
    let data = tempfile::tempdir().expect("tempdir");
    let notes = tempfile::tempdir().expect("tempdir");
    let state = launch(data.path(), notes.path());
    let now = local_instant(2026, 3, 18, 9, 30, 0);

    let today = todays_note_inner(&state, now).expect("an answer");

    let doc = today.opened.doc.expect("the made file opens");
    assert_eq!(today.opened.mode, FileOpenMode::Normal);
    assert_eq!(doc.source_path.as_deref(), Some(today.path.as_str()));
    assert!(today.path.ends_with(&dated_name(now)));
}

// A day's file a tab already shows is that tab, whatever its size.
#[test]
fn a_day_whose_large_file_is_open_answers_its_tab() {
    use std::io::Write;

    let data = tempfile::tempdir().expect("tempdir");
    let notes = tempfile::tempdir().expect("tempdir");
    let state = launch(data.path(), notes.path());
    let now = local_instant(2026, 3, 18, 9, 30, 0);

    let path = state.notes_root().join(dated_name(now));
    let mut file = std::fs::File::create(&path).expect("today's file");
    file.write_all(&b"a line of the day\n".repeat(512))
        .expect("text head");
    file.set_len(writ_core::file_ops::THRESHOLD_LARGE_BYTES + 1)
        .expect("extend");
    let shown = writ_tauri_lib::commands::file::open_file_confirmed_from_path(
        &state,
        &path.to_string_lossy(),
    )
    .expect("open after Open")
    .doc
    .expect("row");

    let today = todays_tab(&state, now);

    assert_eq!(today.id, shown.id);
}
