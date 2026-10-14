//! A file in another text encoding opens, reads, saves and reopens in it
//! (ADR-049), through the real commands.
//!
//! Every save here is guarded by the record the open or the read left, so a
//! record of the editor's text in place of the file's bytes fails the save.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use tempfile::TempDir;
use writ_core::config::WritConfig;
use writ_core::events::bus::EventBus;
use writ_core::file_ops::FileOpenMode;
use writ_core::hash::comparison_digest_hex;
use writ_core::notes::line_ending::LineEnding;
use writ_core::preview::ContentRendererRegistry;
use writ_core::text_encoding::TextEncoding;
use writ_core::update::UpdatePhase;
use writ_core::watcher::reconcile::ReconcileGate;
use writ_plugin::transform::TransformRegistry;
use writ_storage::buffer_store::BufferStore;
use writ_storage::config_store::ConfigStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::layout_state::LayoutStateStore;
use writ_storage::notes_index::NotesIndexStore;
use writ_tauri_lib::commands::buffer::{
    close_buffer_inner, note_disk_state_inner, read_buffer_content_inner,
    reopen_with_encoding_inner, save_buffer_content_inner, NoteDiskAnswer,
};
use writ_tauri_lib::commands::file::open_file_from_path;
use writ_tauri_lib::preview::handler::RenderCache;
use writ_tauri_lib::quit::QuitState;
use writ_tauri_lib::security::{canonicalize_for_authorization, AuthorizedPaths};
use writ_tauri_lib::state::AppState;
use writ_tauri_lib::watcher::handler::create_ignore_set;

fn make_state(dir: &TempDir) -> AppState {
    let writ_dir = dir.path().to_path_buf();
    let buffers_dir = writ_dir.join("buffers");
    std::fs::create_dir_all(&buffers_dir).expect("buffers dir");

    let notes_root = writ_dir.join("Writ");
    std::fs::create_dir_all(&notes_root).expect("notes folder");
    let notes_root = writ_tauri_lib::security::canonicalize_root(&notes_root).expect("canonical");

    let db_path = writ_dir.join("writ.db");
    let conn = open_database(&db_path).expect("open db");
    run_migrations(&conn).expect("migrations");
    let store = BufferStore::new(conn, buffers_dir.clone());

    let config_path = writ_dir.join("config.toml");
    let config_store = ConfigStore::new(config_path);

    let note_history = Arc::new(
        writ_storage::note_history::NoteHistoryStore::open(&writ_dir).expect("version store"),
    );

    AppState {
        store: Mutex::new(store),
        config_store,
        config: Mutex::new(WritConfig::default()),
        writ_dir,
        buffers_dir,
        notes_root: RwLock::new(notes_root),
        first_run: false,
        first_run_finished: std::sync::atomic::AtomicBool::new(false),
        menu_apps: std::sync::Mutex::new(None),
        retitle_watch: std::sync::Arc::new(writ_tauri_lib::first_run::RetitleWatch::new()),
        notes_root_fallback: RwLock::new(None),
        watcher_ignore: create_ignore_set(),
        watcher: Mutex::new(None),
        notes_watcher: Mutex::new(None),
        open_file_watcher: Mutex::new(None),
        file_tracking: Mutex::new(None),
        notes_index: Arc::new(NotesIndexStore::open(&db_path).expect("notes index db")),
        note_history,
        notes_index_cancel: Arc::new(AtomicBool::new(false)),
        notes_reconcile: Arc::new(ReconcileGate::new()),
        quit: Arc::new(QuitState::new()),
        removal_holds: Default::default(),
        pending_opens: Mutex::new(Vec::new()),
        frontend_ready: AtomicBool::new(false),
        window_revealed: AtomicBool::new(false),
        window_dismissed: AtomicBool::new(false),
        transforms: RwLock::new(TransformRegistry::new()),
        event_bus: Arc::new(EventBus::new()),
        update_phase: Mutex::new(UpdatePhase::default()),
        authorized_paths: AuthorizedPaths::new(),
        preview_registry: Arc::new(RwLock::new(ContentRendererRegistry::new())),
        preview_render_cache: Arc::new(RenderCache::new()),
        layout_state: LayoutStateStore::new(open_database(&db_path).expect("layout db")),
        command_usage: writ_storage::command_usage::CommandUsageStore::new(
            open_database(&db_path).expect("usage db"),
        ),
        config_diagnostics: std::sync::Mutex::new(Vec::new()),
        recovered_buffers: Mutex::new(Vec::new()),
        was_dirty_shutdown: false,
        workspace_root: Mutex::new(None),
        workspace_watcher: Mutex::new(None),
        inbox_root: Mutex::new(None),
        inbox_watcher: Mutex::new(None),
        fts_scheduler: writ_tauri_lib::fts_scheduler::FtsScheduler::new(),
        workspace_index: Arc::new(RwLock::new(
            writ_tauri_lib::workspace_index::WorkspaceIndex::new(None),
        )),
        search_generation: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        last_disk_hash: Mutex::new(std::collections::HashMap::new()),
        source_records: Mutex::new(std::collections::HashMap::new()),
    }
}

