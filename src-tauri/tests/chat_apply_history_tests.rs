//! The chat applier still hands the history store to the write, and a note
//! applied over earlier text keeps a version of what it held.

use jotra_storage::note_history::NoteHistoryStore;
use jotra_tauri_lib::commands::chat::{apply_proposal_inner, undo_proposal_inner};
use jotra_tauri_lib::watcher::open_files::NoOpenNotes;
use tempfile::TempDir;

#[test]
fn applying_a_proposal_keeps_what_the_note_held() {
    let dir = TempDir::new().expect("temp dir");
    let notes = std::fs::canonicalize(dir.path()).expect("canonical root");
    let jotra = notes.join(".jotra");
    std::fs::create_dir_all(&jotra).expect("jotra folder");

    let note = notes.join("Launch.md");
    std::fs::write(&note, "before\n").expect("seed a note");
    let before_hash = jotra_core::hash::sha256_hex(b"before\n");

    let store = NoteHistoryStore::open(&jotra).expect("history store");
    store.set_notes_root(notes.clone());
    let key = store.key_for(&note).expect("a key for the note");
    assert!(store.versions(&key).expect("versions").is_empty());

    let outcome = apply_proposal_inner(
        &notes,
        &NoOpenNotes,
        &jotra,
        "probe-host",
        &note.to_string_lossy(),
        "after\n",
        &before_hash,
        Some(&store),
        |_, _, _| unreachable!(),
    )
    .expect("the proposal applies");

    assert_eq!(outcome.path, "Launch.md");
    assert_eq!(outcome.bytes, "after\n".len() as u64);
    assert_eq!(
        std::fs::read_to_string(&note).expect("read back"),
        "after\n"
    );
    assert!(
        !store.versions(&key).expect("versions").is_empty(),
        "the applier no longer installs U8's history hook"
    );
}

/// The same call with no store writes the note and keeps nothing, which is the
/// MCP process's shape.
#[test]
fn applying_without_a_store_still_writes() {
    let dir = TempDir::new().expect("temp dir");
    let notes = std::fs::canonicalize(dir.path()).expect("canonical root");
    let jotra = notes.join(".jotra");
    std::fs::create_dir_all(&jotra).expect("jotra folder");

    let note = notes.join("Launch.md");
    std::fs::write(&note, "before\n").expect("seed a note");

    apply_proposal_inner(
        &notes,
        &NoOpenNotes,
        &jotra,
        "probe-host",
        &note.to_string_lossy(),
        "after\n",
        &jotra_core::hash::sha256_hex(b"before\n"),
        None,
        |_, _, _| unreachable!(),
    )
    .expect("the proposal applies");

    assert_eq!(
        std::fs::read_to_string(&note).expect("read back"),
        "after\n"
    );
}

/// A stale `before_hash` is still refused with the pane's own sentence and the
/// proposed text lands beside the note.
#[test]
fn a_stale_proposal_is_refused_with_a_copy() {
    let dir = TempDir::new().expect("temp dir");
    let notes = std::fs::canonicalize(dir.path()).expect("canonical root");
    let jotra = notes.join(".jotra");
    std::fs::create_dir_all(&jotra).expect("jotra folder");

    let note = notes.join("Launch.md");
    std::fs::write(&note, "somebody else wrote this\n").expect("seed a note");

    let refusal = apply_proposal_inner(
        &notes,
        &NoOpenNotes,
        &jotra,
        "probe-host",
        &note.to_string_lossy(),
        "what the model proposed\n",
        &jotra_core::hash::sha256_hex(b"what the pane last saw\n"),
        None,
        |_, _, _| unreachable!(),
    )
    .expect_err("the note changed under the offer");

    assert!(
        refusal.starts_with("Launch.md changed since the offer was made."),
        "{refusal}"
    );
    assert_eq!(
        std::fs::read_to_string(&note).expect("read back"),
        "somebody else wrote this\n"
    );
    let beside: Vec<String> = std::fs::read_dir(&notes)
        .expect("read the folder")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".md") && name != "Launch.md")
        .collect();
    assert_eq!(
        beside.len(),
        1,
        "the proposed text landed beside: {beside:?}"
    );
}

