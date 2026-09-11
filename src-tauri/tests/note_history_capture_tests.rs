//! What reaches the version store from outside a write.
//!
//! A reload is not a write, so it never reaches the facade's hook, and
//! neither does a conflict resolution or a file another program overwrote
//! while Writ had it open. Those come through the external-change path in
//! `commands::buffer`, and between that and the hook there is no third place
//! a version is captured.
//!
//! The Obsidian gap these close is spec 481 and 482: file recovery there does
//! not capture a file changed outside the app, and does not keep a deleted
//! file's last text.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex, RwLock};

use tempfile::TempDir;
use writ_core::config::WritConfig;
use writ_core::events::bus::EventBus;
use writ_core::notes::line_ending::LineEnding;
use writ_core::notes::reload::ChangeChoice;
use writ_core::preview::ContentRendererRegistry;
use writ_core::update::UpdatePhase;
use writ_core::watcher::reconcile::ReconcileGate;
use writ_plugin::transform::TransformRegistry;
use writ_storage::buffer_store::BufferStore;
use writ_storage::config_store::ConfigStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::layout_state::LayoutStateStore;
use writ_storage::note_history::NoteHistoryStore;
use writ_storage::notes_index::NotesIndexStore;
use writ_tauri_lib::commands::buffer::{
    read_buffer_content_inner, resolve_external_change_at, save_buffer_content_inner,
};
use writ_tauri_lib::commands::file::open_file_from_path;
use writ_tauri_lib::commands::note_history::{
    restore_note_version_for_tab, restore_note_version_inner,
};
use writ_tauri_lib::preview::handler::RenderCache;
use writ_tauri_lib::quit::QuitState;
use writ_tauri_lib::security::{canonicalize_for_authorization, AuthorizedPaths};
use writ_tauri_lib::state::AppState;
use writ_tauri_lib::watcher::handler::create_ignore_set;

/// The app as it runs: a notes folder, a version store wired to it, and a
/// buffer store that hands its writes to that store.
struct App {
    _dir: TempDir,
    notes: PathBuf,
    state: AppState,
}

impl App {
    fn new() -> Self {
        let dir = TempDir::new().expect("temp dir");
        let writ_dir = dir.path().join("data");
        let buffers_dir = writ_dir.join("buffers");
        std::fs::create_dir_all(&buffers_dir).expect("buffers dir");

        let notes_root = writ_dir.join("Writ");
        std::fs::create_dir_all(&notes_root).expect("notes folder");
        let notes_root =
            writ_tauri_lib::security::canonicalize_root(&notes_root).expect("canonical");

        let db_path = writ_dir.join("writ.db");
        let conn = open_database(&db_path).expect("open db");
        run_migrations(&conn).expect("migrations");

        let note_history = Arc::new(NoteHistoryStore::open(&writ_dir).expect("version store"));
        note_history.set_notes_root(notes_root.clone());
        note_history.set_probe(Arc::new(
            writ_tauri_lib::watcher::identity::PlatformIdentity,
        ));

        let mut store = BufferStore::new(conn, buffers_dir.clone());
        store.set_notes_root(notes_root.clone());
        store.set_history(Arc::clone(&note_history));

        let state = AppState {
            store: Mutex::new(store),
            config_store: ConfigStore::new(writ_dir.join("config.toml")),
            config: Mutex::new(WritConfig::default()),
            writ_dir: writ_dir.clone(),
            buffers_dir,
            notes_root: RwLock::new(notes_root.clone()),
            first_run: false,
            retitle_watch: Arc::new(writ_tauri_lib::first_run::RetitleWatch::new()),
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
            search_generation: Arc::new(AtomicU64::new(0)),
            last_disk_hash: Mutex::new(HashMap::new()),
            source_records: Mutex::new(HashMap::new()),
            unsaved_on_exit: Mutex::new(HashMap::new()),
        };
        Self {
            _dir: dir,
            notes: notes_root,
            state,
        }
    }

    /// Opens a note in the notes folder the way the frontend reaches one.
    fn open(&self, name: &str, text: &str) -> (String, PathBuf) {
        let path = self.notes.join(name);
        std::fs::write(&path, text).expect("write");
        let canonical = canonicalize_for_authorization(&path).expect("canonical");
        self.state
            .authorized_paths
            .record_for_open(canonical.clone());
        let opened = open_file_from_path(&self.state, &canonical).expect("open");
        let id = opened.doc.expect("the file opened").id;
        // The editor loads a tab's text through `read_buffer_content` right
        // after the open (`buffer-registry.ts` `readContent`), which is the
        // read this seam hangs on.
        read_buffer_content_inner(&self.state, &id).expect("read");
        (id, PathBuf::from(canonical))
    }

