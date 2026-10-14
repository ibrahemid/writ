//! Quick capture (ADR-055): the entry typed into the capture note, added to the
//! end of Today's File through the guard.

use std::sync::mpsc;
use std::sync::Mutex as StdMutex;

use chrono::{DateTime, Local, TimeZone, Utc};
use writ_core::buffer::document::BufferStatus;
use writ_core::config::FileExtension;
use writ_core::events::bus::WritEvent;
use writ_core::startup::dated_note_name;
use writ_core::watcher::change_event::ExternalChange;
use writ_tauri_lib::commands::buffer::save_buffer_content_inner;
use writ_tauri_lib::commands::capture::{capture_append_inner, capture_target_inner};
use writ_tauri_lib::commands::notes::todays_note_inner;
use writ_tauri_lib::state::AppState;

// `AppState::initialize` reads `WRIT_DATA_DIR` and `WRIT_NOTES_DIR`.
static ENV_LOCK: StdMutex<()> = StdMutex::new(());

fn launch(data_dir: &std::path::Path, notes_dir: &std::path::Path) -> AppState {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("WRIT_DATA_DIR", data_dir);
    std::env::set_var("WRIT_NOTES_DIR", notes_dir);
    let state = AppState::initialize().expect("app state");
    std::env::remove_var("WRIT_DATA_DIR");
    std::env::remove_var("WRIT_NOTES_DIR");
    state
}

fn noon() -> DateTime<Utc> {
    Local
        .with_ymd_and_hms(2026, 10, 15, 12, 0, 0)
        .earliest()
        .expect("a local time")
        .with_timezone(&Utc)
}

struct Fixture {
    _data: tempfile::TempDir,
    _notes: tempfile::TempDir,
    state: AppState,
    today: std::path::PathBuf,
}

fn fixture() -> Fixture {
    let data = tempfile::tempdir().expect("tempdir");
    let notes = tempfile::tempdir().expect("tempdir");
    let state = launch(data.path(), notes.path());
    let today = state
        .notes_root()
        .join(dated_note_name(noon(), FileExtension::Txt));
    Fixture {
        _data: data,
        _notes: notes,
        state,
        today,
    }
}

fn versions_of(state: &AppState, path: &std::path::Path) -> usize {
    let key = state
        .note_history
        .key_for(path)
        .expect("a key for the note");
    state.note_history.versions(&key).expect("versions").len()
}

#[test]
fn an_entry_goes_after_the_last_line_and_keeps_a_version() {
    let f = fixture();
    std::fs::write(&f.today, "first\n").expect("seed today's file");

    let outcome = capture_append_inner(&f.state, noon(), "second").expect("the entry lands");

    assert!(outcome.added);
    assert_eq!(outcome.name, dated_note_name(noon(), FileExtension::Txt));
    assert_eq!(
        std::fs::read_to_string(&f.today).unwrap(),
        "first\nsecond\n"
    );
    assert!(
        versions_of(&f.state, &f.today) > 0,
        "the append went past the version store"
    );
}

#[test]
fn a_day_with_no_file_gets_one_holding_the_entry_and_no_tab() {
    let f = fixture();

    capture_append_inner(&f.state, noon(), "buy milk").expect("the entry lands");

    assert_eq!(std::fs::read_to_string(&f.today).unwrap(), "buy milk\n");
    let rows = f
        .state
        .store
        .lock()
        .unwrap()
        .list_by_status(BufferStatus::Active)
        .expect("rows");
    assert!(rows.is_empty(), "capture opened a tab: {rows:?}");
    assert!(versions_of(&f.state, &f.today) > 0);
}

#[test]
fn a_blank_note_writes_nothing() {
    let f = fixture();

    let outcome = capture_append_inner(&f.state, noon(), "  \n\t").expect("not a failure");

    assert!(!outcome.added);
    assert!(!f.today.exists(), "a blank entry minted the day's file");
}

#[test]
fn a_crlf_file_keeps_its_line_endings() {
    let f = fixture();
    std::fs::write(&f.today, "a\r\nb\r\n").expect("seed");

    capture_append_inner(&f.state, noon(), "c").expect("the entry lands");

    assert_eq!(std::fs::read(&f.today).unwrap(), b"a\r\nb\r\nc\r\n");
}

