//! A note is read and written in its file's text encoding (ADR-049).
//!
//! The editor works in UTF-8 and LF. The row carries the encoding beside the
//! line ending, a read decodes the file in it, and a save encodes back into
//! it or refuses when the text holds a character it has no bytes for.

use std::path::{Path, PathBuf};

use chrono::Utc;
use tempfile::TempDir;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_core::hash::{comparison_digest_hex, sha256_bytes};
use writ_core::notes::guard::DiskState;
use writ_core::notes::line_ending::LineEnding;
use writ_core::text_encoding::TextEncoding;
use writ_storage::buffer_store::{read_note_file_state, BufferStore, RecoveredText};
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::errors::StorageError;

const CP1252: &[u8] = b"caf\xe9,3.50\r\nth\xe9,2.00\r\n";

fn setup() -> (TempDir, BufferStore) {
    let dir = TempDir::new().expect("temp dir");
    let conn = open_database(&dir.path().join("test.db")).expect("open db");
    run_migrations(&conn).expect("migrations");
    let buffers_dir = dir.path().join("buffers");
    std::fs::create_dir_all(&buffers_dir).expect("buffers dir");
    (dir, BufferStore::new(conn, buffers_dir))
}

fn recorded(path: &Path) -> DiskState {
    let bytes = std::fs::read(path).expect("read");
    let metadata = std::fs::metadata(path).expect("metadata");
    DiskState {
        hash: sha256_bytes(&bytes),
        size: metadata.len(),
        mtime: metadata.modified().ok(),
    }
}

fn windows_1252() -> TextEncoding {
    TextEncoding::windows_1252()
}

/// A note on a file holding `bytes`, opened as the open path opens it: the
/// row carries the encoding and the ending.
fn open_note(
    store: &BufferStore,
    dir: &TempDir,
    bytes: &[u8],
    encoding: Option<TextEncoding>,
    line_ending: LineEnding,
) -> PathBuf {
    let path = dir.path().join("prices.csv");
    std::fs::write(&path, bytes).expect("write");
    let now = Utc::now();
    let doc = BufferDocument {
        id: "encoding-1".to_string(),
        title: "prices".to_string(),
        filename: "encoding-1.txt".to_string(),
        status: BufferStatus::Active,
        language: None,
        source_path: Some(path.to_string_lossy().into_owned()),
        cursor_pos: 0,
        scroll_pos: 0,
        tab_order: 0,
        created_at: now,
        updated_at: now,
        closed_at: None,
        read_only: false,
        size_bytes: bytes.len() as u64,
        line_ending,
        encoding,
    };
    store.open_from_path(&doc, "").expect("open");
    path
}

#[test]
fn the_row_keeps_its_encoding_and_reads_it_back() {
    let (dir, store) = setup();
    open_note(&store, &dir, CP1252, Some(windows_1252()), LineEnding::CrLf);
    assert_eq!(
        store.get("encoding-1").expect("row").encoding,
        Some(windows_1252())
    );

    let utf16 = TextEncoding::from_label("UTF-16LE BOM").expect("known");
    store
        .set_encoding("encoding-1", Some(utf16))
        .expect("record");
    assert_eq!(store.get("encoding-1").expect("row").encoding, Some(utf16));

    store.set_encoding("encoding-1", None).expect("record");
    assert_eq!(store.get("encoding-1").expect("row").encoding, None);
}

#[test]
fn a_read_decodes_the_file_in_its_encoding_and_hands_back_its_bytes() {
    let (dir, store) = setup();
    open_note(&store, &dir, CP1252, Some(windows_1252()), LineEnding::CrLf);

    let read = store.read_text("encoding-1").expect("reads");
    assert_eq!(read.text, "café,3.50\r\nthé,2.00\r\n");
    assert_eq!(read.bytes(), Some(CP1252));
    assert_eq!(
        store.read_content("encoding-1").expect("reads"),
        "café,3.50\r\nthé,2.00\r\n"
    );
}

