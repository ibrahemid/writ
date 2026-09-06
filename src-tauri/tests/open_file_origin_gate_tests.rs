use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use tempfile::TempDir;
use writ_core::config::WritConfig;
use writ_core::events::bus::{EventBus, WritEvent};
use writ_core::preview::ContentRendererRegistry;
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
    close_buffer_inner, close_buffers_inner, delete_buffer_inner, read_buffer_content_inner,
    save_buffer_content_inner,
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
        open_file_watcher: Mutex::new(None),
        file_tracking: Mutex::new(None),
        notes_index: Arc::new(NotesIndexStore::open(&db_path).expect("notes index db")),
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
        unsaved_on_exit: Mutex::new(std::collections::HashMap::new()),
    }
}

#[test]
fn open_file_allows_path_inside_open_workspace() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let ws = TempDir::new().unwrap();
    let note = ws.path().join("note.md");
    std::fs::write(&note, "workspace file").unwrap();

    {
        let mut root = state.workspace_root.lock().unwrap();
        *root = Some(writ_tauri_lib::security::canonicalize_root(ws.path()).unwrap());
    }

    let result = open_file_from_path(&state, &note.to_string_lossy());
    assert!(
        result.is_ok(),
        "workspace-contained open must pass: {result:?}"
    );
}

#[test]
fn open_file_still_rejects_path_outside_open_workspace() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let ws = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let secret = outside.path().join("secret.txt");
    std::fs::write(&secret, "shhh").unwrap();

    {
        let mut root = state.workspace_root.lock().unwrap();
        *root = Some(writ_tauri_lib::security::canonicalize_root(ws.path()).unwrap());
    }

    let result = open_file_from_path(&state, &secret.to_string_lossy());
    assert!(result.is_err(), "outside-workspace open must stay rejected");
}

#[test]
fn open_file_rejects_path_that_was_never_authorized() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let secret = dir.path().join("secret.txt");
    std::fs::write(&secret, "shhh").unwrap();
    let secret_str = secret.to_str().unwrap();

    let result = open_file_from_path(&state, secret_str);
    assert!(result.is_err(), "expected gate to reject unauthorized open");
    assert!(
        result.as_ref().unwrap_err().contains("not authorized"),
        "unexpected error: {:?}",
        result
    );

    let store = state.store.lock().unwrap();
    let active = store
        .list_by_status(writ_core::buffer::document::BufferStatus::Active)
        .unwrap();
    assert!(
        active.is_empty(),
        "no buffer should have been created for an unauthorized path"
    );
}

#[test]
fn open_file_accepts_explicitly_authorized_path() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let file = dir.path().join("notes.md");
    std::fs::write(&file, "# hi").unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();
    state.authorized_paths.record_for_open(canonical.clone());

    let result = open_file_from_path(&state, &canonical).expect("authorized open should succeed");
    assert_eq!(
        result
            .doc
            .as_ref()
            .expect("the file opened")
            .source_path
            .as_deref(),
        Some(canonical.as_str())
    );
}

#[test]
fn open_file_authorization_is_single_use() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let file = dir.path().join("once.txt");
    std::fs::write(&file, "first content").unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();

    state.authorized_paths.record_for_open(canonical.clone());
    let first = open_file_from_path(&state, &canonical);
    assert!(first.is_ok(), "first open should succeed: {:?}", first);

    let second = open_file_from_path(&state, &canonical);
    assert!(
        second.is_err(),
        "second open should require fresh authorization"
    );
    assert!(second.unwrap_err().contains("not authorized"));
}

#[test]
fn open_file_blesses_source_path_for_subsequent_saves() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let file = dir.path().join("save.md");
    std::fs::write(&file, "alpha").unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();
    state.authorized_paths.record_for_open(canonical.clone());

    let result = open_file_from_path(&state, &canonical).expect("open");

    save_buffer_content_inner(
        &state,
        &result.doc.as_ref().expect("the file opened").id,
        "beta",
    )
    .expect("save should succeed for blessed source");

    let on_disk = std::fs::read_to_string(&file).unwrap();
    assert_eq!(on_disk, "beta");
}

