//! Coverage for the four version commands (spec H1).
//!
//! Each is exercised through its Tauri-free inner function against a real
//! notes folder and a real version store, so the assertions cover what the
//! panel receives and what the folder holds afterwards. The last test asserts
//! every one of them is in the invoke handler, since a command that is not
//! registered cannot be called however well it behaves.
//!
//! The exception is a restore clicked twice, which turns on the record a tab
//! keeps of its file rather than on the write: that one runs against an app
//! with the note open.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime};

use tempfile::TempDir;
use writ_core::activity::{Actor, Decision};
use writ_core::config::WritConfig;
use writ_core::events::bus::{EventBus, WritEvent};
use writ_core::notes::guard::DiskState;
use writ_core::preview::ContentRendererRegistry;
use writ_core::update::UpdatePhase;
use writ_core::watcher::change_event::{modification_is_news, ExternalChange};
use writ_core::watcher::reconcile::ReconcileGate;
use writ_plugin::transform::TransformRegistry;
use writ_storage::buffer_store::BufferStore;
use writ_storage::config_store::ConfigStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::layout_state::LayoutStateStore;
use writ_storage::note_history::NoteHistoryStore;
use writ_storage::notes_index::NotesIndexStore;
use writ_tauri_lib::commands::buffer::{read_buffer_content_inner, save_buffer_content_inner};
use writ_tauri_lib::commands::file::open_file_from_path;
use writ_tauri_lib::commands::note_history::{
    copy_note_version_inner, note_version_content_inner, note_versions_inner,
    restore_note_version_for_tab, restore_note_version_inner, NoteVersion,
};
use writ_tauri_lib::preview::handler::RenderCache;
use writ_tauri_lib::quit::QuitState;
use writ_tauri_lib::security::{canonicalize_for_authorization, AuthorizedPaths};
use writ_tauri_lib::state::AppState;
use writ_tauri_lib::watcher::handler::create_ignore_set;
use writ_tauri_lib::watcher::identity::PlatformIdentity;

const LIB_RS: &str = include_str!("../src/lib.rs");

const COMMANDS: &[&str] = &[
    "commands::note_history::note_versions",
    "commands::note_history::note_version_content",
    "commands::note_history::restore_note_version",
    "commands::note_history::copy_note_version",
];

/// A notes folder and the store that keeps its versions, wired the way the app
/// wires them.
struct Folder {
    dir: TempDir,
    notes: PathBuf,
    writ_dir: PathBuf,
    store: NoteHistoryStore,
}

impl Folder {
    fn new() -> Self {
        let dir = TempDir::new().expect("temp dir");
        let writ_dir = dir.path().join("data");
        std::fs::create_dir_all(&writ_dir).expect("data folder");
        let notes = writ_dir.join("Writ");
        std::fs::create_dir_all(&notes).expect("notes folder");
        let notes = writ_tauri_lib::security::canonicalize_root(&notes).expect("canonical");

        let store = NoteHistoryStore::open(&writ_dir).expect("version store");
        store.set_notes_root(notes.clone());
        store.set_probe(Arc::new(PlatformIdentity));
        Self {
            dir,
            notes,
            writ_dir,
            store,
        }
    }

    /// Writes a note and keeps that text, at `seconds_ago`.
    fn note(&self, name: &str, bytes: &[u8], seconds_ago: u64) -> PathBuf {
        let path = self.notes.join(name);
        std::fs::write(&path, bytes).expect("write");
        self.keep(&path, bytes, seconds_ago);
        path
    }

    /// Keeps one text of a note, without the merge window: a fixture wants
    /// each text it hands over to be an entry of its own.
    fn keep(&self, path: &Path, bytes: &[u8], seconds_ago: u64) {
        let key = self.store.key_for(path).expect("a note inside the folder");
        let at = SystemTime::now() - Duration::from_secs(seconds_ago);
        self.store
            .capture_replaced(&key, bytes, at)
            .expect("keep the text");
    }

