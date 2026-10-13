//! Writ's own writes to the notes index are announced (ADR-036 section 4).
//!
//! The notes watcher drops every write Writ makes, so a save, a new file, a
//! rename, a delete, a restore and a copy reach the index through the command
//! that made them, and that command is what tells the frontend. Each test runs
//! the Tauri-free inner function against a real notes folder and a real index,
//! and listens on the event bus the bridge forwards from, so what is asserted
//! is the event the frontend receives and the rows a re-read would find.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};
use std::time::SystemTime;

use tempfile::TempDir;
use writ_core::config::WritConfig;
use writ_core::events::bus::{EventBus, WritEvent};
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
use writ_storage::notes_index::{self, NotesIndexStore, ReconcileOutcome};
use writ_tauri_lib::commands::buffer::{
    close_debounce_window, reindex_settled_buffer, resolve_external_change_at,
    resolve_external_change_inner, restore_note_file_inner, save_buffer_content_inner,
};
use writ_tauri_lib::commands::file::open_file_from_path;
use writ_tauri_lib::commands::first_run::{auto_retitle_note_inner, RetitleOutcome};
use writ_tauri_lib::commands::note_history::copy_note_version_and_index;
use writ_tauri_lib::commands::note_index::{announce_indexed, announce_walk, index_copy};
use writ_tauri_lib::commands::notes::{
    delete_note_inner, list_notes_dir_inner, move_notes_folder_to, new_note_inner,
    new_note_named_inner, rename_note_inner, rename_note_with_links_inner, save_note_copy_inner,
};
use writ_tauri_lib::events::bus_bridge::attach_bridge_behind_notes_index;
use writ_tauri_lib::events::WritFrontendEvent;
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

    let note_history = Arc::new(
        writ_storage::note_history::NoteHistoryStore::open(&writ_dir).expect("version store"),
    );

    let mut store = BufferStore::new(conn, buffers_dir.clone());
    store.set_notes_root(notes_root.clone());

    AppState {
        store: Mutex::new(store),
        config_store: ConfigStore::new(writ_dir.join("config.toml")),
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

/// One `notes:indexed` as the bus delivered it, with what a reader asking the
/// index at that moment would have found.
#[derive(Debug, Clone)]
struct Heard {
    paths: Vec<String>,
    /// Every path the index held when the event went out.
    indexed: Vec<String>,
    /// The headings of each announced path, read when the event went out.
    headings: Vec<Vec<String>>,
}

/// Listens for `NotesIndexed` on the state's bus, reading the index from
/// inside the handler: the frontend re-reads the moment it hears the event, so
/// the rows it finds are the rows the index holds then.
fn listen(state: &AppState) -> Arc<Mutex<Vec<Heard>>> {
    let heard = Arc::new(Mutex::new(Vec::new()));
    let sink = heard.clone();
    let index = state.notes_index.clone();
    state.event_bus.subscribe(move |event| {
        let WritEvent::NotesIndexed { paths } = event else {
            return;
        };
        let indexed = index.note_paths().unwrap_or_default();
        let headings = paths
            .iter()
            .map(|path| {
                index
                    .facts(path)
                    .map(|facts| facts.headings.into_iter().map(|h| h.text).collect())
                    .unwrap_or_default()
            })
            .collect();
        sink.lock().unwrap().push(Heard {
            paths: paths.clone(),
            indexed,
            headings,
        });
    });
    heard
}

fn heard(log: &Arc<Mutex<Vec<Heard>>>) -> Vec<Heard> {
    log.lock().unwrap().clone()
}

/// The index's spelling of a path, which is the spelling the event carries.
fn key(path: &str) -> String {
    notes_index::index_key(std::path::Path::new(path))
}

fn file_of(doc: &writ_core::buffer::document::BufferDocument) -> String {
    doc.source_path.clone().expect("the file has a path")
}

fn rows(state: &AppState) -> Vec<String> {
    state.notes_index.note_paths().expect("index rows")
}

#[test]
fn a_new_file_is_announced_once_the_index_holds_it() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let log = listen(&state);

    let doc = new_note_inner(&state).expect("new file");
    let path = key(&file_of(&doc));

    let heard = heard(&log);
    assert_eq!(
        heard.len(),
        1,
        "one new file is one announcement: {heard:?}"
    );
    assert_eq!(heard[0].paths, vec![path.clone()]);
    assert!(
        heard[0].indexed.contains(&path),
        "announced before the index held the file: {:?}",
        heard[0].indexed
    );
}

