//! The index the app holds: one writer held for one file at a time, read
//! connections beside it, and the folder's names in memory (ADR-036
//! section 7).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use tempfile::TempDir;
use writ_core::buffer::manager::BufferManager;
use writ_core::notes::links::Resolution;
use writ_storage::buffer_store::BufferStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::notes_index::{self, IndexedNote, NotesIndex, NotesIndexStore};

/// How long a command issued in the middle of a walk may take before the test
/// calls it blocked. Far beyond any answer that did not wait for the walk.
const ANSWER_WITHIN: Duration = Duration::from_secs(10);

fn fixture() -> (TempDir, PathBuf, PathBuf) {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("writ.db");
    let conn = open_database(&db_path).expect("open_database");
    run_migrations(&conn).expect("migrations");
    drop(conn);
    let notes = dir.path().join("notes");
    std::fs::create_dir_all(&notes).expect("create notes dir");
    (dir, db_path, notes)
}

fn write(notes: &Path, rel: &str, body: &str) -> PathBuf {
    let path = notes.join(rel);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
    std::fs::write(&path, body).expect("write");
    path
}

fn walk(index: &NotesIndexStore, notes: &Path) {
    index
        .reconcile(notes, &|| false, &|_| false)
        .expect("reconcile");
}

fn key(path: &Path) -> String {
    notes_index::index_key(path)
}

/// Halfway through a walk, with the walk's own thread waiting on them, quick
/// open, link completion, content search, backlinks, link resolution, the
/// graph and the write Cmd+N makes all answer. A walk that held the index for
/// its whole length would keep every one of them waiting until it ended
/// (RC-03).
#[test]
fn commands_answer_while_a_walk_is_part_way_through() {
    let (_dir, db_path, notes) = fixture();
    for idx in 0..300 {
        write(
            &notes,
            &format!("{:02}/note-{idx:03}.md", idx % 10),
            &format!(
                "# Note {idx}\n\nrust words and [[note-{:03}]]\n",
                (idx + 1) % 300
            ),
        );
    }
    let index = Arc::new(NotesIndexStore::open(&db_path).expect("index"));
    let linked = key(&notes.join("02").join("note-002.md"));
    let linking = key(&notes.join("01").join("note-001.md"));
    let fresh = write(&notes, "zz/new.md", "# New\n");

    let entries = AtomicUsize::new(0);
    let answered: Mutex<Option<Result<Result<(), String>, mpsc::RecvTimeoutError>>> =
        Mutex::new(None);
    let cancelled = || {
        if entries.fetch_add(1, Ordering::SeqCst) == 150 {
            let (tx, rx) = mpsc::channel();
            let index = Arc::clone(&index);
            let notes = notes.clone();
            let (linked, linking, fresh) = (linked.clone(), linking.clone(), fresh.clone());
            std::thread::spawn(move || {
                let result = (|| -> Result<(), String> {
                    let query = writ_core::search::to_prefix_match("rust").expect("query");
                    let terms = writ_core::search::search_terms("rust");
                    index
                        .search_hits(&query, &terms, 100)
                        .map_err(|e| e.to_string())?;
                    index
                        .search_names("note-0", &notes, 50)
                        .map_err(|e| e.to_string())?;
                    index.backlinks(&linked).map_err(|e| e.to_string())?;
                    index
                        .resolve_link(&linking, "note-002")
                        .map_err(|e| e.to_string())?;
                    index.graph(&notes).map_err(|e| e.to_string())?;
                    index.index_path(&fresh).map_err(|e| e.to_string())?;
                    Ok(())
                })();
                let _ = tx.send(result);
            });
            *answered.lock().expect("lock") = Some(rx.recv_timeout(ANSWER_WITHIN));
        }
        false
    };
    let outcome = index
        .reconcile(&notes, &cancelled, &|_| false)
        .expect("reconcile");

    assert!(!outcome.cancelled);
    match answered.lock().expect("lock").take() {
        Some(Ok(Ok(()))) => {}
        Some(Ok(Err(error))) => panic!("a command failed during the walk: {error}"),
        Some(Err(_)) => panic!("a command issued during the walk waited for the walk"),
        None => panic!("the walk never reached its halfway point"),
    }
    assert_eq!(
        index
            .resolve_link(&key(&fresh), "note-002")
            .expect("resolve"),
        Resolution::Resolved(linked)
    );
}

