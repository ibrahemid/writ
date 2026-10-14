//! The halves of a read, a save and a reindex a caller runs without holding
//! the store (ADR-047), and the size check that keeps a reindex from reading a
//! file the index does not take.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use tempfile::TempDir;
use writ_core::buffer::document::BufferDocument;
use writ_core::buffer::manager::BufferManager;
use writ_core::notes::links::{self, Resolution};
use writ_core::text_encoding::TextEncoding;
use writ_storage::buffer_store::{
    read_disk_state, read_doc_text, read_indexable_text, read_note_file, read_note_file_state,
    BufferStore,
};
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::errors::StorageError;
use writ_storage::notes_index::{self, IndexedNote, NotesIndexStore};

struct Fixture {
    _dir: TempDir,
    notes: PathBuf,
    index: Arc<NotesIndexStore>,
    store: BufferStore,
}

fn fixture() -> Fixture {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("writ.db");
    let conn = open_database(&db_path).expect("open_database");
    run_migrations(&conn).expect("migrations");
    let notes = dir.path().join("Writ");
    std::fs::create_dir_all(&notes).expect("notes dir");
    let notes = notes.canonicalize().expect("canonical notes");
    let index = Arc::new(NotesIndexStore::open(&db_path).expect("index"));
    let mut store = BufferStore::new(conn, dir.path().join("buffers"));
    store.set_notes_root(notes.clone());
    store.set_notes_index(Arc::clone(&index));
    Fixture {
        _dir: dir,
        notes,
        index,
        store,
    }
}

/// A row for `path`, recorded without indexing anything.
fn row(store: &BufferStore, path: &Path, size_bytes: u64, read_only: bool) -> BufferDocument {
    let doc = BufferManager::new()
        .open_external(path.to_string_lossy().into_owned())
        .expect("open");
    let doc = BufferDocument {
        size_bytes,
        read_only,
        ..doc
    };
    store.open_from_path_unindexed(&doc).expect("row");
    doc
}

fn indexed_paths(index: &NotesIndexStore) -> Vec<String> {
    index
        .snapshot()
        .expect("snapshot")
        .into_iter()
        .map(|(path, _, _)| path)
        .collect()
}

/// A named pipe: reading it blocks until something writes to it, so a reindex
/// that reads its file never returns on its own.
#[cfg(unix)]
fn pipe(path: &Path) {
    let status = std::process::Command::new("mkfifo")
        .arg(path)
        .status()
        .expect("mkfifo");
    assert!(status.success(), "mkfifo failed");
}

/// Runs a reindex of `id` on a thread and reports whether it returned without
/// reading `fifo`. A reindex that did read it is let go by opening the pipe
/// for writing, which ends its read.
#[cfg(unix)]
fn reindex_reads_nothing(store: BufferStore, id: String, fifo: &Path) -> bool {
    let (done, finished) = mpsc::channel();
    std::thread::spawn(move || {
        let outcome = store.reindex_buffer(&id);
        done.send(outcome.is_ok()).ok();
    });
    match finished.recv_timeout(Duration::from_secs(5)) {
        Ok(ok) => ok,
        Err(_) => {
            drop(std::fs::OpenOptions::new().write(true).open(fifo));
            let _ = finished.recv_timeout(Duration::from_secs(5));
            false
        }
    }
}

// Before the size check moved ahead of the read, the reindex read the whole
// file and only then found it too large to index.
#[cfg(unix)]
#[test]
fn a_reindex_of_a_row_over_the_normal_tier_reads_nothing() {
    let f = fixture();
    let fifo = f.notes.join("big.log");
    pipe(&fifo);
    let doc = row(
        &f.store,
        &fifo,
        writ_core::file_ops::THRESHOLD_NORMAL_BYTES + 1,
        false,
    );
    assert!(f.store.reindex_target(&doc.id).expect("target").is_none());
    assert!(
        reindex_reads_nothing(f.store, doc.id, &fifo),
        "the reindex read a file the index does not take"
    );
}

#[cfg(unix)]
#[test]
fn a_reindex_of_a_read_only_row_reads_nothing() {
    let f = fixture();
    let fifo = f.notes.join("generated.md");
    pipe(&fifo);
    let doc = row(&f.store, &fifo, 10, true);
    assert!(f.store.reindex_target(&doc.id).expect("target").is_none());
    assert!(
        reindex_reads_nothing(f.store, doc.id, &fifo),
        "the reindex read a read-only row's file"
    );
}