#[test]
fn a_rename_moves_the_row_and_is_announced_with_both_names() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let doc = new_note_named_inner(&state, "Draft.md").expect("file");
    let from = key(&file_of(&doc));
    let log = listen(&state);

    let renamed = rename_note_inner(&state, &doc.id, "Plan").expect("rename");
    let to = key(&file_of(&renamed));

    let held = rows(&state);
    assert!(!held.contains(&from), "the old name kept its row: {held:?}");
    assert!(held.contains(&to), "the new name has no row: {held:?}");
    let heard = heard(&log);
    assert_eq!(heard.len(), 1, "one rename is one announcement: {heard:?}");
    assert_eq!(heard[0].paths, vec![from, to]);
}

#[test]
fn a_file_named_by_its_first_line_leaves_no_row_under_its_old_name() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let doc = new_note_inner(&state).expect("new file");
    let from = key(&file_of(&doc));
    save_buffer_content_inner(&state, &doc.id, "Grocery list\n\nmilk\n").expect("save");
    let log = listen(&state);

    let RetitleOutcome::Renamed { note } =
        auto_retitle_note_inner(&state, &doc.id).expect("retitle")
    else {
        panic!("a file nothing else has touched takes its first line as its name");
    };
    let to = key(&file_of(&note));

    let held = rows(&state);
    assert!(
        !held.contains(&from),
        "the graph and the tag list would count the file twice: {held:?}"
    );
    assert!(held.contains(&to), "the new name has no row: {held:?}");
    assert_eq!(
        heard(&log)
            .iter()
            .map(|h| h.paths.clone())
            .collect::<Vec<_>>(),
        vec![vec![from, to]]
    );
}

#[test]
fn a_rename_that_rewrites_links_is_one_announcement_naming_every_file_it_wrote() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let root = state.notes_root();
    std::fs::write(root.join("A.md"), "See [[B]].\n").expect("write A");
    std::fs::write(root.join("B.md"), "# B\n").expect("write B");
    state
        .notes_index
        .reconcile(&root, &|| false, &|_| false)
        .expect("walk");
    let a = key(&root.join("A.md").to_string_lossy());
    let b = key(&root.join("B.md").to_string_lossy());
    let log = listen(&state);

    let outcome =
        rename_note_with_links_inner(&state, &root.join("B.md").to_string_lossy(), "C", true)
            .expect("rename");
    assert_eq!(outcome.updated, 1, "A links to B and is rewritten");
    let c = key(&outcome.renamed_path);

    let heard = heard(&log);
    assert_eq!(
        heard.len(),
        1,
        "a rename and the files it rewrote are one announcement: {heard:?}"
    );
    assert_eq!(heard[0].paths, vec![b.clone(), c.clone(), a]);
    assert!(heard[0].indexed.contains(&c));
    assert!(!heard[0].indexed.contains(&b));
}

#[test]
fn a_delete_forgets_the_row_before_it_is_announced() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let doc = new_note_named_inner(&state, "Gone.md").expect("file");
    let path = key(&file_of(&doc));
    let log = listen(&state);

    delete_note_inner(&state, &doc.id).expect("delete");

    let held = rows(&state);
    assert!(
        !held.contains(&path),
        "the deleted file kept its row until the watcher caught up: {held:?}"
    );
    let heard = heard(&log);
    assert_eq!(heard.len(), 1, "one delete is one announcement: {heard:?}");
    assert_eq!(heard[0].paths, vec![path.clone()]);
    assert!(!heard[0].indexed.contains(&path));
}