#[test]
fn a_read_in_an_encoding_the_bytes_are_not_text_in_is_refused() {
    let (dir, store) = setup();
    open_note(
        &store,
        &dir,
        CP1252,
        Some(TextEncoding::utf8()),
        LineEnding::CrLf,
    );
    match store.read_text("encoding-1") {
        Err(StorageError::NotInEncoding { encoding }) => {
            assert_eq!(encoding, TextEncoding::utf8());
        }
        other => panic!("a lossy read handed over text: {other:?}"),
    }
}

#[test]
fn a_save_writes_the_files_own_encoding_and_ending_under_the_guard() {
    let (dir, store) = setup();
    let path = open_note(&store, &dir, CP1252, Some(windows_1252()), LineEnding::CrLf);

    let written = store
        .save_to_source(
            "encoding-1",
            "café,3.75\nthé,2.00\n",
            Some(recorded(&path)),
            None,
        )
        .expect("the guard compares the file's bytes and lets the save land");
    assert_eq!(
        std::fs::read(&path).expect("read"),
        b"caf\xe9,3.75\r\nth\xe9,2.00\r\n"
    );

    // The state the first save returned guards the second, which only holds
    // when it describes the bytes written rather than the editor's text.
    store
        .save_to_source("encoding-1", "café,4.00\nthé,2.00\n", Some(written), None)
        .expect("a second save lands");
    assert_eq!(
        std::fs::read(&path).expect("read"),
        b"caf\xe9,4.00\r\nth\xe9,2.00\r\n"
    );
}

#[test]
fn a_save_the_encoding_cannot_hold_is_refused_and_names_the_characters() {
    let (dir, store) = setup();
    let path = open_note(&store, &dir, CP1252, Some(windows_1252()), LineEnding::CrLf);

    let error = store
        .save_to_source(
            "encoding-1",
            "café 😀,3.50\nthé 中,2.00\n",
            Some(recorded(&path)),
            None,
        )
        .expect_err("Windows-1252 has no bytes for an emoji or a CJK character");
    match &error {
        StorageError::EncodingLossy {
            encoding,
            characters,
        } => {
            assert_eq!(*encoding, windows_1252());
            assert_eq!(characters, &vec!['😀', '中']);
        }
        other => panic!("refused for the wrong reason: {other:?}"),
    }
    assert_eq!(
        error.to_string(),
        "windows-1252 has no bytes for U+1F600 U+4E2D"
    );
    assert_eq!(
        std::fs::read(&path).expect("read"),
        CP1252,
        "nothing was written"
    );
    let siblings = std::fs::read_dir(dir.path())
        .expect("list")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("prices"))
        .count();
    assert_eq!(siblings, 1, "no copy was written beside it either");
}

#[test]
fn a_row_with_no_encoding_saves_utf8() {
    let (dir, store) = setup();
    let path = open_note(&store, &dir, b"cafe\n", None, LineEnding::Lf);
    store
        .save_to_source("encoding-1", "café\n", Some(recorded(&path)), None)
        .expect("saves");
    assert_eq!(std::fs::read(&path).expect("read"), "café\n".as_bytes());
}

#[test]
fn recovered_text_the_encoding_cannot_hold_is_set_aside_beside_the_note() {
    let (dir, store) = setup();
    let path = open_note(&store, &dir, CP1252, Some(windows_1252()), LineEnding::CrLf);

    let recovered = store
        .restore_recovered_content("encoding-1", "café 😀\n", None, Some(&|_| None))
        .expect("restore");
    let RecoveredText::SetAside { on_disk, copy } = recovered else {
        panic!("the text was written over a file that cannot hold it");
    };
    assert_eq!(on_disk, Some(recorded(&path)));
    assert_eq!(std::fs::read(&path).expect("read"), CP1252);
    assert_eq!(
        std::fs::read_to_string(copy).expect("the copy"),
        "café 😀\r\n"
    );
}