#[test]
fn a_note_row_is_a_reindex_target_and_its_text_lands_in_the_index() {
    let f = fixture();
    let path = f.notes.join("Plan.md");
    std::fs::write(&path, "# Plan\n").expect("write");
    let doc = row(&f.store, &path, 7, false);

    let target = f
        .store
        .reindex_target(&doc.id)
        .expect("target")
        .expect("a note in the folder is indexed");
    assert_eq!(target.id, doc.id);
    let text = read_indexable_text(&target);
    assert_eq!(text, "# Plan\n");
    f.store.reindex_with(&target, &text).expect("reindex");
    assert!(indexed_paths(&f.index).contains(&notes_index::index_key(&path)));
}

#[test]
fn a_file_outside_the_notes_folder_is_no_reindex_target() {
    let f = fixture();
    let outside = f._dir.path().join("outside.md");
    std::fs::write(&outside, "# Elsewhere\n").expect("write");
    let doc = row(&f.store, &outside, 12, false);
    assert!(f.store.reindex_target(&doc.id).expect("target").is_none());
}

#[test]
fn a_planned_write_lands_without_the_store_and_settles_back_under_it() {
    let f = fixture();
    let path = f.notes.join("Note.md");
    std::fs::write(&path, "one\r\ntwo\r\n").expect("write");
    let doc = row(&f.store, &path, 10, false);
    let doc = BufferDocument {
        line_ending: writ_core::notes::line_ending::LineEnding::CrLf,
        ..doc
    };
    f.store
        .set_line_ending(&doc.id, doc.line_ending)
        .expect("ending");
    let before = f.store.get(&doc.id).expect("row").updated_at;

    let planned = f
        .store
        .plan_source_write(&doc.id, "one\ntwo\nthree\n")
        .expect("plan");
    assert_eq!(planned.doc().id, doc.id);
    let outcome = planned.land(None, None).expect("land");
    assert_eq!(
        std::fs::read(&path).expect("read"),
        b"one\r\ntwo\r\nthree\r\n"
    );
    assert_eq!(
        Some(outcome.disk_state.hash),
        read_disk_state(&path)
            .expect("state")
            .map(|state| state.hash)
    );
    std::thread::sleep(Duration::from_millis(5));
    f.store
        .settle_source_write(&doc.id, &outcome)
        .expect("settle");
    assert!(f.store.get(&doc.id).expect("row").updated_at >= before);
}

#[test]
fn a_settled_write_records_the_size_it_wrote_on_the_row() {
    let f = fixture();
    let path = f.notes.join("Grown.md");
    std::fs::write(&path, "small\n").expect("write");
    let doc = row(&f.store, &path, 6, false);
    f.store
        .set_line_ending(&doc.id, writ_core::notes::line_ending::LineEnding::CrLf)
        .expect("ending");

    let grown = "line\n".repeat(4096);
    let planned = f.store.plan_source_write(&doc.id, &grown).expect("plan");
    let outcome = planned.land(None, None).expect("land");
    f.store
        .settle_source_write(&doc.id, &outcome)
        .expect("settle");

    let on_disk = std::fs::metadata(&path).expect("metadata").len();
    assert_eq!(on_disk, 4096 * 6, "the file carries its CRLF endings");
    assert_eq!(f.store.get(&doc.id).expect("row").size_bytes, on_disk);
}

#[test]
fn a_save_through_the_store_records_the_size_it_wrote_on_the_row() {
    let f = fixture();
    let path = f.notes.join("Saved.md");
    std::fs::write(&path, "small\n").expect("write");
    let doc = row(&f.store, &path, 6, false);

    let state = f
        .store
        .save_to_source_without_index(&doc.id, "a longer body than before\n", None, None)
        .expect("save");
    assert_eq!(state.size, 26);
    assert_eq!(f.store.get(&doc.id).expect("row").size_bytes, 26);

    f.store
        .save_to_source(&doc.id, "short\n", Some(state), None)
        .expect("save");
    assert_eq!(f.store.get(&doc.id).expect("row").size_bytes, 6);
}

#[test]
fn a_planned_write_keeps_the_guard_against_a_change_writ_never_read() {
    let f = fixture();
    let path = f.notes.join("Note.md");
    std::fs::write(&path, "first\n").expect("write");
    let doc = row(&f.store, &path, 6, false);
    let seen = read_disk_state(&path).expect("state").expect("file");
    std::fs::write(&path, "changed elsewhere\n").expect("write");

    let planned = f.store.plan_source_write(&doc.id, "mine\n").expect("plan");
    match planned.land(Some(seen), None) {
        Err(StorageError::SourceChangedOnDisk { .. }) => {}
        other => panic!("expected the guard to stop the write, got {other:?}"),
    }
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        "changed elsewhere\n"
    );
}