#[test]
fn a_settled_save_is_announced_once_its_reindex_lands() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let doc = new_note_named_inner(&state, "Plan.md").expect("file");
    let path = key(&file_of(&doc));
    save_buffer_content_inner(&state, &doc.id, "# Plan\n\n#plan\n").expect("save");
    let log = listen(&state);

    reindex_settled_buffer(&state, &doc.id);

    let heard = heard(&log);
    assert_eq!(
        heard.len(),
        1,
        "one settled save is one announcement: {heard:?}"
    );
    assert_eq!(heard[0].paths, vec![path]);
    assert_eq!(
        heard[0].headings,
        vec![vec!["Plan".to_string()]],
        "a re-read on the event finds the heading the save wrote"
    );
}

#[test]
fn an_autosave_is_announced_once_when_its_edits_settle() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let doc = new_note_named_inner(&state, "Plan.md").expect("file");
    let path = key(&file_of(&doc));
    let log = listen(&state);

    save_buffer_content_inner(&state, &doc.id, "# Plan\n").expect("first keystroke");
    let first = state
        .fts_scheduler
        .on_edit(&doc.id)
        .expect("the first save starts a worker");
    save_buffer_content_inner(&state, &doc.id, "# Plan\n\n## Later\n").expect("second keystroke");
    assert_eq!(
        state.fts_scheduler.on_edit(&doc.id),
        None,
        "the running worker takes the second save"
    );

    let again = close_debounce_window(&state, &doc.id, first)
        .expect("a newer save keeps the worker waiting");
    assert!(
        heard(&log).is_empty(),
        "nothing is announced while the edits are still arriving"
    );
    assert_eq!(close_debounce_window(&state, &doc.id, again), None);

    let heard = heard(&log);
    assert_eq!(
        heard.len(),
        1,
        "one burst of saves is one announcement: {heard:?}"
    );
    assert_eq!(heard[0].paths, vec![path]);
    assert_eq!(
        heard[0].headings,
        vec![vec!["Plan".to_string(), "Later".to_string()]],
        "a re-read on the event finds what the last save wrote"
    );
}

#[test]
fn a_save_outside_the_notes_folder_is_not_announced() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let elsewhere = dir.path().join("elsewhere.md");
    std::fs::write(&elsewhere, "# Elsewhere\n").expect("write");
    let canonical = canonicalize_for_authorization(&elsewhere).expect("canonical");
    state.authorized_paths.record_for_open(canonical.clone());
    let id = open_file_from_path(&state, &canonical)
        .expect("open")
        .doc
        .expect("a row")
        .id;
    save_buffer_content_inner(&state, &id, "# Changed\n").expect("save");
    let log = listen(&state);

    reindex_settled_buffer(&state, &id);

    assert!(
        heard(&log).is_empty(),
        "the index holds nothing outside the notes folder, so nothing moved"
    );
}

#[test]
fn a_restored_file_is_announced_once_its_reindex_lands() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let doc = new_note_named_inner(&state, "Back.md").expect("file");
    let file = file_of(&doc);
    save_buffer_content_inner(&state, &doc.id, "# Back\n").expect("save");
    reindex_settled_buffer(&state, &doc.id);
    std::fs::remove_file(&file).expect("deleted outside Writ");
    state
        .notes_index
        .forget_path(std::path::Path::new(&file))
        .expect("the watcher's forget");
    let log = listen(&state);

    restore_note_file_inner(&state, &doc.id, "# Back\n").expect("restore");
    reindex_settled_buffer(&state, &doc.id);

    let path = key(&file);
    let heard = heard(&log);
    assert_eq!(heard.len(), 1, "{heard:?}");
    assert_eq!(heard[0].paths, vec![path.clone()]);
    assert!(heard[0].indexed.contains(&path));
}