#[test]
fn save_to_source_rejects_unblessed_source_path() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let file = dir.path().join("planted.md");
    std::fs::write(&file, "original").unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();

    let store = state.store.lock().unwrap();
    let mut mgr = writ_core::buffer::manager::BufferManager::new();
    let doc = mgr.open_external(canonical.clone()).expect("mint");
    store.open_from_path(&doc, "original").expect("persist");
    drop(store);

    let result = save_buffer_content_inner(&state, &doc.id, "hijacked");
    assert!(
        result.is_err(),
        "unblessed source path must not be writable"
    );
    assert!(result.unwrap_err().contains("not authorized"));

    let on_disk = std::fs::read_to_string(&file).unwrap();
    assert_eq!(on_disk, "original", "file must not have been overwritten");
}

#[test]
fn open_file_for_active_duplicate_blesses_without_consuming_again() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let file = dir.path().join("dup.md");
    std::fs::write(&file, "x").unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();

    state.authorized_paths.record_for_open(canonical.clone());
    let first = open_file_from_path(&state, &canonical).expect("first open");

    state.authorized_paths.record_for_open(canonical.clone());
    let second = open_file_from_path(&state, &canonical).expect("second open returns existing");
    assert_eq!(
        first.doc.as_ref().expect("the file opened").id,
        second.doc.as_ref().expect("the file opened").id
    );

    save_buffer_content_inner(
        &state,
        &second.doc.as_ref().expect("the file opened").id,
        "y",
    )
    .expect("save");
}

#[test]
fn autosave_writes_the_file_the_buffer_was_opened_from() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let file = dir.path().join("aliases.zsh");
    std::fs::write(&file, "alias a=b\n").unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();
    state.authorized_paths.record_for_open(canonical.clone());
    let opened = open_file_from_path(&state, &canonical).expect("open");

    // The regression: every save route ends here, and for a file-backed buffer
    // it used to write only Writ's private copy, so edits never reached the
    // file the user had opened.
    save_buffer_content_inner(
        &state,
        &opened.doc.as_ref().expect("the file opened").id,
        "alias a=c\n",
    )
    .expect("save");

    assert_eq!(std::fs::read_to_string(&file).unwrap(), "alias a=c\n");
    assert!(
        is_empty_dir(&dir.path().join("buffers")),
        "the file is the only copy of the text"
    );
}

/// Mints a new note the way `create_buffer` does, writing nothing to disk.
fn new_note(state: &AppState) -> writ_core::buffer::document::BufferDocument {
    let store = state.store.lock().unwrap();
    let mut mgr = writ_core::buffer::manager::BufferManager::new();
    let doc = mgr.create_buffer(None).expect("mint");
    store.insert(&doc).expect("persist");
    doc
}

fn is_empty_dir(dir: &std::path::Path) -> bool {
    std::fs::read_dir(dir)
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(true)
}

#[test]
fn first_save_of_a_new_note_creates_a_dated_file_in_the_notes_folder() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let doc = new_note(&state);

    save_buffer_content_inner(&state, &doc.id, "just notes").expect("save");

    let expected = state.notes_root().join(format!(
        "{}.md",
        writ_core::notes::date_stem(doc.created_at)
    ));
    assert_eq!(std::fs::read_to_string(&expected).unwrap(), "just notes");
    assert!(
        is_empty_dir(&dir.path().join("buffers")),
        "the note is a file in the notes folder and nowhere else"
    );

    let store = state.store.lock().unwrap();
    assert_eq!(
        store.get(&doc.id).unwrap().source_path.as_deref(),
        expected.to_str(),
        "the row points at the file from the first keystroke on"
    );
}

#[test]
fn the_dated_file_name_dedupes_when_todays_note_already_exists() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let day = writ_core::notes::date_stem(chrono::Utc::now());
    std::fs::write(state.notes_root().join(format!("{day}.md")), "yesterday's").unwrap();

    let doc = new_note(&state);
    save_buffer_content_inner(&state, &doc.id, "today's").expect("save");

    assert_eq!(
        std::fs::read_to_string(state.notes_root().join(format!("{day} 2.md"))).unwrap(),
        "today's"
    );
    assert_eq!(
        std::fs::read_to_string(state.notes_root().join(format!("{day}.md"))).unwrap(),
        "yesterday's",
        "the note already there is never written over"
    );
}

