//! Acceptance coverage for `commands::file::open_generated_document`
//! (ADR-028 §1): a document Writ writes rather than the user must never mint
//! a file in the notes folder, must land at a fixed, rewritable path under
//! the data directory, and must refuse a save through the ordinary read-only
//! mechanism. A plain user note is unaffected and still mints into the notes
//! folder on its first keystroke.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use tempfile::TempDir;
use writ_core::config::WritConfig;
use writ_core::events::bus::EventBus;
use writ_core::preview::ContentRendererRegistry;
use writ_core::update::UpdatePhase;
use writ_plugin::transform::TransformRegistry;
use writ_storage::buffer_store::BufferStore;
use writ_storage::config_store::ConfigStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::layout_state::LayoutStateStore;
use writ_tauri_lib::commands::buffer::save_buffer_content_inner;
use writ_tauri_lib::commands::file::open_generated_document;
use writ_tauri_lib::preview::handler::RenderCache;
use writ_tauri_lib::security::AuthorizedPaths;
use writ_tauri_lib::state::AppState;
use writ_tauri_lib::watcher::handler::create_ignore_set;

const NOTICES_TITLE: &str = "Third-party licences";

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

    AppState {
        store: Mutex::new(store),
        config_store,
        config: Mutex::new(WritConfig::default()),
        writ_dir,
        buffers_dir,
        notes_root,
        notes_root_fallback: None,
        watcher_ignore: create_ignore_set(),
        watcher: Mutex::new(None),
        pending_opens: Mutex::new(Vec::new()),
        frontend_ready: AtomicBool::new(false),
        transforms: RwLock::new(TransformRegistry::new()),
        event_bus: Arc::new(EventBus::new()),
        update_phase: Mutex::new(UpdatePhase::default()),
        authorized_paths: AuthorizedPaths::new(),
        preview_registry: Arc::new(RwLock::new(ContentRendererRegistry::new())),
        preview_render_cache: Arc::new(RenderCache::new()),
        layout_state: LayoutStateStore::new(open_database(&db_path).expect("layout db")),
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
    }
}

fn is_empty_dir(dir: &std::path::Path) -> bool {
    std::fs::read_dir(dir)
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(true)
}

#[test]
fn opening_a_generated_document_creates_no_file_in_the_notes_folder() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let result = open_generated_document(&state, NOTICES_TITLE, "# Third-party notices\nMIT\n")
        .expect("open");

    assert!(
        is_empty_dir(&state.notes_root),
        "a generated document must never mint into the notes folder"
    );
    let expected = state
        .writ_dir
        .join("generated")
        .join(format!("{NOTICES_TITLE}.md"));
    assert_eq!(
        std::fs::read_to_string(&expected).unwrap(),
        "# Third-party notices\nMIT\n"
    );
    let expected_canonical = writ_tauri_lib::security::canonicalize_for_authorization(&expected)
        .expect("canonicalize expected path");
    assert_eq!(
        result.doc.source_path.as_deref(),
        Some(expected_canonical.as_str()),
        "the buffer points at the file under the data directory"
    );
    assert!(result.doc.read_only, "a generated document is read-only");
}

#[test]
fn a_second_open_rewrites_the_same_file_in_place() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let first = open_generated_document(&state, NOTICES_TITLE, "first version").expect("open");
    let second = open_generated_document(&state, NOTICES_TITLE, "second version").expect("reopen");

    assert_eq!(first.doc.id, second.doc.id, "the same buffer is reused");
    let generated_dir = state.writ_dir.join("generated");
    let entries: Vec<_> = std::fs::read_dir(&generated_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(
        entries.len(),
        1,
        "reopening must not mint a dedupe sibling: {entries:?}"
    );
    let expected = generated_dir.join(format!("{NOTICES_TITLE}.md"));
    assert_eq!(
        std::fs::read_to_string(&expected).unwrap(),
        "second version",
        "the file is rewritten in place"
    );

    let store = state.store.lock().unwrap();
    let active = store
        .list_by_status(writ_core::buffer::document::BufferStatus::Active)
        .unwrap();
    assert_eq!(
        active.len(),
        1,
        "reopening must not leave a second buffer row behind"
    );
}

#[test]
fn saving_a_generated_document_is_refused() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let opened = open_generated_document(&state, NOTICES_TITLE, "original text").expect("open");

    let err = save_buffer_content_inner(&state, &opened.doc.id, "hijacked")
        .expect_err("a read-only buffer must refuse a save");
    assert!(err.contains("read-only"), "unexpected error: {err}");

    let expected = state
        .writ_dir
        .join("generated")
        .join(format!("{NOTICES_TITLE}.md"));
    assert_eq!(
        std::fs::read_to_string(&expected).unwrap(),
        "original text",
        "the refused save must not have touched the file"
    );
}

#[test]
fn a_plain_new_note_still_mints_a_dated_file_in_the_notes_folder() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let doc = {
        let store = state.store.lock().unwrap();
        let mut mgr = writ_core::buffer::manager::BufferManager::new();
        let doc = mgr.create_buffer(None).expect("mint");
        store.insert(&doc).expect("persist");
        doc
    };

    save_buffer_content_inner(&state, &doc.id, "just notes").expect("save");

    let expected = state.notes_root.join(format!(
        "{}.md",
        writ_core::notes::date_stem(doc.created_at)
    ));
    assert_eq!(std::fs::read_to_string(&expected).unwrap(), "just notes");
    assert!(
        is_empty_dir(&state.writ_dir.join("generated")),
        "a user note has no business under the generated documents folder"
    );
}