#[test]
fn a_copy_into_the_folder_is_indexed_and_announced() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let elsewhere = dir.path().join("report.md");
    std::fs::write(&elsewhere, "# Report\n").expect("write");
    let canonical = canonicalize_for_authorization(&elsewhere).expect("canonical");
    state.authorized_paths.record_for_open(canonical.clone());
    let id = open_file_from_path(&state, &canonical)
        .expect("open")
        .doc
        .expect("a row")
        .id;
    let log = listen(&state);

    let copy = save_note_copy_inner(&state, &id, "# Report\n").expect("copy");
    let path = key(&copy);

    let held = rows(&state);
    assert!(
        held.contains(&path),
        "the copy is a file in the folder the graph and search do not know: {held:?}"
    );
    let heard = heard(&log);
    assert_eq!(heard.len(), 1, "{heard:?}");
    assert_eq!(heard[0].paths, vec![path]);
}

#[test]
fn moving_the_folder_is_announced_with_the_folder_it_moved_to() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    new_note_named_inner(&state, "Kept.md").expect("file");
    // Outside the data folder: Writ refuses a notes folder inside its own.
    let elsewhere = TempDir::new().expect("destination");
    let destination = elsewhere.path().join("Moved");
    let log = listen(&state);

    let outcome = move_notes_folder_to(&state, &destination).expect("move");

    let root = key(&outcome.new_root);
    let heard = heard(&log);
    assert!(
        !heard.is_empty(),
        "every row was re-keyed and nothing said so"
    );
    assert_eq!(heard[0].paths, vec![root]);
}

/// Every `NotesIndexed` a bare bus delivers, as the paths it named.
fn announcements(bus: &EventBus) -> Arc<Mutex<Vec<Vec<String>>>> {
    let heard = Arc::new(Mutex::new(Vec::new()));
    let sink = heard.clone();
    bus.subscribe(move |event| {
        if let WritEvent::NotesIndexed { paths } = event {
            sink.lock().unwrap().push(paths.clone());
        }
    });
    heard
}

#[test]
fn one_announcement_names_each_file_once_in_the_order_given() {
    let bus = EventBus::new();
    let heard = announcements(&bus);

    announce_indexed(
        &bus,
        [
            "/notes/B.md".to_string(),
            "/notes/C.md".to_string(),
            "/notes/B.md".to_string(),
            String::new(),
        ],
    );
    announce_indexed(&bus, Vec::<String>::new());

    assert_eq!(
        *heard.lock().unwrap(),
        vec![vec!["/notes/B.md".to_string(), "/notes/C.md".to_string()]],
        "a call naming nothing raises nothing"
    );
}

#[test]
fn a_walk_that_changed_rows_names_the_folder_once_they_are_written() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let root = state.notes_root();
    std::fs::write(root.join("Plan.md"), "# Plan\n#plan\n").expect("write");
    std::fs::write(root.join("Recipes.md"), "# Recipes\n").expect("write");
    let log = listen(&state);

    let outcome = state
        .notes_index
        .reconcile(&root, &|| false, &|_| false)
        .expect("walk");
    announce_walk(&state.event_bus, &root, &outcome);

    let heard = heard(&log);
    assert_eq!(heard.len(), 1, "{heard:?}");
    assert_eq!(heard[0].paths, vec![key(&root.to_string_lossy())]);
    assert!(heard[0]
        .indexed
        .contains(&key(&root.join("Plan.md").to_string_lossy())));
}

#[test]
fn a_walk_that_found_the_index_right_says_nothing() {
    let bus = EventBus::new();
    let heard = announcements(&bus);

    announce_walk(
        &bus,
        std::path::Path::new("/notes"),
        &ReconcileOutcome {
            skipped_dataless: 3,
            ..ReconcileOutcome::default()
        },
    );

    assert!(
        heard.lock().unwrap().is_empty(),
        "a launch over an unchanged folder costs no re-read"
    );
}