#[test]
fn a_new_note_with_nothing_in_it_writes_no_file() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);
    let doc = new_note(&state);

    save_buffer_content_inner(&state, &doc.id, "").expect("an empty save is a no-op");

    assert!(
        is_empty_dir(&state.notes_root()),
        "opening a tab and changing your mind leaves the folder as it was"
    );
    let store = state.store.lock().unwrap();
    assert!(store.get(&doc.id).unwrap().source_path.is_none());
}

#[test]
fn autosave_recreates_a_file_deleted_underneath_the_buffer() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let file = dir.path().join("gone.md");
    std::fs::write(&file, "here").unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();
    state.authorized_paths.record_for_open(canonical.clone());
    let opened = open_file_from_path(&state, &canonical).expect("open");

    std::fs::remove_file(&file).unwrap();

    // The gate re-resolves the path on every save; a deleted file has nothing
    // to resolve, and refusing there would turn autosave into a permanent
    // failure instead of writing the buffer back out.
    save_buffer_content_inner(
        &state,
        &opened.doc.as_ref().expect("the file opened").id,
        "back",
    )
    .expect("save");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "back");
}

#[cfg(unix)]
#[test]
fn autosave_keeps_the_files_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let file = dir.path().join("run.sh");
    std::fs::write(&file, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();
    state.authorized_paths.record_for_open(canonical.clone());
    let opened = open_file_from_path(&state, &canonical).expect("open");

    save_buffer_content_inner(
        &state,
        &opened.doc.as_ref().expect("the file opened").id,
        "#!/bin/sh\necho hi\n",
    )
    .expect("save");

    let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        mode, 0o755,
        "editing a script must not make it unexecutable"
    );
}

#[test]
fn reopening_a_file_edited_elsewhere_resyncs_the_buffer() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let file = dir.path().join("shared.md");
    std::fs::write(&file, "writ version").unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();
    state.authorized_paths.record_for_open(canonical.clone());
    let opened = open_file_from_path(&state, &canonical).expect("open");

    std::fs::write(&file, "someone else's version").unwrap();

    state.authorized_paths.record_for_open(canonical.clone());
    let reopened = open_file_from_path(&state, &canonical).expect("reopen");
    assert_eq!(
        reopened.doc.as_ref().expect("the file opened").id,
        opened.doc.as_ref().expect("the file opened").id,
        "same tab, not a second one"
    );

    let store = state.store.lock().unwrap();
    assert_eq!(
        store
            .read_content(&opened.doc.as_ref().expect("the file opened").id)
            .unwrap(),
        "someone else's version",
        "reopening shows the file, not the copy Writ loaded earlier"
    );
}

#[test]
fn reopening_an_untouched_file_leaves_the_buffer_alone() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let file = dir.path().join("steady.md");
    std::fs::write(&file, "unchanged").unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();
    state.authorized_paths.record_for_open(canonical.clone());
    let opened = open_file_from_path(&state, &canonical).expect("open");

    let before = std::fs::metadata(&file).unwrap().modified().unwrap();

    state.authorized_paths.record_for_open(canonical.clone());
    open_file_from_path(&state, &canonical).expect("reopen");

    assert_eq!(
        std::fs::metadata(&file).unwrap().modified().unwrap(),
        before,
        "reopening reads the file; it never writes it back"
    );
    let store = state.store.lock().unwrap();
    assert_eq!(
        store
            .read_content(&opened.doc.as_ref().expect("the file opened").id)
            .unwrap(),
        "unchanged"
    );
}

#[test]
fn a_tab_restored_after_a_restart_can_still_be_saved() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let file = dir.path().join("restored.md");
    std::fs::write(&file, "before the restart").unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();
    state.authorized_paths.record_for_open(canonical.clone());
    let opened = open_file_from_path(&state, &canonical).expect("open");

    // Blessing lives in memory, so a relaunch starts from the persisted rows.
    // If the rehydrated key did not match what a save presents, every autosave
    // after a restart would fail as an unauthorized write.
    let restarted = make_state(&dir);
    {
        let store = restarted.store.lock().unwrap();
        let hydrated =
            writ_tauri_lib::state::bless_persisted_sources(&store, &restarted.authorized_paths);
        assert_eq!(hydrated, 1);
    }

    save_buffer_content_inner(
        &restarted,
        &opened.doc.as_ref().expect("the file opened").id,
        "after the restart",
    )
    .expect("save");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "after the restart");
}