/// Where the walk below stands with the note it found away from the folder.
#[derive(Debug)]
enum Away {
    /// Neither note reached yet.
    Waiting,
    /// The walk reached this note, and it was moved out and forgotten, the
    /// way the watcher forgets a deleted file.
    Gone(PathBuf),
    /// The walk moved on, and the note was put back and indexed again.
    Back(PathBuf),
}

/// A note the walk found missing, and that came back and was indexed again
/// before the walk ended, keeps the row that write left. The walk removes the
/// rows of the files it did not find only at its end, and a write made after
/// it started is newer than anything the walk saw.
///
/// The note is whichever of two the walk reaches first, so the walk always
/// reaches the other one after it, whatever order the folder lists them in.
#[test]
fn a_note_indexed_again_during_a_walk_keeps_its_row_when_the_walk_ends() {
    let (dir, db_path, notes) = fixture();
    let home = write(&notes, "Home.md", "see [[Plan]] and [[Kestrel]]\n");
    let candidates = [
        write(&notes, "a/Plan.md", "# Plan\n\nheron one\n"),
        write(&notes, "b/Kestrel.md", "# Kestrel\n\nheron two\n"),
    ];
    for idx in 0..10 {
        write(&notes, &format!("c/filler-{idx:02}.md"), "filler\n");
    }
    let index = NotesIndexStore::open(&db_path).expect("index");
    walk(&index, &notes);
    let outside = dir.path().join("outside");
    std::fs::create_dir_all(&outside).expect("create outside folder");
    let parked = |note: &Path| outside.join(note.file_name().expect("file name"));

    let state = RefCell::new(Away::Waiting);
    let hook = |path: &Path| {
        let mut state = state.borrow_mut();
        match &*state {
            Away::Waiting if candidates.iter().any(|note| note == path) => {
                std::fs::rename(path, parked(path)).expect("move the note out");
                index.forget_path(path).expect("forget the note");
                *state = Away::Gone(path.to_path_buf());
            }
            Away::Gone(note) => {
                let note = note.clone();
                std::fs::rename(parked(&note), &note).expect("put the note back");
                assert!(index.index_path(&note).expect("index the note"));
                *state = Away::Back(note);
            }
            _ => {}
        }
        false
    };
    let outcome = index
        .reconcile(&notes, &|| false, &hook)
        .expect("reconcile");

    let Away::Back(note) = state.into_inner() else {
        panic!("the walk did not reach a note after the one it found away");
    };
    assert_eq!(
        outcome.removed, 0,
        "the walk removed a note that is on disk"
    );
    assert!(
        index
            .snapshot()
            .expect("snapshot")
            .iter()
            .any(|(path, ..)| *path == key(&note)),
        "the note is on disk and the walk's end removed it from the index"
    );
    let name = note
        .file_stem()
        .expect("stem")
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        index.resolve_link(&key(&home), &name).expect("resolve"),
        Resolution::Resolved(key(&note))
    );
}