#[test]
fn recovered_text_is_written_back_in_the_files_encoding() {
    let (dir, store) = setup();
    let path = dir.path().join("prices.csv");
    let doc_path = open_note(&store, &dir, CP1252, Some(windows_1252()), LineEnding::CrLf);
    assert_eq!(doc_path, path);
    std::fs::remove_file(&path).expect("the file is gone before the relaunch");

    let recovered = store
        .restore_recovered_content("encoding-1", "café\n", None, Some(&|_| None))
        .expect("restore");
    assert!(matches!(recovered, RecoveredText::Restored(_)));
    assert_eq!(std::fs::read(&path).expect("read"), b"caf\xe9\r\n");
}

#[test]
fn the_comparison_digest_of_a_file_is_over_its_text() {
    let (dir, store) = setup();
    let path = open_note(&store, &dir, CP1252, Some(windows_1252()), LineEnding::CrLf);
    let state = read_note_file_state(&path, windows_1252())
        .expect("read")
        .expect("a file");
    assert_eq!(
        state.comparison_hash,
        comparison_digest_hex("café,3.50\nthé,2.00\n".as_bytes())
    );
    assert_eq!(state.disk.hash, sha256_bytes(CP1252));
}

#[test]
fn the_index_reads_a_note_in_another_encoding() {
    let dir = TempDir::new().expect("temp dir");
    let conn = open_database(&dir.path().join("test.db")).expect("open db");
    run_migrations(&conn).expect("migrations");
    let notes = dir.path().join("notes");
    std::fs::create_dir_all(&notes).expect("notes");
    std::fs::write(notes.join("Menu.md"), b"Cr\xe8me br\xfbl\xe9e et caf\xe9\n").expect("seed");

    writ_storage::notes_index::reconcile(&conn, &notes, &|| false, &|_| false).expect("walk");
    let raw = "brûlée";
    let query = writ_core::search::to_prefix_match(raw).expect("query");
    let terms = writ_core::search::search_terms(raw);
    let hits = writ_storage::notes_index::NotesIndex::new(&conn)
        .search_hits(&query, &terms, 50)
        .expect("search");
    assert_eq!(hits.len(), 1, "the note's words are found in the index");
}

#[test]
fn a_utf8_read_hands_over_the_bytes_it_read_as_the_text() {
    let (dir, store) = setup();
    open_note(&store, &dir, "café\n".as_bytes(), None, LineEnding::Lf);
    let read = store.read_text("encoding-1").expect("reads");
    assert_eq!(read.text, "café\n");
    assert_eq!(read.bytes(), Some("café\n".as_bytes()));

    let mark = b"\xef\xbb\xbfcaf\xc3\xa9\n";
    std::fs::write(dir.path().join("prices.csv"), mark).expect("write");
    let read = store.read_text("encoding-1").expect("reads");
    assert_eq!(read.text, "café\n", "the mark is not text");
    assert_eq!(read.bytes(), Some(&mark[..]));
}

#[test]
fn a_hex_row_of_a_utf16_file_stays_a_hex_view_until_it_is_opened_again() {
    let (dir, store) = setup();
    let utf16: Vec<u8> = b"\xff\xfe"
        .iter()
        .copied()
        .chain("dir\r\n".encode_utf16().flat_map(u16::to_le_bytes))
        .collect();
    open_note(&store, &dir, &utf16, None, LineEnding::Lf);
    store.set_read_only("encoding-1", true).expect("record");
    assert!(store.get("encoding-1").expect("row").read_only);

    let read = store.read_text("encoding-1").expect("reads");
    assert!(
        read.text.contains("ff fe"),
        "not the hex view: {}",
        read.text
    );

    store.set_read_only("encoding-1", false).expect("record");
    assert!(!store.get("encoding-1").expect("row").read_only);
}