#[test]
fn a_restored_tab_whose_file_was_deleted_recreates_it() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let file = dir.path().join("deleted-while-closed.md");
    std::fs::write(&file, "content").unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();
    state.authorized_paths.record_for_open(canonical.clone());
    let opened = open_file_from_path(&state, &canonical).expect("open");

    std::fs::remove_file(&file).unwrap();

    let restarted = make_state(&dir);
    {
        let store = restarted.store.lock().unwrap();
        writ_tauri_lib::state::bless_persisted_sources(&store, &restarted.authorized_paths);
    }

    save_buffer_content_inner(
        &restarted,
        &opened.doc.as_ref().expect("the file opened").id,
        "rewritten",
    )
    .expect("save");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "rewritten");
}

#[test]
fn file_created_in_the_notes_folder_by_another_program_opens() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    // Nothing recorded this path: it arrived the way a sync client delivers a
    // note written on another machine.
    let note = state.notes_root().join("from-another-machine.md");
    std::fs::write(&note, "typed elsewhere").unwrap();

    let opened = open_file_from_path(&state, &note.to_string_lossy())
        .expect("a file in the notes folder opens without a dialog");
    assert_eq!(
        opened
            .doc
            .as_ref()
            .expect("the file opened")
            .source_path
            .as_deref(),
        Some(canonicalize_for_authorization(&note).unwrap().as_str())
    );
}

#[test]
fn save_into_the_notes_folder_is_authorized_without_a_dialog() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let note = state.notes_root().join("synced.md");
    std::fs::write(&note, "arrived from a sync client").unwrap();
    let canonical = canonicalize_for_authorization(&note).unwrap();

    // The row exists with no blessing, which is what a restart plus a note
    // that was never opened through a dialog leaves behind.
    let doc = {
        let store = state.store.lock().unwrap();
        let mut mgr = writ_core::buffer::manager::BufferManager::new();
        let doc = mgr.open_external(canonical.clone()).expect("mint");
        store
            .open_from_path(&doc, "arrived from a sync client")
            .expect("persist");
        doc
    };
    assert!(!state.authorized_paths.is_blessed_source(&canonical));

    save_buffer_content_inner(&state, &doc.id, "edited in Writ").expect("save");
    assert_eq!(std::fs::read_to_string(&note).unwrap(), "edited in Writ");
}

#[test]
fn a_path_that_climbs_out_of_the_notes_folder_is_still_refused() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let outside = dir.path().join("outside.md");
    std::fs::write(&outside, "original").unwrap();

    let climbing = state
        .notes_root()
        .join("..")
        .join("outside.md")
        .to_string_lossy()
        .into_owned();

    let doc = {
        let store = state.store.lock().unwrap();
        let mut mgr = writ_core::buffer::manager::BufferManager::new();
        let doc = mgr.open_external(climbing).expect("mint");
        store.open_from_path(&doc, "original").expect("persist");
        doc
    };

    let result = save_buffer_content_inner(&state, &doc.id, "hijacked");
    assert!(result.is_err(), "containment must not accept a traversal");
    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "original");

    // A traversal to a file that does not exist yet cannot be resolved, so the
    // comparison falls back to the path as written; refusing `..` outright is
    // what stops it creating a file outside the folder.
    let unwritten = dir.path().join("planted-by-traversal.md");
    let climbing_new = state
        .notes_root()
        .join("..")
        .join("planted-by-traversal.md")
        .to_string_lossy()
        .into_owned();
    let new_doc = {
        let store = state.store.lock().unwrap();
        let mut mgr = writ_core::buffer::manager::BufferManager::new();
        let doc = mgr.open_external(climbing_new).expect("mint");
        store.open_from_path(&doc, "").expect("persist");
        doc
    };

    assert!(save_buffer_content_inner(&state, &new_doc.id, "planted").is_err());
    assert!(
        !unwritten.exists(),
        "nothing may be created outside the folder"
    );
}