/// A save of a new note resolves the links that were waiting for it, and a
/// note leaving takes its links with it, without reading the folder's names.
#[test]
fn the_names_in_memory_follow_every_write_of_the_store() {
    let (_dir, db_path, notes) = fixture();
    let home = write(&notes, "Home.md", "see [[Plan]]\n");
    let index = NotesIndexStore::open(&db_path).expect("index");
    walk(&index, &notes);
    let home_key = key(&home);
    assert_eq!(
        index.resolve_link(&home_key, "Plan").expect("resolve"),
        Resolution::Missing
    );

    let plan = write(&notes, "Plan.md", "the plan\n");
    let note = IndexedNote::from_file(&plan).expect("note");
    index.upsert(&note, "the plan\n").expect("upsert");
    assert_eq!(
        index.resolve_link(&home_key, "Plan").expect("resolve"),
        Resolution::Resolved(key(&plan))
    );
    assert_eq!(
        index.links_from(&home_key).expect("links")[0].to_path,
        Some(key(&plan)),
        "the stored link was filled in by the save"
    );
    assert_eq!(index.backlinks(&key(&plan)).expect("backlinks").len(), 1);

    std::fs::remove_file(&plan).expect("remove");
    index.forget_path(&plan).expect("forget");
    assert_eq!(
        index.resolve_link(&home_key, "Plan").expect("resolve"),
        Resolution::Missing
    );
    assert_eq!(index.links_from(&home_key).expect("links")[0].to_path, None);
    assert!(index
        .search_names("Plan", &notes, 10)
        .expect("names")
        .is_empty());
}

/// A second note of one name makes the links naming it ambiguous, and its
/// removal hands them back to the note left standing.
#[test]
fn a_note_of_the_same_name_arriving_and_leaving_moves_the_links_that_name_it() {
    let (_dir, db_path, notes) = fixture();
    let home = write(&notes, "c/Home.md", "see [[Plan]]\n");
    let first = write(&notes, "a/Plan.md", "first\n");
    let index = NotesIndexStore::open(&db_path).expect("index");
    walk(&index, &notes);
    let home_key = key(&home);
    assert_eq!(
        index.links_from(&home_key).expect("links")[0].to_path,
        Some(key(&first))
    );

    let second = write(&notes, "b/Plan.md", "second\n");
    assert!(index.index_path(&second).expect("index_path"));
    assert_eq!(index.links_from(&home_key).expect("links")[0].to_path, None);
    let backlinks = index.backlinks(&key(&second)).expect("backlinks");
    assert_eq!(backlinks.len(), 1);
    assert_eq!(backlinks[0].candidates, vec![key(&first)]);

    std::fs::remove_file(&first).expect("remove");
    index.forget_path(&first).expect("forget");
    assert_eq!(
        index.links_from(&home_key).expect("links")[0].to_path,
        Some(key(&second))
    );
}

/// A write made through another connection is seen by the store's names the
/// next time they are read.
#[test]
fn the_names_follow_a_write_made_through_another_connection() {
    let (_dir, db_path, notes) = fixture();
    let home = write(&notes, "Home.md", "see [[Plan]]\n");
    let index = NotesIndexStore::open(&db_path).expect("index");
    walk(&index, &notes);
    assert!(index
        .candidate_paths("Plan")
        .expect("candidates")
        .is_empty());

    let plan = write(&notes, "Plan.md", "the plan\n");
    let other = open_database(&db_path).expect("second connection");
    let note = IndexedNote::from_file(&plan).expect("note");
    NotesIndex::new(&other)
        .upsert(&note, "the plan\n")
        .expect("upsert through another connection");

    assert_eq!(
        index.candidate_paths("Plan").expect("candidates"),
        vec![key(&plan)]
    );
    assert_eq!(
        index.resolve_link(&key(&home), "Plan").expect("resolve"),
        Resolution::Resolved(key(&plan))
    );
}

/// A note another connection added is in the names before the store's next
/// write resolves against them, so a save naming it links to it.
#[test]
fn a_save_after_another_connection_added_a_note_links_to_it() {
    let (_dir, db_path, notes) = fixture();
    let index = NotesIndexStore::open(&db_path).expect("index");
    walk(&index, &notes);
    assert!(index
        .candidate_paths("Plan")
        .expect("candidates")
        .is_empty());

    let plan = write(&notes, "Plan.md", "the plan\n");
    let other = open_database(&db_path).expect("second connection");
    NotesIndex::new(&other)
        .upsert(&IndexedNote::from_file(&plan).expect("note"), "the plan\n")
        .expect("upsert through another connection");

    let home = write(&notes, "Home.md", "see [[Plan]]\n");
    index
        .upsert(
            &IndexedNote::from_file(&home).expect("note"),
            "see [[Plan]]\n",
        )
        .expect("upsert");
    assert_eq!(
        index.links_from(&key(&home)).expect("links")[0].to_path,
        Some(key(&plan))
    );
}