#[test]
fn the_notes_folder_lists_its_folders_then_its_files() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let root = state.notes_root();
    std::fs::create_dir_all(root.join("Projects")).expect("folder");
    std::fs::write(root.join("Plan.md"), "plan").expect("write");
    std::fs::write(root.join("a.txt"), "a").expect("write");
    std::fs::write(root.join(".writ-tmp-a1b2c3"), "half written").expect("write");
    std::fs::create_dir_all(root.join(".git")).expect("git");

    let entries = list_notes_dir_inner(&state, &root.to_string_lossy()).expect("listing");

    let names: Vec<(&str, bool)> = entries
        .iter()
        .map(|entry| (entry.name.as_str(), entry.is_dir))
        .collect();
    assert_eq!(
        names,
        vec![("Projects", true), ("a.txt", false), ("Plan.md", false)]
    );
    assert_eq!(
        entries[2].path,
        key(&root.join("Plan.md").to_string_lossy()),
        "the tree and the index name a file the same way"
    );
}

#[test]
fn a_folder_inside_the_notes_folder_lists_what_it_holds() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let projects = state.notes_root().join("Projects");
    std::fs::create_dir_all(&projects).expect("folder");
    std::fs::write(projects.join("Launch.md"), "launch").expect("write");

    let entries = list_notes_dir_inner(&state, &projects.to_string_lossy()).expect("listing");

    assert_eq!(
        entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        vec!["Launch.md"]
    );
}

#[test]
fn a_folder_outside_the_notes_folder_is_refused() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let outside = dir.path().join("elsewhere");
    std::fs::create_dir_all(&outside).expect("folder");
    std::fs::write(outside.join("secret.md"), "not yours").expect("write");

    assert!(list_notes_dir_inner(&state, &outside.to_string_lossy()).is_err());
    assert!(list_notes_dir_inner(
        &state,
        &state
            .notes_root()
            .join("..")
            .join("elsewhere")
            .to_string_lossy()
    )
    .is_err());
}

#[cfg(unix)]
#[test]
fn a_link_leading_out_of_the_notes_folder_is_refused() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let outside = dir.path().join("elsewhere");
    std::fs::create_dir_all(&outside).expect("folder");
    std::fs::write(outside.join("secret.md"), "not yours").expect("write");
    let link = state.notes_root().join("Shortcut");
    std::os::unix::fs::symlink(&outside, &link).expect("symlink");

    assert!(list_notes_dir_inner(&state, &link.to_string_lossy()).is_err());
    let root_listing =
        list_notes_dir_inner(&state, &state.notes_root().to_string_lossy()).expect("root");
    assert!(
        root_listing.iter().all(|entry| entry.name != "Shortcut"),
        "a link out of the folder is not a folder of it"
    );
}

#[test]
fn a_copy_is_indexed_inside_the_notes_folder_and_nowhere_else() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let name = "Plan (conflict 2026-10-13 06.36.31).md";
    let inside = state.notes_root().join(name);
    std::fs::write(&inside, "# Plan\n").expect("write");
    let outside = dir.path().join(name);
    std::fs::write(&outside, "# Plan\n").expect("write");

    let path = key(&inside.to_string_lossy());
    assert_eq!(index_copy(&state, &inside), Some(path.clone()));
    assert_eq!(index_copy(&state, &outside), None);
    assert_eq!(rows(&state), vec![path]);
}