    /// The entry holding `text`, which is what a restore is asked for by id.
    fn version_holding(&self, path: &Path, text: &[u8]) -> i64 {
        let note = self
            .state
            .note_history
            .key_for(path)
            .expect("a note inside the folder");
        self.state
            .note_history
            .versions(&note)
            .expect("versions")
            .into_iter()
            .find(|entry| self.state.note_history.content(entry.id).expect("text") == text.to_vec())
            .map(|entry| entry.id)
            .expect("an entry holding that text")
    }

    /// Every text the store holds for the note at `path`, newest first.
    fn versions_of(&self, path: &Path) -> Vec<Vec<u8>> {
        let note = self
            .state
            .note_history
            .key_for(path)
            .expect("a note inside the folder");
        self.state
            .note_history
            .versions(&note)
            .expect("versions")
            .into_iter()
            .map(|entry| {
                self.state
                    .note_history
                    .content(entry.id)
                    .expect("the text of an entry")
            })
            .collect()
    }
}

#[test]
fn opening_a_note_keeps_what_the_file_holds() {
    let app = App::new();
    let (_id, path) = app.open("Launch.md", "what the file holds\n");

    assert_eq!(
        app.versions_of(&path),
        vec![b"what the file holds\n".to_vec()]
    );
}

#[test]
fn an_external_overwrite_puts_the_text_it_replaced_in_the_store_before_the_reload_lands() {
    let app = App::new();
    let (id, path) = app.open("Launch.md", "what the tab was given\n");

    // Somebody else writes the file while Writ has it open.
    std::fs::write(&path, "what somebody else wrote\n").expect("overwrite");

    assert_eq!(
        app.versions_of(&path),
        vec![b"what the tab was given\n".to_vec()],
        "the text about to be overwritten is already kept, before anything reloads"
    );

    let reloaded = read_buffer_content_inner(&app.state, &id).expect("reload");

    assert_eq!(reloaded, b"what somebody else wrote\n");
    assert_eq!(
        app.versions_of(&path),
        vec![
            b"what somebody else wrote\n".to_vec(),
            b"what the tab was given\n".to_vec(),
        ],
        "and the text the reload landed is kept beside it"
    );
}

#[test]
fn a_tab_that_leaves_the_question_standing_still_keeps_the_text_on_disk() {
    let app = App::new();
    let (id, path) = app.open("Launch.md", "the first\n");
    std::fs::write(&path, "the second\n").expect("overwrite");

    // The tab is told its file changed and asks what it holds. It does not
    // reload: the document is dirty, so the question stands.
    let state = &app.state;
    let answer =
        writ_tauri_lib::commands::buffer::note_disk_state_inner(state, &id).expect("state");
    assert!(matches!(
        answer,
        writ_tauri_lib::commands::buffer::NoteDiskAnswer::Described { .. }
    ));

    // A second change over the top of the first does not take the first with
    // it.
    std::fs::write(&path, "the third\n").expect("overwrite");
    writ_tauri_lib::commands::buffer::note_disk_state_inner(state, &id).expect("state");

    assert_eq!(
        app.versions_of(&path),
        vec![
            b"the third\n".to_vec(),
            b"the second\n".to_vec(),
            b"the first\n".to_vec(),
        ]
    );
}

#[test]
fn a_conflict_resolution_puts_both_sides_in_the_store() {
    let app = App::new();
    let (id, path) = app.open("Launch.md", "what the tab was given\n");
    std::fs::write(&path, "what somebody else wrote\n").expect("overwrite");

    let outcome = resolve_external_change_at(
        &app.state,
        &id,
        &path,
        None,
        LineEnding::Lf,
        ChangeChoice::KeepMine,
        "what the person typed\n",
    )
    .expect("resolve");
    assert!(outcome.conflict_copy_path.is_some());

    let kept = app.versions_of(&path);
    assert!(
        kept.contains(&b"what somebody else wrote\n".to_vec()),
        "the file's side is kept: {kept:?}"
    );
    assert!(
        kept.contains(&b"what the person typed\n".to_vec()),
        "and so is the tab's: {kept:?}"
    );
}

#[test]
fn a_note_deleted_externally_keeps_its_last_known_text() {
    let app = App::new();
    let (id, path) = app.open("Launch.md", "the first\n");
    save_buffer_content_inner(&app.state, &id, "the last thing it said\n").expect("save");

    std::fs::remove_file(&path).expect("delete");

    let kept = app.versions_of(&path);
    assert_eq!(
        kept.first().map(Vec::as_slice),
        Some(b"the last thing it said\n".as_slice()),
        "a file that is gone is still a note with a history: {kept:?}"
    );
}