/// The command line and the MCP server read the index read-only while the app
/// keeps writing it: a note the app indexes after they opened it is one they
/// can name.
#[test]
fn a_read_only_store_sees_notes_the_app_indexes_after_it_opened() {
    let (_dir, db_path, notes) = fixture();
    let home = write(&notes, "Home.md", "see [[Plan]]\n");
    let app = NotesIndexStore::open(&db_path).expect("app index");
    walk(&app, &notes);
    let reader = NotesIndexStore::open_read_only(&db_path).expect("read-only index");
    assert_eq!(
        reader.resolve_link(&key(&home), "Plan").expect("resolve"),
        Resolution::Missing
    );

    let plan = write(&notes, "Plan.md", "the plan\n");
    assert!(app.index_path(&plan).expect("index_path"));

    assert_eq!(
        reader.resolve_link(&key(&home), "Plan").expect("resolve"),
        Resolution::Resolved(key(&plan))
    );
    assert!(reader
        .search_names("Plan", &notes, 10)
        .expect("names")
        .iter()
        .any(|hit| hit.path == key(&plan)));
    assert!(
        reader.index_path(&plan).is_err(),
        "a read-only store refuses a write"
    );
}

/// A note the app forgets and a folder the app re-keys reach a read-only
/// store's names too: both move the names version it compares against.
#[test]
fn a_read_only_store_follows_notes_the_app_forgets_and_re_keys() {
    let (dir, db_path, notes) = fixture();
    let home = write(&notes, "Home.md", "see [[Plan]]\n");
    let plan = write(&notes, "Plan.md", "the plan\n");
    let app = NotesIndexStore::open(&db_path).expect("app index");
    walk(&app, &notes);
    let reader = NotesIndexStore::open_read_only(&db_path).expect("read-only index");
    assert_eq!(
        reader.resolve_link(&key(&home), "Plan").expect("resolve"),
        Resolution::Resolved(key(&plan))
    );

    std::fs::remove_file(&plan).expect("remove");
    app.forget_path(&plan).expect("forget");
    assert_eq!(
        reader.resolve_link(&key(&home), "Plan").expect("resolve"),
        Resolution::Missing
    );

    let moved = dir.path().join("moved");
    std::fs::rename(&notes, &moved).expect("move the folder");
    app.rekey_root(&notes, &moved).expect("rekey");
    assert_eq!(
        reader.candidate_paths("Home").expect("candidates"),
        vec![key(&moved.join("Home.md"))]
    );
}

/// A save through a buffer store that names the app's index is written
/// through it, so the index's names know the note at once.
#[test]
fn a_save_through_a_buffer_store_wired_to_the_index_is_in_its_names() {
    let (dir, db_path, notes) = fixture();
    let home = write(&notes, "Home.md", "see [[Launch]]\n");
    let index = Arc::new(NotesIndexStore::open(&db_path).expect("index"));
    walk(&index, &notes);
    assert!(index
        .candidate_paths("Launch")
        .expect("candidates")
        .is_empty());

    let buffers = dir.path().join("buffers");
    std::fs::create_dir_all(&buffers).expect("buffers");
    let mut store = BufferStore::new(open_database(&db_path).expect("store db"), buffers);
    store.set_notes_root(notes.clone());
    store.set_notes_index(Arc::clone(&index));
    let doc = BufferManager::new()
        .create_buffer(Some("Launch".to_string()))
        .expect("mint");
    store.insert(&doc).expect("insert");
    let launch = notes.join("Launch.md");
    store
        .attach_source_path(&doc.id, launch.to_str().expect("path"))
        .expect("attach");
    store.save_content(&doc.id, "launch day\n").expect("save");

    assert_eq!(
        index.resolve_link(&key(&home), "Launch").expect("resolve"),
        Resolution::Resolved(key(&launch))
    );
    assert_eq!(index.backlinks(&key(&launch)).expect("backlinks").len(), 1);
}