#[test]
fn a_version_copy_is_indexed_and_announced() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let root = state.notes_root();
    state.note_history.set_notes_root(root.clone());
    let projects = root.join("Projects");
    std::fs::create_dir_all(&projects).expect("folder");
    let file = projects.join("Plan.md");
    std::fs::write(&file, "# Plan\n").expect("write");
    let version_key = state
        .note_history
        .key_for(&file)
        .expect("a file inside the folder");
    state
        .note_history
        .capture_replaced(&version_key, b"# Older\n\n#older\n", SystemTime::now())
        .expect("keep a version");
    let version = state.note_history.versions(&version_key).expect("versions")[0].id;
    let log = listen(&state);

    let copy = copy_note_version_and_index(&state, version)
        .expect("copy")
        .path;

    let path = key(&copy.to_string_lossy());
    assert_eq!(
        copy.parent().map(|folder| key(&folder.to_string_lossy())),
        Some(key(&projects.to_string_lossy())),
        "the copy sits beside its file"
    );
    assert!(
        rows(&state).contains(&path),
        "the copy is a file in the folder search, the tags and the graph do not know"
    );
    let heard = heard(&log);
    assert_eq!(heard.len(), 1, "one copy is one announcement: {heard:?}");
    assert_eq!(heard[0].paths, vec![path]);
    assert_eq!(heard[0].headings, vec![vec!["Older".to_string()]]);
}

/// A file in the notes folder holding `mine` as Writ last wrote it, which
/// another program has since rewritten to `theirs`, with the index patched
/// the way the watcher patches it.
fn changed_under_writ(state: &AppState, mine: &str, theirs: &str) -> (String, String) {
    let doc = new_note_named_inner(state, "Plan.md").expect("file");
    let file = file_of(&doc);
    save_buffer_content_inner(state, &doc.id, mine).expect("save");
    reindex_settled_buffer(state, &doc.id);
    std::fs::write(&file, theirs).expect("another program's write");
    state
        .notes_index
        .index_path(Path::new(&file))
        .expect("the watcher's index patch");
    (doc.id, file)
}

fn headings_of(state: &AppState, path: &str) -> Vec<String> {
    state
        .notes_index
        .facts(path)
        .map(|facts| facts.headings.into_iter().map(|h| h.text).collect())
        .unwrap_or_default()
}

#[test]
fn keeping_mine_reindexes_the_file_and_announces_it_with_its_copy() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let (id, file) = changed_under_writ(&state, "# Mine\n", "# Theirs\n");
    let log = listen(&state);

    let outcome =
        resolve_external_change_inner(&state, &id, ChangeChoice::KeepMine, "# Mine\n\n## Kept\n")
            .expect("resolve");

    let note = key(&file);
    let copy = key(&outcome
        .conflict_copy_path
        .expect("the file's text set aside"));
    let heard = heard(&log);
    assert_eq!(heard.len(), 1, "one answer is one announcement: {heard:?}");
    assert_eq!(heard[0].paths, vec![note.clone(), copy.clone()]);
    assert_eq!(
        heard[0].headings,
        vec![
            vec!["Mine".to_string(), "Kept".to_string()],
            vec!["Theirs".to_string()],
        ],
        "a re-read on the event finds what Keep mine wrote, and the copy"
    );
    assert!(heard[0].indexed.contains(&copy));
}

#[test]
fn taking_the_file_announces_the_copy_of_what_was_typed() {
    for choice in [ChangeChoice::UseDisk, ChangeChoice::KeepBoth] {
        let dir = TempDir::new().expect("temp dir");
        let state = make_state(&dir);
        let (id, file) = changed_under_writ(&state, "# Mine\n", "# Theirs\n");
        let log = listen(&state);

        let outcome = resolve_external_change_inner(&state, &id, choice, "# Mine\n\n## Kept\n")
            .expect("resolve");

        let copy = key(&outcome
            .conflict_copy_path
            .expect("the typed text set aside"));
        let heard = heard(&log);
        assert_eq!(heard.len(), 1, "{choice:?}: {heard:?}");
        assert_eq!(
            heard[0].paths,
            vec![copy.clone()],
            "{choice:?}: the file is left as the other program wrote it"
        );
        assert_eq!(
            heard[0].headings,
            vec![vec!["Mine".to_string(), "Kept".to_string()]],
            "{choice:?}"
        );
        assert_eq!(headings_of(&state, &key(&file)), vec!["Theirs".to_string()]);
    }
}