    fn versions(&self, path: &Path) -> Vec<NoteVersion> {
        note_versions_inner(&self.store, path).expect("versions")
    }

    fn text_of(&self, version: &NoteVersion) -> String {
        note_version_content_inner(&self.store, version.id).expect("the text of an entry")
    }

    /// What Writ would have recorded for a tab holding these bytes.
    fn last_known(bytes: &[u8]) -> DiskState {
        DiskState {
            hash: writ_core::hash::sha256_bytes(bytes),
            size: bytes.len() as u64,
            mtime: None,
        }
    }

    /// The names the folder holds, sorted.
    fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.notes)
            .expect("read the folder")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }
}

/// The app as it runs, for the one test that needs a tab: a notes folder, a
/// version store wired to it, and a buffer store that hands its writes to
/// that store.
///
/// Built here rather than shared, which is how every other file under
/// `tests/` builds an `AppState`.
struct Running {
    _dir: TempDir,
    notes: PathBuf,
    state: AppState,
}

impl Running {
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

    /// The names the folder holds, sorted.
    fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.notes)
            .expect("read the folder")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }

    /// Collects what the bus carries from here on.
    ///
    /// Attached at the point a test is about to act, not at the open, so what
    /// it holds is the answer to that one call rather than everything the
    /// setup raised on the way there.
    fn events(&self) -> Arc<Mutex<Vec<WritEvent>>> {
        let events = Arc::new(Mutex::new(Vec::new()));
        let collected = Arc::clone(&events);
        self.state.event_bus.subscribe(move |event| {
            collected
                .lock()
                .expect("collected events")
                .push(event.clone());
        });
        events
    }
}

/// The external-change events among what was collected.
fn external_events(events: &Mutex<Vec<WritEvent>>) -> Vec<WritEvent> {
    events
        .lock()
        .expect("collected events")
        .iter()
        .filter(|event| matches!(event, WritEvent::BufferExternal { .. }))
        .cloned()
        .collect()
}

#[test]
fn note_versions_lists_what_a_note_held_newest_first() {
    let folder = Folder::new();
    let path = folder.note("Launch.md", b"the first\n", 60);
    folder.keep(&path, b"the second\n", 30);

    let listed = folder.versions(&path);

    assert_eq!(listed.len(), 2);
    assert_eq!(folder.text_of(&listed[0]), "the second\n");
    assert_eq!(folder.text_of(&listed[1]), "the first\n");
    assert!(listed[0].at_ms > listed[1].at_ms);
    assert_eq!(listed[0].bytes, b"the second\n".len() as u64);
}

#[test]
fn a_file_the_notes_folder_does_not_hold_has_no_versions() {
    let folder = Folder::new();
    let outside = folder.dir.path().join("elsewhere.md");
    std::fs::write(&outside, "not a note\n").expect("write");

    assert!(note_versions_inner(&folder.store, &outside)
        .expect("an answer, not a failure")
        .is_empty());
}

#[test]
fn note_version_content_reads_the_text_an_entry_holds() {
    let folder = Folder::new();
    let path = folder.note("Launch.md", b"what it said\n", 60);

    let listed = folder.versions(&path);

    assert_eq!(folder.text_of(&listed[0]), "what it said\n");
}

#[test]
fn an_entry_that_is_gone_reads_as_gone_rather_than_as_a_failure_of_the_store() {
    let folder = Folder::new();

    let error = note_version_content_inner(&folder.store, 4_242).expect_err("no such entry");

    assert_eq!(error, "That version is not here any more.");
}

#[test]
fn restoring_a_version_writes_it_back_and_keeps_the_text_it_replaced() {
    let folder = Folder::new();
    let path = folder.note("Launch.md", b"the first\n", 60);
    std::fs::write(&path, b"the second\n").expect("overwrite");
    folder.keep(&path, b"the second\n", 30);

    let listed = folder.versions(&path);
    let first = listed[1].id;

    let restored =
        restore_note_version_inner(&folder.notes, &folder.writ_dir, &folder.store, first, None)
            .expect("restore");

    assert_eq!(restored.note, "Launch.md");
    assert_eq!(std::fs::read(&path).expect("read"), b"the first\n");

    let after = folder.versions(&path);
    assert_eq!(
        folder.text_of(&after[0]),
        "the first\n",
        "the restore is an entry of its own"
    );
    assert!(
        after
            .iter()
            .any(|entry| folder.text_of(entry) == "the second\n"),
        "and what it replaced is still there to go back to"
    );
}