/// Opens `path`, taking the single-use authorization the gate wants.
fn open(state: &AppState, path: &Path) -> String {
    let canonical = canonicalize_for_authorization(path).expect("canonical");
    state.authorized_paths.record_for_open(canonical.clone());
    open_file_from_path(state, &canonical)
        .expect("open")
        .doc
        .expect("the file opened")
        .id
}

const CP1252_CSV: &[u8] = b"name,note\r\nCaf\xe9 Nord,cr\xe8me br\xfbl\xe9e\r\n";
const CP1252_TEXT: &str = "name,note\r\nCafé Nord,crème brûlée\r\n";

fn utf16le_with_bom(text: &str) -> Vec<u8> {
    b"\xff\xfe"
        .iter()
        .copied()
        .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
        .collect()
}

fn encoding_of(state: &AppState, id: &str) -> Option<String> {
    state
        .store
        .lock()
        .expect("store")
        .get(id)
        .expect("get")
        .encoding
        .map(TextEncoding::label)
}

fn read(state: &AppState, id: &str) -> String {
    String::from_utf8(read_buffer_content_inner(state, id).expect("reads")).expect("UTF-8 text")
}

#[test]
fn a_windows_1252_csv_opens_reads_and_saves_in_windows_1252() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = dir.path().join("menu.csv");
    std::fs::write(&file, CP1252_CSV).unwrap();

    let canonical = canonicalize_for_authorization(&file).expect("canonical");
    state.authorized_paths.record_for_open(canonical.clone());
    let opened = open_file_from_path(&state, &canonical).expect("opens");
    assert_eq!(opened.mode, FileOpenMode::Normal);
    let doc = opened.doc.expect("a row");
    assert_eq!(
        serde_json::to_value(&doc).expect("the open's row as JSON")["encoding"],
        "windows-1252",
        "the open answer carries the encoding beside the line ending"
    );
    assert_eq!(doc.line_ending, LineEnding::CrLf);

    assert_eq!(read(&state, &doc.id), CP1252_TEXT);
    save_buffer_content_inner(&state, &doc.id, "name,note\nCafé Süd,crème brûlée\n")
        .expect("the save lands under the guard");
    assert_eq!(
        std::fs::read(&file).unwrap(),
        b"name,note\r\nCaf\xe9 S\xfcd,cr\xe8me br\xfbl\xe9e\r\n"
    );
    save_buffer_content_inner(&state, &doc.id, "name,note\nCafé Ost,crème brûlée\n")
        .expect("and so does the next one");
    assert_eq!(
        std::fs::read(&file).unwrap(),
        b"name,note\r\nCaf\xe9 Ost,cr\xe8me br\xfbl\xe9e\r\n"
    );
}

#[test]
fn the_digest_the_tab_is_handed_matches_the_text_it_holds() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = dir.path().join("menu.csv");
    std::fs::write(&file, CP1252_CSV).unwrap();
    let id = open(&state, &file);

    let NoteDiskAnswer::Described { disk } = note_disk_state_inner(&state, &id).expect("asks")
    else {
        panic!("the file was not described");
    };
    assert_eq!(
        disk.hash,
        comparison_digest_hex(LineEnding::normalise(CP1252_TEXT).as_bytes()),
        "a tab holding the file's text would read as edited"
    );
}

#[test]
fn a_powershell_utf16_file_opens_as_text_and_saves_as_utf16() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = dir.path().join("out.txt");
    let listing = "\r\n    Directory: C:\\Users\\ibra\r\n\r\nMode   Name\r\nd----  Documents\r\n";
    std::fs::write(&file, utf16le_with_bom(listing)).unwrap();

    let canonical = canonicalize_for_authorization(&file).expect("canonical");
    state.authorized_paths.record_for_open(canonical.clone());
    let opened = open_file_from_path(&state, &canonical).expect("opens");
    assert_eq!(opened.mode, FileOpenMode::Normal, "not the hex view");
    let doc = opened.doc.expect("a row");
    assert!(!doc.read_only);
    assert_eq!(
        encoding_of(&state, &doc.id).as_deref(),
        Some("UTF-16LE BOM")
    );
    assert_eq!(doc.line_ending, LineEnding::CrLf);
    assert_eq!(read(&state, &doc.id), listing);

    let edited = format!("{}-a---  notes.txt\n", LineEnding::normalise(listing));
    save_buffer_content_inner(&state, &doc.id, &edited).expect("saves");
    assert_eq!(
        std::fs::read(&file).unwrap(),
        utf16le_with_bom(&format!("{listing}-a---  notes.txt\r\n"))
    );
}

