//! The chat applier still hands the history store to the write, and a note
//! applied over earlier text keeps a version of what it held.

use tempfile::TempDir;
use writ_storage::note_history::NoteHistoryStore;
use writ_tauri_lib::commands::chat::apply_proposal_inner;

#[test]
fn applying_a_proposal_keeps_what_the_note_held() {
    let dir = TempDir::new().expect("temp dir");
    let notes = std::fs::canonicalize(dir.path()).expect("canonical root");
    let writ = notes.join(".writ");
    std::fs::create_dir_all(&writ).expect("writ folder");

    let note = notes.join("Launch.md");
    std::fs::write(&note, "before\n").expect("seed a note");
    let before_hash = writ_core::hash::sha256_hex(b"before\n");

    let store = NoteHistoryStore::open(&writ).expect("history store");
    store.set_notes_root(notes.clone());
    let key = store.key_for(&note).expect("a key for the note");
    assert!(store.versions(&key).expect("versions").is_empty());

    let outcome = apply_proposal_inner(
        &notes,
        &writ,
        "probe-host",
        &note.to_string_lossy(),
        "after\n",
        &before_hash,
        Some(&store),
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
    let writ = notes.join(".writ");
    std::fs::create_dir_all(&writ).expect("writ folder");

    let note = notes.join("Launch.md");
    std::fs::write(&note, "before\n").expect("seed a note");

    apply_proposal_inner(
        &notes,
        &writ,
        "probe-host",
        &note.to_string_lossy(),
        "after\n",
        &writ_core::hash::sha256_hex(b"before\n"),
        None,
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
    let writ = notes.join(".writ");
    std::fs::create_dir_all(&writ).expect("writ folder");

    let note = notes.join("Launch.md");
    std::fs::write(&note, "somebody else wrote this\n").expect("seed a note");

    let refusal = apply_proposal_inner(
        &notes,
        &writ,
        "probe-host",
        &note.to_string_lossy(),
        "what the model proposed\n",
        &writ_core::hash::sha256_hex(b"what the pane last saw\n"),
        None,
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
