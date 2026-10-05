//! Two writers on one version store.
//!
//! The app holds `history.db`, and `writ mcp` opens the same file to keep the
//! text a connected program's write replaced (ADR-031 rule 1.3). Two
//! connections can then reach a note nobody has versioned yet at the same
//! moment, and both have to find or make the one row that note gets: neither
//! capture may fail, and neither text may be lost.
//!
//! The two stores here are two connections in one process. SQLite's locks are
//! the same between two connections in one process as between two processes,
//! which `crates/writ-mcp/tests/history_two_writers.rs` drives for real.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};
use std::time::SystemTime;

use tempfile::TempDir;
use writ_storage::note_history::NoteHistoryStore;

/// How many fresh notes the two writers race on.
const ROUNDS: usize = 200;

fn store(writ: &Path, notes: &Path) -> NoteHistoryStore {
    let store = NoteHistoryStore::open(writ).expect("open the version store");
    store.set_notes_root(notes.to_path_buf());
    store
}

fn note(notes: &Path, round: usize) -> PathBuf {
    notes.join(format!("note-{round}.md"))
}

/// One writer: for every round, waits for the other at the line and keeps
/// its own text for that round's note.
fn race(
    store: Arc<NoteHistoryStore>,
    notes: PathBuf,
    barrier: Arc<Barrier>,
    side: &'static str,
) -> std::thread::JoinHandle<Vec<String>> {
    std::thread::spawn(move || {
        let mut failures = Vec::new();
        for round in 0..ROUNDS {
            let key = store
                .key_at(&note(&notes, round), None)
                .expect("a note in the folder has a key");
            barrier.wait();
            let text = format!("{side} {round}\n");
            if let Err(error) = store.capture_replaced(&key, text.as_bytes(), SystemTime::now()) {
                failures.push(format!("{side} round {round}: {error}"));
            }
        }
        failures
    })
}

#[test]
fn two_writers_reaching_one_new_note_at_once_both_keep_their_text() {
    let dir = TempDir::new().expect("temp dir");
    let writ = dir.path().to_path_buf();
    let notes = writ.join("notes");
    std::fs::create_dir_all(&notes).expect("notes folder");

    let app = Arc::new(store(&writ, &notes));
    let server = Arc::new(store(&writ, &notes));
    let barrier = Arc::new(Barrier::new(2));

    let app_side = race(Arc::clone(&app), notes.clone(), Arc::clone(&barrier), "app");
    let server_side = race(Arc::clone(&server), notes.clone(), barrier, "server");
    let mut failures = app_side.join().expect("the app side ran");
    failures.extend(server_side.join().expect("the server side ran"));

    assert_eq!(failures, Vec::<String>::new(), "every capture was kept");
    for round in 0..ROUNDS {
        let key = app
            .key_at(&note(&notes, round), None)
            .expect("a note in the folder has a key");
        let mut texts: Vec<Vec<u8>> = app
            .versions(&key)
            .expect("read the versions")
            .into_iter()
            .map(|entry| app.content(entry.id).expect("a version's text"))
            .collect();
        texts.sort();
        assert_eq!(
            texts,
            vec![
                format!("app {round}\n").into_bytes(),
                format!("server {round}\n").into_bytes()
            ],
            "round {round}: one note, both texts"
        );
    }
}