#[test]
fn an_answer_over_the_same_text_announces_nothing() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let (id, _) = changed_under_writ(&state, "# Mine\n", "# Mine\n");
    let log = listen(&state);

    let outcome = resolve_external_change_inner(&state, &id, ChangeChoice::KeepMine, "# Mine\n")
        .expect("resolve");

    assert_eq!(outcome.conflict_copy_path, None);
    assert!(heard(&log).is_empty(), "nothing was written");
}

#[test]
fn an_answer_for_a_file_outside_the_notes_folder_indexes_and_announces_nothing() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let elsewhere = dir.path().join("elsewhere.md");
    std::fs::write(&elsewhere, "# Elsewhere\n").expect("write");
    let canonical = canonicalize_for_authorization(&elsewhere).expect("canonical");
    state.authorized_paths.record_for_open(canonical.clone());
    let id = open_file_from_path(&state, &canonical)
        .expect("open")
        .doc
        .expect("a row")
        .id;
    save_buffer_content_inner(&state, &id, "# Mine\n").expect("save");
    std::fs::write(&canonical, "# Theirs\n").expect("another program's write");
    let before = rows(&state);
    let log = listen(&state);

    let outcome =
        resolve_external_change_inner(&state, &id, ChangeChoice::KeepMine, "# Mine\n\n## Kept\n")
            .expect("resolve");

    assert!(outcome.conflict_copy_path.is_some(), "the copy is written");
    assert_eq!(
        rows(&state),
        before,
        "the index holds the notes folder alone"
    );
    assert!(heard(&log).is_empty(), "{:?}", heard(&log));
}

#[test]
fn a_save_the_guard_stopped_announces_the_copy_it_wrote() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let doc = new_note_named_inner(&state, "Plan.md").expect("file");
    let file = file_of(&doc);
    save_buffer_content_inner(&state, &doc.id, "# Mine\n").expect("save");
    std::fs::write(&file, "# Theirs\n").expect("another program's write");
    let log = listen(&state);

    let stopped = save_buffer_content_inner(&state, &doc.id, "# Mine\n\n## More\n");

    assert!(
        stopped.is_err(),
        "a save over a change Writ never read is stopped"
    );
    let copies: Vec<PathBuf> = std::fs::read_dir(state.notes_root())
        .expect("read the folder")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.to_string_lossy().contains("(conflict "))
        .collect();
    assert_eq!(copies.len(), 1, "{copies:?}");
    let copy = key(&copies[0].to_string_lossy());
    let heard = heard(&log);
    assert_eq!(heard.len(), 1, "{heard:?}");
    assert_eq!(heard[0].paths, vec![copy.clone()]);
    assert_eq!(
        heard[0].headings,
        vec![vec!["Mine".to_string(), "More".to_string()]]
    );
    assert!(heard[0].indexed.contains(&copy));
}