#[test]
fn restoring_twice_returns_the_note_to_where_it_started() {
    let folder = Folder::new();
    let path = folder.note("Launch.md", b"the first\n", 60);
    std::fs::write(&path, b"the second\n").expect("overwrite");
    folder.keep(&path, b"the second\n", 30);

    let first = folder.versions(&path)[1].id;
    restore_note_version_inner(&folder.notes, &folder.writ_dir, &folder.store, first, None)
        .expect("restore");

    let back = folder
        .versions(&path)
        .into_iter()
        .find(|entry| folder.text_of(entry) == "the second\n")
        .expect("the text the restore replaced");
    // What the tab holding the note has recorded by now: the first restore
    // wrote `the first`, and a restore tells the tab what it wrote the way a
    // save does. `None` here would be the one case the command never reaches,
    // a note nothing has open.
    restore_note_version_inner(
        &folder.notes,
        &folder.writ_dir,
        &folder.store,
        back.id,
        Some(Folder::last_known(b"the first\n")),
    )
    .expect("restore back");

    assert_eq!(std::fs::read(&path).expect("read"), b"the second\n");
}

#[test]
fn a_note_changed_underneath_is_refused_and_the_version_is_written_beside_it() {
    let folder = Folder::new();
    let path = folder.note("Launch.md", b"the first\n", 60);
    let last_known = Folder::last_known(b"the first\n");
    // Somebody else writes the file, and Writ never read what they wrote.
    std::fs::write(&path, b"what somebody else wrote\n").expect("overwrite");

    let version = folder.versions(&path)[0].id;
    let error = restore_note_version_inner(
        &folder.notes,
        &folder.writ_dir,
        &folder.store,
        version,
        Some(last_known),
    )
    .expect_err("refused");

    assert!(
        error.starts_with("Launch.md changed on disk."),
        "got: {error}"
    );
    assert_eq!(
        std::fs::read(&path).expect("read"),
        b"what somebody else wrote\n",
        "the file keeps what it holds"
    );
    let copy = folder
        .names()
        .into_iter()
        .find(|name| name != "Launch.md")
        .expect("a copy beside the note");
    assert!(error.contains(&copy), "the refusal names it: {error}");
    assert_eq!(
        std::fs::read(folder.notes.join(&copy)).expect("read the copy"),
        b"the first\n"
    );
}

#[test]
fn a_restore_puts_back_every_byte_it_was_given() {
    let folder = Folder::new();
    // Frontmatter, CRLF line endings and a byte order mark: everything a
    // round trip through a string would quietly rewrite (spec 163).
    let original: &[u8] =
        b"\xef\xbb\xbf---\r\ntitle: Launch\r\ntags: [a, b]\r\n---\r\n\r\nBody\r\n";
    let path = folder.note("Launch.md", original, 60);
    std::fs::write(&path, "something else\n").expect("overwrite");

    let version = folder.versions(&path)[0].id;
    restore_note_version_inner(
        &folder.notes,
        &folder.writ_dir,
        &folder.store,
        version,
        None,
    )
    .expect("restore");

    assert_eq!(std::fs::read(&path).expect("read"), original);
}