#[test]
fn a_lossy_save_is_refused_with_the_characters_named() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = dir.path().join("menu.csv");
    std::fs::write(&file, CP1252_CSV).unwrap();
    let id = open(&state, &file);
    read(&state, &id);

    let error = save_buffer_content_inner(&state, &id, "name,note\nCafé 😀,中\n")
        .expect_err("Windows-1252 cannot hold these");
    assert!(
        error.starts_with("ERR_ENCODING_LOSSY: windows-1252 has no bytes for U+1F600 U+4E2D"),
        "the refusal is not in the shape the editor reads: {}",
        error
    );
    assert_eq!(
        std::fs::read(&file).unwrap(),
        CP1252_CSV,
        "nothing was written"
    );
}

#[test]
fn reopening_with_another_encoding_reads_the_file_in_it() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = dir.path().join("menu.csv");
    std::fs::write(&file, CP1252_CSV).unwrap();
    let id = open(&state, &file);
    assert_eq!(encoding_of(&state, &id).as_deref(), Some("windows-1252"));

    let reopened = reopen_with_encoding_inner(&state, &id, "windows-1250").expect("reopens");
    assert_eq!(
        reopened.encoding.map(TextEncoding::label).as_deref(),
        Some("windows-1250")
    );
    assert_eq!(
        read(&state, &id),
        "name,note\r\nCaf\u{e9} Nord,cr\u{10d}me br\u{171}l\u{e9}e\r\n"
    );

    let refused = reopen_with_encoding_inner(&state, &id, "UTF-8").expect_err("not UTF-8");
    assert!(refused.starts_with("ERR_READ_NOT_UTF8:"), "{refused}");
    assert_eq!(
        encoding_of(&state, &id).as_deref(),
        Some("windows-1250"),
        "a refused reopen keeps the encoding the note had"
    );
    let unknown = reopen_with_encoding_inner(&state, &id, "klingon").expect_err("unknown");
    assert!(unknown.starts_with("ERR_UNKNOWN_ENCODING:"), "{unknown}");

    reopen_with_encoding_inner(&state, &id, "windows-1252").expect("back");
    assert_eq!(read(&state, &id), CP1252_TEXT);
    assert_eq!(
        std::fs::read(&file).unwrap(),
        CP1252_CSV,
        "a reopen writes nothing"
    );
}

#[test]
fn the_version_kept_before_a_save_is_the_files_own_bytes() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let root = state.notes_root();
    state.note_history.set_notes_root(root.clone());
    state.note_history.set_probe(Arc::new(
        writ_tauri_lib::watcher::identity::PlatformIdentity,
    ));
    {
        let mut store = state.store.lock().expect("store");
        store.set_notes_root(root.clone());
        store.set_history(Arc::clone(&state.note_history));
    }
    let file = root.join("Menu.txt");
    std::fs::write(&file, CP1252_CSV).unwrap();
    let id = open(&state, &file);
    read(&state, &id);
    save_buffer_content_inner(&state, &id, "name,note\nCafé Süd,crème brûlée\n").expect("saves");

    let key = state
        .note_history
        .key_for(&file)
        .expect("a note in the notes folder");
    let kept: Vec<Vec<u8>> = state
        .note_history
        .versions(&key)
        .expect("versions")
        .into_iter()
        .map(|entry| state.note_history.content(entry.id).expect("content"))
        .collect();
    assert!(
        kept.iter().any(|bytes| bytes.as_slice() == CP1252_CSV),
        "the version from before the save is the file as it was, byte for byte"
    );
}

#[test]
fn a_utf16_file_kept_as_a_hex_view_opens_as_text_when_it_is_opened_again() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = dir.path().join("out.txt");
    std::fs::write(&file, utf16le_with_bom("dir\r\n")).unwrap();
    let id = open(&state, &file);
    // The row as a build before UTF-16 was text left it: a hex view.
    {
        let store = state.store.lock().expect("store");
        store.set_read_only(&id, true).expect("record");
        store.set_encoding(&id, None).expect("record");
    }
    close_buffer_inner(&state, &id).expect("close");

    assert_eq!(open(&state, &file), id, "the closed tab comes back");
    let row = state.store.lock().expect("store").get(&id).expect("get");
    assert!(!row.read_only);
    assert_eq!(
        row.encoding.map(TextEncoding::label).as_deref(),
        Some("UTF-16LE BOM")
    );
    assert_eq!(read(&state, &id), "dir\r\n");
}

#[test]
fn reopening_a_file_that_is_gone_says_so() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let file = dir.path().join("menu.csv");
    std::fs::write(&file, CP1252_CSV).unwrap();
    let id = open(&state, &file);
    std::fs::remove_file(&file).unwrap();
    let refused = reopen_with_encoding_inner(&state, &id, "UTF-8").expect_err("no file");
    assert!(refused.starts_with("ERR_READ_FILE_MISSING:"), "{refused}");
}