#[test]
fn a_read_only_row_has_no_write_to_plan() {
    let f = fixture();
    let path = f.notes.join("Generated.md");
    std::fs::write(&path, "made by writ\n").expect("write");
    let doc = row(&f.store, &path, 13, true);
    assert!(matches!(
        f.store.plan_source_write(&doc.id, "edit"),
        Err(StorageError::Consistency { .. })
    ));
}

#[test]
fn reading_a_row_in_hand_reads_what_the_store_reads() {
    let f = fixture();
    let path = f.notes.join("Note.md");
    std::fs::write(&path, "text\n").expect("write");
    let doc = row(&f.store, &path, 5, false);
    let in_hand = read_doc_text(&doc).expect("read");
    assert_eq!(in_hand, f.store.read_text(&doc.id).expect("read"));
    assert_eq!(in_hand.text, "text\n");
    assert_eq!(in_hand.bytes(), Some(&b"text\n"[..]));
}

#[test]
fn one_read_of_a_note_file_answers_its_state_and_its_bytes() {
    let f = fixture();
    let path = f.notes.join("Note.md");
    std::fs::write(&path, "a\r\nb\r\n").expect("write");
    let encoding = TextEncoding::utf8();
    let (state, bytes) = read_note_file(&path, encoding)
        .expect("read")
        .expect("file");
    assert_eq!(bytes, b"a\r\nb\r\n");
    let alone = read_note_file_state(&path, encoding)
        .expect("state")
        .expect("file");
    assert_eq!(state.disk.hash, alone.disk.hash);
    assert_eq!(state.comparison_hash, alone.comparison_hash);
    assert_eq!(
        Some(state.disk.hash),
        read_disk_state(&path).expect("disk").map(|disk| disk.hash)
    );
    assert!(read_note_file(&f.notes.join("missing.md"), encoding)
        .expect("read")
        .is_none());
    assert!(read_disk_state(&f.notes.join("missing.md"))
        .expect("read")
        .is_none());
}

#[test]
fn links_resolved_together_answer_as_each_one_alone() {
    let f = fixture();
    std::fs::create_dir_all(f.notes.join("a")).expect("folder");
    std::fs::create_dir_all(f.notes.join("b")).expect("folder");
    for rel in ["Note.md", "a/Twin.md", "b/Twin.md", "From.md"] {
        std::fs::write(f.notes.join(rel), "# x\n").expect("write");
    }
    f.index
        .reconcile(&f.notes, &|| false, &|_| false)
        .expect("reconcile");
    let from = notes_index::index_key(&f.notes.join("From.md"));
    let inners = ["Note", "Twin", "Nowhere", "a/Twin|alias", "Note#Heading"];
    let targets: Vec<_> = inners
        .iter()
        .map(|inner| links::parse_wikilink(inner))
        .collect();
    let together = f.index.resolve_links(&from, &targets).expect("batch");
    let alone: Vec<Resolution> = inners
        .iter()
        .map(|inner| f.index.resolve_link(&from, inner).expect("one"))
        .collect();
    assert_eq!(together, alone);
    assert!(matches!(together[0], Resolution::Resolved(_)));
    assert!(matches!(together[2], Resolution::Missing));
}

#[test]
fn a_write_to_the_index_waits_for_a_held_writer() {
    let f = fixture();
    let path = f.notes.join("Note.md");
    std::fs::write(&path, "# Note\n").expect("write");
    let note = IndexedNote::from_file(&path).expect("note");
    let index = Arc::clone(&f.index);
    let released = Arc::new(AtomicBool::new(false));
    let (started, start) = mpsc::channel();
    let writer = {
        let released = Arc::clone(&released);
        std::thread::spawn(move || {
            start.recv().expect("start");
            index.upsert(&note, "# Note\n").expect("upsert");
            released.load(Ordering::SeqCst)
        })
    };
    let answer = f.index.holding_writes(|| {
        started.send(()).expect("send");
        std::thread::sleep(Duration::from_millis(100));
        released.store(true, Ordering::SeqCst);
        7
    });
    assert_eq!(answer, 7);
    assert!(
        writer.join().expect("writer"),
        "the write landed while the writer was held"
    );
}