/// A notes folder holding `Launch.md` with `text`, its data folder, and a
/// history store pointed at the folder.
fn versioned(
    text: &str,
) -> (
    TempDir,
    std::path::PathBuf,
    std::path::PathBuf,
    NoteHistoryStore,
) {
    let dir = TempDir::new().expect("temp dir");
    let notes = std::fs::canonicalize(dir.path()).expect("canonical root");
    let jotra = notes.join(".jotra");
    std::fs::create_dir_all(&jotra).expect("jotra folder");
    std::fs::write(notes.join("Launch.md"), text).expect("seed a note");
    let store = NoteHistoryStore::open(&jotra).expect("history store");
    store.set_notes_root(notes.clone());
    (dir, notes, jotra, store)
}

/// Applies `after` over `before` and answers what the apply did.
fn apply_over(
    notes: &std::path::Path,
    jotra: &std::path::Path,
    store: &NoteHistoryStore,
    before: &str,
    after: &str,
) -> jotra_tauri_lib::commands::chat::ProposalOutcome {
    apply_proposal_inner(
        notes,
        &NoOpenNotes,
        jotra,
        "probe-host",
        "Launch.md",
        after,
        &jotra_core::hash::sha256_hex(before.as_bytes()),
        Some(store),
        |_, _, _| unreachable!("a note in the folder is not written through a tab"),
    )
    .expect("applied")
}

#[test]
fn undo_after_apply_puts_back_what_the_note_held() {
    let (_dir, notes, jotra, store) = versioned("before\n");
    let applied = apply_over(&notes, &jotra, &store, "before\n", "after\n");
    assert!(applied.changed);
    assert!(applied.undoable, "the apply kept the text it replaced");

    let undone = undo_proposal_inner(
        &notes,
        &NoOpenNotes,
        &jotra,
        "probe-host",
        "Launch.md",
        &jotra_core::hash::sha256_hex(b"before\n"),
        &applied.hash,
        Some(&store),
    )
    .expect("undone");

    assert!(undone.changed);
    assert_eq!(
        std::fs::read_to_string(notes.join("Launch.md")).expect("read"),
        "before\n"
    );
    let log = jotra_storage::activity_log::read_recent(&jotra, 10);
    assert!(
        log.iter().any(|record| record.action == "undo_proposal"),
        "the undo is its own line in the log"
    );
}

#[test]
fn undo_refuses_a_note_that_changed_after_the_apply() {
    let (_dir, notes, jotra, store) = versioned("before\n");
    let applied = apply_over(&notes, &jotra, &store, "before\n", "after\n");
    std::fs::write(notes.join("Launch.md"), "somebody kept writing\n").expect("write");

    let error = undo_proposal_inner(
        &notes,
        &NoOpenNotes,
        &jotra,
        "probe-host",
        "Launch.md",
        &jotra_core::hash::sha256_hex(b"before\n"),
        &applied.hash,
        Some(&store),
    )
    .expect_err("refused");

    assert!(
        error.starts_with("Launch.md changed since the change was applied."),
        "got: {error}"
    );
    assert_eq!(
        std::fs::read_to_string(notes.join("Launch.md")).expect("read"),
        "somebody kept writing\n",
        "the later change is not overwritten"
    );
}

#[test]
fn a_write_without_versions_offers_no_undo() {
    let (_dir, notes, jotra, store) = versioned("before\n");
    let unkept = apply_proposal_inner(
        &notes,
        &NoOpenNotes,
        &jotra,
        "probe-host",
        "Launch.md",
        "after\n",
        &jotra_core::hash::sha256_hex(b"before\n"),
        None,
        |_, _, _| unreachable!(),
    )
    .expect("applied");
    assert!(!unkept.undoable);

    let error = undo_proposal_inner(
        &notes,
        &NoOpenNotes,
        &jotra,
        "probe-host",
        "Launch.md",
        &jotra_core::hash::sha256_hex(b"before\n"),
        &unkept.hash,
        Some(&store),
    )
    .expect_err("nothing to put back");
    assert_eq!(error, "The earlier text of Launch.md is not kept.");

    let same = apply_over(&notes, &jotra, &store, "after\n", "after\n");
    assert!(!same.changed);
    assert!(
        !same.undoable,
        "a write that moved nothing has nothing to undo"
    );
}