#[test]
fn copying_a_version_leaves_the_note_alone() {
    let folder = Folder::new();
    let path = folder.note("Launch.md", b"the first\n", 60);
    std::fs::write(&path, b"the second\n").expect("overwrite");
    folder.keep(&path, b"the second\n", 30);

    let first = folder.versions(&path)[1].id;
    let copy = copy_note_version_inner(
        &folder.notes,
        &folder.writ_dir,
        &folder.store,
        first,
        chrono::Utc::now(),
        None,
    )
    .expect("copy");

    assert!(copy.name.starts_with("Launch ("), "got: {}", copy.name);
    assert!(copy.name.ends_with(".md"), "got: {}", copy.name);
    assert_eq!(
        std::fs::read(folder.notes.join(&copy.name)).expect("read the copy"),
        b"the first\n"
    );
    assert_eq!(
        std::fs::read(&path).expect("read"),
        b"the second\n",
        "the note is untouched"
    );
}

#[test]
fn a_version_copy_is_stamped_for_the_watcher_like_a_conflict_copy() {
    let folder = Folder::new();
    let path = folder.note("Launch.md", b"the first\n", 60);
    let version = folder.versions(&path)[0].id;

    let stamped: Mutex<Vec<(PathBuf, Vec<u8>)>> = Mutex::new(Vec::new());
    let hook = |written: &Path, bytes: &[u8]| {
        stamped
            .lock()
            .expect("the stamp list")
            .push((written.to_path_buf(), bytes.to_vec()));
    };

    let copy = copy_note_version_inner(
        &folder.notes,
        &folder.writ_dir,
        &folder.store,
        version,
        chrono::Utc::now(),
        Some(&hook),
    )
    .expect("copy");

    assert_eq!(
        stamped.into_inner().expect("the stamp list"),
        vec![(folder.notes.join(&copy.name), b"the first\n".to_vec())],
        "the watcher is told about the file before it appears"
    );
}

#[test]
fn a_restore_and_a_copy_each_leave_a_line_in_the_activity_log_naming_no_text() {
    let folder = Folder::new();
    let text = b"the only text this note ever held\n";
    let path = folder.note("Launch.md", text, 60);
    let version = folder.versions(&path)[0].id;

    restore_note_version_inner(
        &folder.notes,
        &folder.writ_dir,
        &folder.store,
        version,
        None,
    )
    .expect("restore");
    copy_note_version_inner(
        &folder.notes,
        &folder.writ_dir,
        &folder.store,
        version,
        chrono::Utc::now(),
        None,
    )
    .expect("copy");

    let lines = writ_storage::activity_log::read_recent(&folder.writ_dir, 10);
    for action in ["restore_note_version", "copy_note_version"] {
        let line = lines
            .iter()
            .find(|record| record.action == action)
            .unwrap_or_else(|| panic!("{action} left no line: {lines:?}"));
        assert_eq!(line.actor, Actor::App);
        assert_eq!(line.decision, Decision::Allow);
        assert_eq!(line.path.as_deref(), Some(Path::new("Launch.md")));
        assert_eq!(line.bytes, Some(text.len() as u64));
    }

    let written = format!("{lines:?}");
    assert!(
        !written.contains("the only text this note ever held"),
        "a log line holds a length and a name, never what the note said: {written}"
    );
}

#[test]
fn every_version_command_is_in_the_invoke_handler() {
    for command in COMMANDS {
        assert!(
            LIB_RS.contains(command),
            "{command} is not registered in the invoke handler"
        );
    }
}

#[test]
fn a_restore_undone_by_the_row_above_it_lands_rather_than_reading_as_a_change_on_disk() {
    let app = Running::new();
    let (id, path) = app.open("Launch.md", "the first\n");
    save_buffer_content_inner(&app.state, &id, "the second\n").expect("save");

    // The panel stays open on the same list, so the way back from a restore
    // is the row above it, one click later.
    let first = app.version_holding(&path, b"the first\n");
    restore_note_version_for_tab(&app.state, first).expect("restore");
    let second = app.version_holding(&path, b"the second\n");
    restore_note_version_for_tab(&app.state, second)
        .expect("the second restore is the first one undone, not a file somebody else changed");

    assert_eq!(std::fs::read(&path).expect("read"), b"the second\n");
    assert_eq!(
        app.names(),
        vec!["Launch.md".to_string()],
        "a restore Writ itself wrote leaves no dated copy behind"
    );
}