#[cfg(unix)]
#[test]
fn a_linked_folder_inside_the_notes_folder_cannot_carry_a_save_outside() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let outside = TempDir::new().unwrap();
    let link = state.notes_root().join("linked");
    std::os::unix::fs::symlink(outside.path(), &link).unwrap();

    // The leaf does not exist, so only resolving the folders above it shows
    // that the write would land outside the notes folder.
    let planted = link.join("planted.md");
    let doc = {
        let store = state.store.lock().unwrap();
        let mut mgr = writ_core::buffer::manager::BufferManager::new();
        let doc = mgr
            .open_external(planted.to_string_lossy().into_owned())
            .expect("mint");
        store.open_from_path(&doc, "").expect("persist");
        doc
    };

    assert!(writ_tauri_lib::commands::file::authorize_source_write(
        &state,
        &planted.to_string_lossy()
    )
    .is_err());
    assert!(save_buffer_content_inner(&state, &doc.id, "planted").is_err());
    assert!(
        !outside.path().join("planted.md").exists(),
        "a linked folder must not carry the write out of the notes folder"
    );
}

#[test]
fn a_new_note_in_a_real_subfolder_of_the_notes_folder_is_authorized() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let subfolder = state.notes_root().join("projects");
    std::fs::create_dir(&subfolder).unwrap();
    let minted = subfolder.join("not-written-yet.md");

    writ_tauri_lib::commands::file::authorize_source_write(&state, &minted.to_string_lossy())
        .expect("a note about to be minted inside the folder is writable");
}

#[test]
fn is_within_notes_refuses_a_path_that_climbs_out() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let inside = state.notes_root().join("note.md");
    assert!(state.is_within_notes(&inside.to_string_lossy()));

    let climbing = state.notes_root().join("..").join("note.md");
    assert!(!state.is_within_notes(&climbing.to_string_lossy()));
}

fn count_external_events(state: &AppState) -> Arc<Mutex<u32>> {
    let count = Arc::new(Mutex::new(0u32));
    let count_clone = count.clone();
    state.event_bus.subscribe(move |event| {
        if let WritEvent::BufferExternal { .. } = event {
            *count_clone.lock().unwrap() += 1;
        }
    });
    count
}

#[test]
fn reopening_an_unchanged_file_emits_nothing() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let file = dir.path().join("quiet.md");
    std::fs::write(&file, "steady text").unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();
    state.authorized_paths.record_for_open(canonical.clone());
    open_file_from_path(&state, &canonical).expect("open");

    let count = count_external_events(&state);

    state.authorized_paths.record_for_open(canonical.clone());
    open_file_from_path(&state, &canonical).expect("reopen");

    assert_eq!(
        *count.lock().unwrap(),
        0,
        "an unchanged reopen must not emit an external-change event"
    );
}

#[test]
fn a_file_changed_out_of_band_keeps_being_announced_until_it_is_read() {
    // The event is an offer the editor can decline: it asks before discarding
    // unsaved keystrokes. Until the file is actually read, the tab still shows
    // the old text, so every reopen has the same news to deliver.
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let file = dir.path().join("changing.md");
    std::fs::write(&file, "first").unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();
    state.authorized_paths.record_for_open(canonical.clone());
    let opened = open_file_from_path(&state, &canonical).expect("open");

    let count = count_external_events(&state);

    std::fs::write(&file, "second").unwrap();
    state.authorized_paths.record_for_open(canonical.clone());
    open_file_from_path(&state, &canonical).expect("reopen after external change");
    assert_eq!(
        *count.lock().unwrap(),
        1,
        "a reopen with a changed digest must emit"
    );

    state.authorized_paths.record_for_open(canonical.clone());
    open_file_from_path(&state, &canonical).expect("reopen after the offer was declined");
    assert_eq!(
        *count.lock().unwrap(),
        2,
        "a reload nobody took must not leave the tab looking current"
    );

    read_buffer_content_inner(&state, &opened.doc.as_ref().expect("the file opened").id)
        .expect("the editor takes the reload");

    state.authorized_paths.record_for_open(canonical.clone());
    open_file_from_path(&state, &canonical).expect("reopen with the file already read");
    assert_eq!(
        *count.lock().unwrap(),
        2,
        "once the file has been read there is nothing left to announce"
    );
}