#[test]
fn a_note_deleted_externally_and_never_saved_here_is_restorable_through_revert_to() {
    let app = App::new();
    // Opened, read into the editor, and never saved in this session: the
    // open read is the only entry the store will ever hold for it.
    let (id, path) = app.open("Launch.md", "written by another program\n");

    std::fs::remove_file(&path).expect("delete");
    assert!(!path.exists(), "the file is gone before the restore");

    let kept = app.version_holding(&path, b"written by another program\n");
    restore_note_version_inner(
        &app.state.notes_root(),
        &app.state.writ_dir,
        &app.state.note_history,
        kept,
        app.state.disk_state(&id),
    )
    .expect("a note that is gone is still a note a version can bring back");

    assert_eq!(
        std::fs::read(&path).expect("the restore put the file back"),
        b"written by another program\n",
        "the restore writes the last text the file held, byte for byte"
    );
    assert_eq!(
        app.versions_of(&path).first().map(Vec::as_slice),
        Some(b"written by another program\n".as_slice()),
        "the restore itself is recorded, so it can be undone by restoring again"
    );
}

#[test]
fn a_save_keeps_the_text_it_wrote() {
    let app = App::new();
    let (id, path) = app.open("Launch.md", "the first\n");
    save_buffer_content_inner(&app.state, &id, "the second\n").expect("save");

    assert_eq!(
        app.versions_of(&path),
        vec![b"the second\n".to_vec(), b"the first\n".to_vec()]
    );
}

#[test]
fn a_restore_moments_after_a_save_keeps_the_text_that_save_wrote() {
    let app = App::new();
    let (id, path) = app.open("Launch.md", "the first\n");
    save_buffer_content_inner(&app.state, &id, "the second\n").expect("save");

    // No wait: the restore lands inside the ten seconds a run of saves
    // collapses into one version.
    let first = app.version_holding(&path, b"the first\n");
    restore_note_version_inner(
        &app.state.notes_root(),
        &app.state.writ_dir,
        &app.state.note_history,
        first,
        app.state.disk_state(&id),
    )
    .expect("restore");

    assert_eq!(std::fs::read(&path).expect("read"), b"the first\n");
    let kept = app.versions_of(&path);
    assert!(
        kept.contains(&b"the second\n".to_vec()),
        "the text the restore replaced is still there: {kept:?}"
    );
}

#[test]
fn restoring_back_moments_later_returns_the_note_to_what_the_save_wrote() {
    let app = App::new();
    let (id, path) = app.open("Launch.md", "the first\n");
    save_buffer_content_inner(&app.state, &id, "the second\n").expect("save");

    // Through the seam the command uses, so the first restore leaves the tab
    // holding a record of what it wrote.
    let first = app.version_holding(&path, b"the first\n");
    restore_note_version_for_tab(&app.state, first).expect("restore");

    // And the second is handed that record, which is what the command hands
    // it for a note somebody has open.
    let second = app.version_holding(&path, b"the second\n");
    restore_note_version_inner(
        &app.state.notes_root(),
        &app.state.writ_dir,
        &app.state.note_history,
        second,
        app.state.disk_state(&id),
    )
    .expect("restore back");

    assert_eq!(std::fs::read(&path).expect("read"), b"the second\n");
}

#[test]
fn the_announcement_a_restore_raises_does_not_keep_the_restored_text_twice() {
    let app = App::new();
    let (id, path) = app.open("Launch.md", "the first\n");
    save_buffer_content_inner(&app.state, &id, "the second\n").expect("save");

    // The version holding the text the note started with.
    let note = app
        .state
        .note_history
        .key_for(&path)
        .expect("a note inside the folder");
    let first = app
        .state
        .note_history
        .versions(&note)
        .expect("versions")
        .into_iter()
        .find(|entry| {
            app.state.note_history.content(entry.id).expect("text") == b"the first\n".to_vec()
        })
        .expect("the text the note started with");

    let last_known = app.state.disk_state(&id);
    restore_note_version_inner(
        &app.state.notes_root(),
        &app.state.writ_dir,
        &app.state.note_history,
        first.id,
        last_known,
    )
    .expect("restore");

    let after_restore = app.versions_of(&path);
    assert_eq!(
        after_restore.first().map(Vec::as_slice),
        Some(b"the first\n".as_slice())
    );

    // A restore is written without an ignore stamp, so the folder watcher
    // announces it and the tab asks what its file holds. That read is seam
    // two, and the text it finds is the one the restore already kept.
    writ_tauri_lib::commands::buffer::note_disk_state_inner(&app.state, &id).expect("state");

    assert_eq!(
        app.versions_of(&path),
        after_restore,
        "the announcement a restore raises keeps nothing new"
    );
}

#[test]
fn a_file_the_notes_folder_does_not_hold_is_not_versioned() {
    let app = App::new();
    let outside = app._dir.path().join("elsewhere.md");
    std::fs::write(&outside, "not a note\n").expect("write");
    let canonical = canonicalize_for_authorization(&outside).expect("canonical");
    app.state
        .authorized_paths
        .record_for_open(canonical.clone());
    let opened = open_file_from_path(&app.state, &canonical).expect("open");
    let id = opened.doc.expect("the file opened").id;

    save_buffer_content_inner(&app.state, &id, "still not a note\n").expect("save");

    assert!(
        app.state
            .note_history
            .key_for(Path::new(&canonical))
            .is_none(),
        "a file somebody else owns is not copied into Writ's data folder"
    );
}
