//! A recovery record the launch cannot read is reported and left in place.
//!
//! The record holds text that reached no file. Dropping it because it does not
//! decode loses that text for good with nothing in the log to say so; leaving
//! it where it is keeps it for whatever can read it later.
//!
//! One test in its own binary, because `initialize` reads `WRIT_DATA_DIR` and
//! `WRIT_NOTES_DIR` from the process environment and a second test in the same
//! binary would race it.

use std::io::Write;
use std::sync::{Arc, Mutex};

use tempfile::TempDir;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_tauri_lib::state::AppState;

/// Collects what the launch logs, for the assertion that it said something.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("log buffer").extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Captured {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().expect("log buffer")).into_owned()
    }
}

/// The record the session before the upgrade left behind: an unclean session
/// snapshot whose `buffers` entry holds a number where text belongs.
const LEGACY_STATE: &str = r#"{"buffers":{"note-1":42}}"#;

#[test]
fn a_recovery_record_that_cannot_be_decoded_is_logged_and_kept() {
    let data = TempDir::new().expect("data folder");
    let notes = TempDir::new().expect("notes folder");
    let db_path = data.path().join("writ.db");
    {
        let conn = open_database(&db_path).expect("open db");
        run_migrations(&conn).expect("migrations");
        conn.execute(
            "INSERT INTO session_snapshots (id, format_version, state_json, created_at, is_clean)
             VALUES ('legacy', 1, ?1, datetime('now'), 0)",
            [LEGACY_STATE],
        )
        .expect("insert the record");
    }

    let logs = Captured::default();
    let writer = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();

    std::env::set_var("WRIT_DATA_DIR", data.path());
    std::env::set_var("WRIT_NOTES_DIR", notes.path());
    let launched = tracing::subscriber::with_default(subscriber, AppState::initialize);
    std::env::remove_var("WRIT_DATA_DIR");
    std::env::remove_var("WRIT_NOTES_DIR");
    let launched = launched.expect("the launch must not fail on a record it cannot read");
    drop(launched);

    let logged = logs.text();
    assert!(
        logged.contains("could not be decoded"),
        "nothing in the log says a recovery record was unreadable:\n{logged}"
    );

    let conn = open_database(&db_path).expect("reopen db");
    let kept: String = conn
        .query_row(
            "SELECT state_json FROM session_snapshots WHERE id = 'legacy'",
            [],
            |row| row.get(0),
        )
        .expect("the record is still there");
    assert_eq!(kept, LEGACY_STATE, "the record was changed or emptied");
}