#[test]
fn a_restore_tells_the_tab_holding_the_note_what_it_put_on_the_file() {
    let app = Running::new();
    let (id, path) = app.open("Launch.md", "the first\n");
    save_buffer_content_inner(&app.state, &id, "the second\n").expect("save");
    let first = app.version_holding(&path, b"the first\n");

    let events = app.events();
    restore_note_version_for_tab(&app.state, first).expect("restore");

    let raised = external_events(&events);
    assert_eq!(
        raised.len(),
        1,
        "a restore tells the tab holding the note once: {raised:?}"
    );
    match &raised[0] {
        WritEvent::BufferExternal {
            buffer_id,
            path: told,
            change,
            new_path,
            disk_hash,
        } => {
            assert_eq!(buffer_id, &id, "the tab holding the note");
            assert_eq!(Path::new(told), path.as_path());
            assert_eq!(change, &ExternalChange::Modified);
            assert_eq!(new_path, &None);
            assert_eq!(
                disk_hash.as_deref(),
                Some(writ_core::hash::comparison_digest_hex(b"the first\n").as_str()),
                "the digest is the restored text's, in the form the editor compares its \
                 document against"
            );
        }
        other => panic!("a restore is a modification: {other:?}"),
    }
}

#[test]
fn a_restore_the_guard_refuses_tells_the_tab_nothing() {
    let app = Running::new();
    let (id, path) = app.open("Launch.md", "the first\n");
    save_buffer_content_inner(&app.state, &id, "the second\n").expect("save");
    let first = app.version_holding(&path, b"the first\n");
    // A write the tab never read, so what Writ recorded for it no longer
    // describes the file and the guard refuses the restore.
    std::fs::write(&path, b"somebody else\n").expect("write");
    let recorded = app.state.disk_state(&id).expect("a record of the file");

    let events = app.events();
    restore_note_version_for_tab(&app.state, first)
        .expect_err("a note that changed underneath is refused");

    assert!(
        external_events(&events).is_empty(),
        "a restore that did not land has nothing to tell the tab"
    );
    assert_eq!(
        app.state.disk_state(&id),
        Some(recorded),
        "a refused restore leaves the tab's record of its file alone"
    );
}

#[test]
fn a_restore_of_a_note_nothing_has_open_tells_no_tab() {
    let app = Running::new();
    let path = app.notes.join("Launch.md");
    std::fs::write(&path, b"the second\n").expect("write");
    let note = app
        .state
        .note_history
        .key_for(&path)
        .expect("a note inside the folder");
    app.state
        .note_history
        .capture_replaced(
            &note,
            b"the first\n",
            SystemTime::now() - Duration::from_secs(60),
        )
        .expect("keep the text");
    let first = app.version_holding(&path, b"the first\n");

    let events = app.events();
    restore_note_version_for_tab(&app.state, first).expect("restore");

    assert_eq!(std::fs::read(&path).expect("read"), b"the first\n");
    assert!(
        external_events(&events).is_empty(),
        "a note no tab holds has nobody to tell"
    );
}

#[test]
fn the_watcher_has_nothing_to_add_once_a_restore_has_told_the_tab() {
    let app = Running::new();
    let (id, path) = app.open("Launch.md", "the first\n");
    save_buffer_content_inner(&app.state, &id, "the second\n").expect("save");
    let first = app.version_holding(&path, b"the first\n");

    let events = app.events();
    restore_note_version_for_tab(&app.state, first).expect("restore");

    // The two halves of telling the tab what the restore wrote: the command
    // raises the event, and the record it keeps is what makes the watcher's
    // own report of the same write no news rather than a second announcement.
    assert_eq!(
        external_events(&events).len(),
        1,
        "the command tells the tab what the restore wrote"
    );
    let recorded = app.state.disk_state(&id).expect("a record of the file");
    let on_disk = writ_core::hash::sha256_bytes(&std::fs::read(&path).expect("read"));
    assert!(
        !modification_is_news(Some(recorded.hash), Some(on_disk), false),
        "the watcher reporting the restore is telling the tab what it already knows"
    );
}
