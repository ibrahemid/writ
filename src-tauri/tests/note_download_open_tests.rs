//! Opening a note whose bytes a sync provider has not put on this machine.
//!
//! The open must answer from a stat: `classify_path` sniffs the first bytes,
//! and on a placeholder that sniff blocks the IPC thread until the provider
//! has fetched the whole file. These tests drive the arm through the
//! dev-build dataless hook, so no provider is needed.

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
use writ_storage::notes_index::NotesIndexStore;
use writ_tauri_lib::commands::file::{
    authorize_download, clear_dataless_for_test, mark_dataless_for_test, open_file_from_path,
};
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

    AppState {
        store: Mutex::new(store),
        config_store,
        config: Mutex::new(WritConfig::default()),
        writ_dir,
        buffers_dir,
        notes_root: RwLock::new(notes_root),
        notes_root_fallback: RwLock::new(None),
        watcher_ignore: create_ignore_set(),
        watcher: Mutex::new(None),
        notes_watcher: Mutex::new(None),
        notes_index: Arc::new(NotesIndexStore::open(&db_path).expect("notes index db")),
        notes_index_cancel: Arc::new(AtomicBool::new(false)),
        quit: Arc::new(QuitState::new()),
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

/// Every test marks paths on its own thread; clear them so a later test on a
/// reused thread does not inherit a placeholder.
struct Marked;

impl Drop for Marked {
    fn drop(&mut self) {
        clear_dataless_for_test();
    }
}

fn mark(canonical: &str) -> Marked {
    mark_dataless_for_test(canonical);
    Marked
}

#[test]
fn opening_a_note_that_is_not_downloaded_reads_nothing_and_names_the_state() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let note = state.notes_root().join("away.md");
    std::fs::write(&note, "placeholder stand-in").unwrap();
    let canonical = canonicalize_for_authorization(&note).unwrap();
    let _marked = mark(&canonical);

    let result =
        open_file_from_path(&state, &canonical).expect("the open is answered, not refused");

    assert!(
        result.doc.is_none(),
        "nothing is registered for a placeholder"
    );
    let writ_core::file_ops::FileOpenMode::NotDownloaded { path, .. } = &result.mode else {
        panic!("expected the not-downloaded mode, got {:?}", result.mode);
    };
    assert_eq!(path, &canonical);
}

#[test]
fn a_note_that_is_not_downloaded_can_be_opened_again_once_it_arrives() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let note = dir.path().join("dropped.md");
    std::fs::write(&note, "# here now\n").unwrap();
    let canonical = canonicalize_for_authorization(&note).unwrap();
    state.authorized_paths.record_for_open(canonical.clone());

    // The first open spends the token and hands back the download state.
    let marked = mark(&canonical);
    let first = open_file_from_path(&state, &canonical).expect("open");
    assert!(first.doc.is_none());
    drop(marked);

    // The bytes land, and the frontend opens the note again. The token the
    // first open recorded is what authorizes this one.
    let second = open_file_from_path(&state, &canonical).expect("the second open succeeds");
    let doc = second.doc.expect("the note is registered once it is here");
    assert_eq!(doc.source_path.as_deref(), Some(canonical.as_str()));
}

#[test]
fn a_second_look_at_a_note_that_is_still_away_leaves_one_token() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let note = dir.path().join("still-away.md");
    std::fs::write(&note, "placeholder stand-in").unwrap();
    let canonical = canonicalize_for_authorization(&note).unwrap();
    state.authorized_paths.record_for_open(canonical.clone());

    // Every open of a note that is not here spends the token it was given and
    // records the one the next open needs, so asking again neither loses the
    // authorization nor stacks a second one up.
    let _marked = mark(&canonical);
    for _ in 0..3 {
        let result = open_file_from_path(&state, &canonical).expect("open");
        assert!(result.doc.is_none());
        assert_eq!(state.authorized_paths.pending_open_len(), 1);
    }
    assert!(state.authorized_paths.is_pending_open(&canonical));
}