/// Keep mine whose own save is stopped, because another program wrote the
/// file between the bar's read and that save, writes two copies: the text the
/// bar read, and the typed text the stopped save set aside. The bar's read is
/// pointed at a second file holding what it would have read, so the save finds
/// the note holding something else, as it would after that write.
#[test]
fn keeping_mine_whose_save_is_stopped_is_still_one_announcement() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let (id, file) = changed_under_writ(&state, "# Mine\n", "# Theirs\n");
    let read_by_the_bar = state.notes_root().join("Seen.md");
    std::fs::write(&read_by_the_bar, "# Seen\n").expect("what the bar read");
    let log = listen(&state);

    let stopped = resolve_external_change_at(
        &state,
        &id,
        &read_by_the_bar,
        None,
        LineEnding::Lf,
        ChangeChoice::KeepMine,
        "# Mine\n\n## Kept\n",
    );

    assert!(stopped.is_err(), "the save is stopped: {stopped:?}");
    let copy_of = |stem: &str| {
        let copies: Vec<String> = std::fs::read_dir(state.notes_root())
            .expect("read the folder")
            .map(|entry| entry.expect("entry").path())
            .filter(|path| {
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                name.starts_with(stem) && name.contains("(conflict ")
            })
            .map(|path| key(&path.to_string_lossy()))
            .collect();
        assert_eq!(copies.len(), 1, "{stem}: {copies:?}");
        copies[0].clone()
    };
    let bar_copy = copy_of("Seen");
    let save_copy = copy_of("Plan");
    let heard = heard(&log);
    assert_eq!(heard.len(), 1, "one answer is one announcement: {heard:?}");
    assert_eq!(heard[0].paths, vec![bar_copy.clone(), save_copy.clone()]);
    assert_eq!(
        heard[0].headings,
        vec![
            vec!["Seen".to_string()],
            vec!["Mine".to_string(), "Kept".to_string()],
        ],
        "a re-read on the event finds both copies"
    );
    assert_eq!(
        std::fs::read_to_string(&file).expect("the note"),
        "# Theirs\n",
        "the stopped save left the note as the other program wrote it"
    );
}

/// A file another program changed is in the index by the time the frontend
/// hears of it: the frontend reads the index again on `notes:changed`, and
/// nothing after that read would say the rows moved.
#[test]
fn the_index_holds_another_programs_change_before_the_frontend_hears_of_it() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let file = state.notes_root().join("Plan.md");
    let path = file.to_string_lossy().into_owned();
    let index = state.notes_index.clone();
    let heard = Arc::new(Mutex::new(Vec::new()));
    let sink = heard.clone();
    attach_bridge_behind_notes_index(&state.event_bus, state.notes_index.clone(), move |event| {
        if let WritFrontendEvent::NotesChanged { path, removed } = event {
            let held = index.note_paths().unwrap_or_default().contains(&key(&path));
            sink.lock().unwrap().push((removed, held));
        }
    });

    std::fs::write(&file, "# Plan\n").expect("another program's write");
    state.event_bus.emit(WritEvent::NotesChanged {
        path: path.clone(),
        removed: false,
    });
    std::fs::remove_file(&file).expect("another program's delete");
    state.event_bus.emit(WritEvent::NotesChanged {
        path,
        removed: true,
    });

    assert_eq!(
        *heard.lock().unwrap(),
        vec![(false, true), (true, false)],
        "(removed, held when the frontend heard): the index takes each change first"
    );
}

/// The tree and the index must hide the same folders: a tag naming a file the
/// tree cannot show filters the tree down to "No files with this tag".
#[test]
fn the_tree_hides_the_folders_the_index_skips_and_no_others() {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    let root = state.notes_root();
    for (folder, name) in [
        ("", "Plan.md"),
        ("Projects", "Launch.md"),
        ("build", "Plan.md"),
        (".obsidian", "workspace.md"),
    ] {
        let folder = root.join(folder);
        std::fs::create_dir_all(&folder).expect("folder");
        std::fs::write(folder.join(name), "# Text\n\n#plan\n").expect("write");
    }
    state
        .notes_index
        .reconcile(&root, &|| false, &|_| false)
        .expect("walk");

    let mut shown = Vec::new();
    let mut folders = vec![root.to_string_lossy().into_owned()];
    while let Some(folder) = folders.pop() {
        for entry in list_notes_dir_inner(&state, &folder).expect("listing") {
            if entry.is_dir {
                folders.push(entry.path);
            } else {
                shown.push(key(&entry.path));
            }
        }
    }
    shown.sort();
    let mut held = rows(&state);
    held.sort();

    assert!(
        held.contains(&key(&root
            .join("Projects")
            .join("Launch.md")
            .to_string_lossy())),
        "the walk reads folders: {held:?}"
    );
    assert_eq!(shown, held);
}