#[test]
fn a_byte_order_mark_is_kept() {
    let f = fixture();
    std::fs::write(&f.today, b"\xEF\xBB\xBFfirst\n").expect("seed");

    capture_append_inner(&f.state, noon(), "second").expect("the entry lands");

    assert_eq!(
        std::fs::read(&f.today).unwrap(),
        b"\xEF\xBB\xBFfirst\nsecond\n"
    );
}

/// Long enough for the detector to settle on Windows-1252.
const FRENCH: &str = "Le caf\u{e9} est tr\u{e8}s bon, \u{e0} c\u{f4}t\u{e9} de la fen\u{ea}tre. \
D\u{e9}j\u{e0} vu, d\u{e9}j\u{e0} lu, d\u{e9}j\u{e0} \u{e9}crit.\n";

fn windows_1252(text: &str) -> Vec<u8> {
    writ_core::text_encoding::TextEncoding::windows_1252()
        .encode(text)
        .expect("Windows-1252 text")
        .into_owned()
}

#[test]
fn a_windows_1252_file_is_added_to_in_windows_1252() {
    let f = fixture();
    std::fs::write(&f.today, windows_1252(FRENCH)).expect("seed");

    capture_append_inner(&f.state, noon(), "cr\u{e8}me br\u{fb}l\u{e9}e").expect("the entry lands");

    let mut expected = windows_1252(FRENCH);
    expected.extend_from_slice(b"cr\xE8me br\xFBl\xE9e\n");
    assert_eq!(std::fs::read(&f.today).unwrap(), expected);
}

#[test]
fn an_entry_the_encoding_cannot_hold_writes_nothing() {
    let f = fixture();
    let before = windows_1252(FRENCH);
    std::fs::write(&f.today, &before).expect("seed");

    let error = capture_append_inner(&f.state, noon(), "\u{4e2d}").expect_err("turned down");

    assert!(error.starts_with("ERR_ENCODING_LOSSY"), "{error}");
    assert_eq!(std::fs::read(&f.today).unwrap(), before);
}

#[test]
fn a_folder_wearing_the_dated_name_is_reported_and_left_alone() {
    let f = fixture();
    std::fs::create_dir(&f.today).expect("a folder named for today");

    let error = capture_append_inner(&f.state, noon(), "buy milk").expect_err("turned down");

    assert!(error.contains("is already there"), "{error}");
    assert!(f.today.is_dir());
    assert!(capture_target_inner(&f.state, noon()).is_err());
}

#[cfg(unix)]
#[test]
fn a_dated_link_to_a_file_outside_the_folder_is_not_written_through() {
    let f = fixture();
    let elsewhere = tempfile::tempdir().expect("tempdir");
    let outside = elsewhere.path().join("diary.txt");
    std::fs::write(&outside, "private\n").expect("seed");
    std::os::unix::fs::symlink(&outside, &f.today).expect("link");

    let error = capture_append_inner(&f.state, noon(), "buy milk").expect_err("turned down");

    assert!(error.contains("outside your folder"), "{error}");
    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "private\n");
}

/// The tab holding Today's File is told what the file holds now, and a save
/// of the text it held before cannot write over the entry.
#[test]
fn a_tab_holding_the_file_is_told_and_cannot_write_over_the_entry() {
    let f = fixture();
    std::fs::write(&f.today, "first\n").expect("seed");
    let tab = todays_note_inner(&f.state, noon())
        .expect("today's file opens")
        .opened
        .doc
        .expect("into a tab");
    let (tx, rx) = mpsc::channel();
    f.state.event_bus.subscribe(move |event| {
        let _ = tx.send(event.clone());
    });

    capture_append_inner(&f.state, noon(), "second").expect("the entry lands");

    let told = rx.try_iter().any(|event| {
        matches!(
            event,
            WritEvent::BufferExternal { ref buffer_id, change: ExternalChange::Modified, .. }
                if *buffer_id == tab.id
        )
    });
    assert!(told, "the tab was not told the file changed");

    // The tab still believes the file holds "first\n". Its save is turned
    // down rather than landing over the entry.
    let refused = save_buffer_content_inner(&f.state, &tab.id, "first\nedited\n");
    assert!(
        refused.is_err(),
        "a stale save wrote over the captured entry"
    );
    assert_eq!(
        std::fs::read_to_string(&f.today).unwrap(),
        "first\nsecond\n"
    );
}