#[test]
fn a_downloaded_note_opens_the_way_it_always_did() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let note = state.notes_root().join("here.md");
    std::fs::write(&note, "# here\n").unwrap();
    let canonical = canonicalize_for_authorization(&note).unwrap();

    let result = open_file_from_path(&state, &canonical).expect("open");
    assert_eq!(result.mode, writ_core::file_ops::FileOpenMode::Normal);
    assert!(result.doc.is_some());
}

#[test]
fn the_download_gate_keeps_the_open_token_for_the_open_that_follows() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let note = dir.path().join("outside.md");
    std::fs::write(&note, "bytes").unwrap();
    let canonical = canonicalize_for_authorization(&note).unwrap();
    state.authorized_paths.record_for_open(canonical.clone());

    let gated = authorize_download(&state, &canonical).expect("a pending open authorizes it");
    assert_eq!(gated, canonical);

    // Still spendable: the download did not consume it.
    let opened = open_file_from_path(&state, &canonical).expect("open");
    assert!(opened.doc.is_some());
}

#[test]
fn a_note_outside_every_root_can_still_be_dismissed() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    // The token is the only thing that authorizes this path, and closing the
    // tab is what gives it back, so the dismissal has to pass the same gate
    // the download did.
    let note = dir.path().join("outside.md");
    std::fs::write(&note, "placeholder stand-in").unwrap();
    let canonical = canonicalize_for_authorization(&note).unwrap();
    state.authorized_paths.record_for_open(canonical.clone());

    let gated = authorize_download(&state, &canonical).expect("the tab's token authorizes it");
    assert_eq!(gated, canonical);
    assert!(state.authorized_paths.discard_pending_open(&gated));
    assert_eq!(state.authorized_paths.pending_open_len(), 0);

    // And with the tab gone, the path is a stranger again.
    assert!(authorize_download(&state, &canonical).is_err());
}

#[test]
fn the_download_gate_refuses_a_path_nothing_authorized() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let secret = dir.path().join("secret.txt");
    std::fs::write(&secret, "not yours").unwrap();

    let result = authorize_download(&state, &secret.to_string_lossy());
    assert!(
        result.is_err(),
        "an unauthorized path may not be downloaded"
    );
    assert!(result.unwrap_err().contains("not authorized"));
}

#[test]
fn a_note_inside_the_notes_folder_may_be_downloaded() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let note = state.notes_root().join("mine.md");
    std::fs::write(&note, "bytes").unwrap();
    let canonical = canonicalize_for_authorization(&note).unwrap();

    assert_eq!(
        authorize_download(&state, &canonical).expect("containment authorizes it"),
        canonical
    );
}

#[test]
fn the_not_downloaded_result_names_the_provider_the_note_syncs_through() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    // An .stfolder marker is what a Syncthing folder is known by, and it says
    // so without depending on where this machine's home directory is.
    let synced = dir.path().join("Sync");
    std::fs::create_dir_all(synced.join(".stfolder")).unwrap();
    let note = synced.join("away.md");
    std::fs::write(&note, "placeholder stand-in").unwrap();
    let canonical = canonicalize_for_authorization(&note).unwrap();
    state.authorized_paths.record_for_open(canonical.clone());
    let _marked = mark(&canonical);

    let result = open_file_from_path(&state, &canonical).expect("open");
    let writ_core::file_ops::FileOpenMode::NotDownloaded { provider, .. } = &result.mode else {
        panic!("expected the not-downloaded mode, got {:?}", result.mode);
    };
    assert_eq!(provider.as_deref(), Some("Syncthing"));
}

#[test]
fn a_note_in_no_sync_folder_names_no_provider() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let note = state.notes_root().join("away.md");
    std::fs::write(&note, "placeholder stand-in").unwrap();
    let canonical = canonicalize_for_authorization(&note).unwrap();
    let _marked = mark(&canonical);

    let result = open_file_from_path(&state, &canonical).expect("open");
    let writ_core::file_ops::FileOpenMode::NotDownloaded { provider, .. } = &result.mode else {
        panic!("expected the not-downloaded mode, got {:?}", result.mode);
    };
    assert_eq!(*provider, None);
}