#[test]
fn reopening_a_tab_whose_file_writ_never_read_announces_nothing() {
    // The tab restored at launch and never brought to the front. Its editor is
    // not mounted, so nothing has read the file and no digest was recorded. An
    // empty record is what a fresh process starts with, which
    // `forget_disk_state` reproduces here.
    //
    // Reopening it — `writ <path>`, an OS document open, a drop on the window —
    // used to compare against a record that was not there, read the miss as a
    // change, and announce one. The frontend fails closed on a note it holds no
    // record of, so that announcement arrives as a prompt asking whether to
    // discard work, over a document nobody typed into and a file nobody
    // touched.
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let file = dir.path().join("background.md");
    std::fs::write(&file, "as it was").unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();
    state.authorized_paths.record_for_open(canonical.clone());
    let opened = open_file_from_path(&state, &canonical).expect("open");
    state.forget_disk_state(&opened.doc.as_ref().expect("the file opened").id);

    let count = count_external_events(&state);

    state.authorized_paths.record_for_open(canonical.clone());
    open_file_from_path(&state, &canonical).expect("reopen");
    assert_eq!(
        *count.lock().unwrap(),
        0,
        "a reopen with nothing to compare against must not claim a change"
    );

    // Even where the file did move on, Writ has no basis to say so and no need
    // to: the editor reads the file itself the moment the tab is mounted.
    std::fs::write(&file, "moved on while nobody was looking").unwrap();
    state.authorized_paths.record_for_open(canonical.clone());
    open_file_from_path(&state, &canonical).expect("reopen after an out-of-band write");
    assert_eq!(
        *count.lock().unwrap(),
        0,
        "an unread file's contents are not something Writ can report a change to"
    );

    // The record arrives with the read, and from there the announcement works.
    read_buffer_content_inner(&state, &opened.doc.as_ref().expect("the file opened").id)
        .expect("the editor mounts and reads");
    std::fs::write(&file, "and again").unwrap();
    state.authorized_paths.record_for_open(canonical.clone());
    open_file_from_path(&state, &canonical).expect("reopen after the read");
    assert_eq!(
        *count.lock().unwrap(),
        1,
        "a change against a recorded digest is still announced"
    );
}

#[test]
fn closing_a_tab_forgets_what_its_file_held() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let file = dir.path().join("closing.md");
    std::fs::write(&file, "text").unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();
    state.authorized_paths.record_for_open(canonical.clone());
    let opened = open_file_from_path(&state, &canonical).expect("open");
    assert!(state
        .disk_state(&opened.doc.as_ref().expect("the file opened").id)
        .is_some());

    close_buffer_inner(&state, &opened.doc.as_ref().expect("the file opened").id).expect("close");

    assert!(
        state
            .disk_state(&opened.doc.as_ref().expect("the file opened").id)
            .is_none(),
        "a closed tab is not a file Writ is still watching the bytes of"
    );
}

#[test]
fn closing_several_tabs_forgets_every_one_of_them() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let mut ids = Vec::new();
    for name in ["one.md", "two.md"] {
        let file = dir.path().join(name);
        std::fs::write(&file, "text").unwrap();
        let canonical = canonicalize_for_authorization(&file).unwrap();
        state.authorized_paths.record_for_open(canonical.clone());
        ids.push(
            open_file_from_path(&state, &canonical)
                .expect("open")
                .doc
                .expect("the file opened")
                .id,
        );
    }

    close_buffers_inner(&state, &ids).expect("close both");

    for id in &ids {
        assert!(state.disk_state(id).is_none(), "{id} was left recorded");
    }
}

#[test]
fn deleting_a_note_forgets_what_its_file_held() {
    let dir = TempDir::new().unwrap();
    let state = make_state(&dir);

    let file = dir.path().join("doomed.md");
    std::fs::write(&file, "text").unwrap();
    let canonical = canonicalize_for_authorization(&file).unwrap();
    state.authorized_paths.record_for_open(canonical.clone());
    let opened = open_file_from_path(&state, &canonical).expect("open");
    assert!(state
        .disk_state(&opened.doc.as_ref().expect("the file opened").id)
        .is_some());

    delete_buffer_inner(&state, &opened.doc.as_ref().expect("the file opened").id)
        .expect("delete the row");

    assert!(state
        .disk_state(&opened.doc.as_ref().expect("the file opened").id)
        .is_none());
    assert!(file.exists(), "deleting the row never deletes the file");
}
