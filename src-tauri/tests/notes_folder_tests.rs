//! The notes folder as Settings drives it: where it is, moving it, emptying
//! the archive into it, and the report the migration left (ADR-028 §2, §4).

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use chrono::Utc;
use tempfile::TempDir;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_core::config::WritConfig;
use writ_core::events::bus::EventBus;
use writ_core::preview::ContentRendererRegistry;
use writ_core::update::UpdatePhase;
use writ_plugin::transform::TransformRegistry;
use writ_storage::buffer_store::BufferStore;
use writ_storage::config_store::ConfigStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::database::queries;
use writ_storage::layout_state::LayoutStateStore;
use writ_storage::notes_index::NotesIndexStore;
use writ_storage::notes_migration::{MigrationReport, RowOutcome};
use writ_storage::schema_meta::{self, KEY_NOTES_MIGRATION_REPORT};
use writ_tauri_lib::commands::buffer::save_buffer_content_inner;
use writ_tauri_lib::commands::notes::{
    dismiss_notes_migration_report_inner, move_archived_notes_inner, move_notes_folder_to,
    notes_folder_info, notes_migration_report,
};
use writ_tauri_lib::preview::handler::RenderCache;
use writ_tauri_lib::quit::QuitState;
use writ_tauri_lib::security::AuthorizedPaths;
use writ_tauri_lib::state::{
    resolve_and_create_notes_root, AppState, NotesRootFallback, NotesRootFallbackReason,
};
use writ_tauri_lib::watcher::handler::create_ignore_set;

fn make_state_at(dir: &TempDir, notes_name: &str, fallback: Option<NotesRootFallback>) -> AppState {
    let writ_dir = dir.path().join("data");
    let buffers_dir = writ_dir.join("buffers");
    std::fs::create_dir_all(&buffers_dir).expect("buffers dir");

    let notes_root = dir.path().join(notes_name);
    std::fs::create_dir_all(&notes_root).expect("notes folder");
    let notes_root = writ_tauri_lib::security::canonicalize_root(&notes_root).expect("canonical");

    let db_path = writ_dir.join("writ.db");
    let conn = open_database(&db_path).expect("open db");
    run_migrations(&conn).expect("migrations");

    AppState {
        store: Mutex::new(BufferStore::new(conn, buffers_dir.clone())),
        config_store: ConfigStore::new(writ_dir.join("config.toml")),
        config: Mutex::new(WritConfig::default()),
        writ_dir,
        buffers_dir,
        notes_root: RwLock::new(notes_root),
        notes_root_fallback: RwLock::new(fallback),
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

fn make_state(dir: &TempDir) -> AppState {
    make_state_at(dir, "Writ", None)
}

/// Inserts a row pointing at `source_path`, as an opened file would.
fn seed_row(state: &AppState, id: &str, source_path: Option<&str>) {
    let now = Utc::now();
    let doc = BufferDocument {
        id: id.to_string(),
        title: format!("{id}.md"),
        filename: format!("{id}.txt"),
        status: BufferStatus::Active,
        language: None,
        source_path: source_path.map(str::to_string),
        cursor_pos: 0,
        scroll_pos: 0,
        tab_order: 0,
        created_at: now,
        updated_at: now,
        closed_at: None,
        read_only: false,
        size_bytes: 0,
    };
    let store = state.store.lock().expect("lock");
    store.open_from_path(&doc, "").expect("insert row");
}

/// The database file, which a test opens its own connection to.
///
/// The store keeps its connection and does not lend it out; WAL permits a
/// second one, which is how the layout store already works.
fn db_path(state: &AppState) -> std::path::PathBuf {
    state.writ_dir.join("writ.db")
}

fn source_path_of(state: &AppState, id: &str) -> Option<String> {
    let store = state.store.lock().expect("lock");
    store.get(id).expect("row").source_path
}

#[test]
fn move_refuses_when_the_destination_has_a_colliding_name_and_names_it() {
    let dir = TempDir::new().expect("temp");
    let state = make_state(&dir);
    let from = state.notes_root();

    std::fs::write(from.join("Grocery list.md"), "mine").expect("seed");
    std::fs::write(from.join("Recipes.md"), "mine too").expect("seed");
    let to = dir.path().join("Elsewhere");
    std::fs::create_dir_all(&to).expect("destination");
    std::fs::write(to.join("grocery list.md"), "theirs").expect("seed");

    let outcome = move_notes_folder_to(&state, &to).expect("move");

    assert_eq!(outcome.collided, vec!["Grocery list.md".to_string()]);
    assert_eq!(outcome.moved, 0);
    assert_eq!(outcome.new_root, from.to_string_lossy());
    assert_eq!(state.notes_root(), from, "the folder did not move");
    assert!(from.join("Recipes.md").exists(), "nothing else moved");
    assert_eq!(
        std::fs::read_to_string(to.join("grocery list.md")).expect("read"),
        "theirs",
        "the file already there is untouched"
    );
}

#[test]
fn move_rewrites_source_paths_for_every_row_under_the_old_root() {
    let dir = TempDir::new().expect("temp");
    let state = make_state(&dir);
    let from = state.notes_root();

    std::fs::create_dir_all(from.join("projects")).expect("subfolder");
    std::fs::write(from.join("One.md"), "one").expect("seed");
    std::fs::write(from.join("projects").join("Two.md"), "two").expect("seed");
    let outside = dir.path().join("outside.md");
    std::fs::write(&outside, "somebody else's").expect("seed");

    seed_row(&state, "one", Some(&from.join("One.md").to_string_lossy()));
    seed_row(
        &state,
        "two",
        Some(&from.join("projects").join("Two.md").to_string_lossy()),
    );
    seed_row(&state, "outside", Some(&outside.to_string_lossy()));

    let to = dir.path().join("Elsewhere");
    let outcome = move_notes_folder_to(&state, &to).expect("move");
    let to = writ_tauri_lib::security::canonicalize_root(&to).expect("canonical");

    assert_eq!(outcome.moved, 2);
    assert!(outcome.collided.is_empty());
    assert_eq!(state.notes_root(), to);
    assert_eq!(
        source_path_of(&state, "one").as_deref(),
        Some(to.join("One.md").to_string_lossy().as_ref())
    );
    assert_eq!(
        source_path_of(&state, "two").as_deref(),
        Some(
            to.join("projects")
                .join("Two.md")
                .to_string_lossy()
                .as_ref()
        )
    );
    assert_eq!(
        source_path_of(&state, "outside").as_deref(),
        Some(outside.to_string_lossy().as_ref()),
        "a file outside the notes folder keeps its own path"
    );

    let config = state.config.lock().expect("lock");
    assert_eq!(
        config.notes.root.as_deref(),
        Some(to.to_string_lossy().as_ref())
    );
}

#[test]
fn notes_folder_path_round_trips_spaces_apostrophe_and_arabic() {
    let dir = TempDir::new().expect("temp");
    let state = make_state(&dir);

    let awkward = dir.path().join("It's a folder").join("مجلد ملاحظات");
    std::fs::create_dir_all(&awkward).expect("awkward folder");
    std::fs::write(state.notes_root().join("ملاحظة.md"), "نص").expect("seed");
    seed_row(
        &state,
        "arabic",
        Some(&state.notes_root().join("ملاحظة.md").to_string_lossy()),
    );

    let outcome = move_notes_folder_to(&state, &awkward).expect("move");
    let awkward = writ_tauri_lib::security::canonicalize_root(&awkward).expect("canonical");

    assert_eq!(outcome.moved, 1);
    assert_eq!(outcome.new_root, awkward.to_string_lossy());

    let info = notes_folder_info(&state);
    assert_eq!(info.path, awkward.to_string_lossy());
    assert!(
        info.display_path.ends_with("مجلد ملاحظات"),
        "{}",
        info.display_path
    );
    assert_eq!(info.fallback, None);

    let moved = awkward.join("ملاحظة.md");
    assert_eq!(std::fs::read_to_string(&moved).expect("read"), "نص");
    assert_eq!(
        source_path_of(&state, "arabic").as_deref(),
        Some(moved.to_string_lossy().as_ref())
    );
    assert!(state.is_within_notes(&moved.to_string_lossy()));

    save_buffer_content_inner(&state, "arabic", "نص محفوظ").expect("save through the write gate");

    assert_eq!(
        std::fs::read(&moved).expect("read back"),
        "نص محفوظ".as_bytes(),
        "the bytes on disk are the ones that were saved"
    );
    assert_eq!(
        source_path_of(&state, "arabic").as_deref(),
        Some(moved.to_string_lossy().as_ref()),
        "the row still names the file the save landed in"
    );
    assert_eq!(moved.parent(), Some(awkward.as_path()));

    seed_row(&state, "minted", None);
    save_buffer_content_inner(&state, "minted", "ملاحظة جديدة").expect("mint a file and save it");

    let minted = source_path_of(&state, "minted").expect("the note was given a file");
    let minted = std::path::Path::new(&minted);
    assert_eq!(minted.parent(), Some(awkward.as_path()));
    assert_eq!(
        std::fs::read(minted).expect("read back"),
        "ملاحظة جديدة".as_bytes()
    );
}

#[test]
fn the_folder_row_names_the_one_that_could_not_be_used() {
    let dir = TempDir::new().expect("temp");
    let state = make_state_at(
        &dir,
        "Writ",
        Some(NotesRootFallback {
            from: "/nope/Writ".to_string(),
            reason: NotesRootFallbackReason::Unusable,
        }),
    );
    let info = notes_folder_info(&state);
    assert_eq!(
        info.fallback,
        Some(NotesRootFallback {
            from: "/nope/Writ".to_string(),
            reason: NotesRootFallbackReason::Unusable,
        })
    );
    assert_eq!(info.path, state.notes_root().to_string_lossy());
}

#[test]
fn move_archived_notes_dedupes_and_updates_rows() {
    let dir = TempDir::new().expect("temp");
    let state = make_state(&dir);
    let notes = state.notes_root();
    let archive = state.writ_dir.join("archive");
    std::fs::create_dir_all(&archive).expect("archive");

    std::fs::write(notes.join("Meeting.md"), "the note already there").expect("seed");
    std::fs::write(archive.join("Meeting.md"), "archived one").expect("seed");
    std::fs::write(archive.join("Ideas.md"), "archived two").expect("seed");

    seed_row(&state, "meeting", None);
    seed_row(&state, "ideas", None);
    {
        let conn = open_database(&db_path(&state)).expect("open db");
        queries::mark_migrated(
            &conn,
            "meeting",
            &archive.join("Meeting.md").to_string_lossy(),
            0,
        )
        .expect("mark");
        queries::mark_migrated(
            &conn,
            "ideas",
            &archive.join("Ideas.md").to_string_lossy(),
            0,
        )
        .expect("mark");
    }

    let outcome = move_archived_notes_inner(&state).expect("move the archive");

    assert_eq!(outcome.moved, 2);
    assert_eq!(outcome.collided, vec!["Meeting.md".to_string()]);
    assert_eq!(
        std::fs::read_to_string(notes.join("Meeting.md")).expect("read"),
        "the note already there",
        "the note already in the folder is untouched"
    );
    assert_eq!(
        std::fs::read_to_string(notes.join("Meeting 2.md")).expect("read"),
        "archived one"
    );
    assert_eq!(
        source_path_of(&state, "meeting").as_deref(),
        Some(notes.join("Meeting 2.md").to_string_lossy().as_ref())
    );
    assert_eq!(
        source_path_of(&state, "ideas").as_deref(),
        Some(notes.join("Ideas.md").to_string_lossy().as_ref())
    );

    assert_eq!(
        queries::get_migrated_path(
            &open_database(&db_path(&state)).expect("open db"),
            "meeting"
        )
        .expect("migrated path")
        .as_deref(),
        Some(notes.join("Meeting 2.md").to_string_lossy().as_ref()),
        "the record no longer names the archive"
    );
    assert!(
        !archive.join("Meeting.md").exists() && !archive.join("Ideas.md").exists(),
        "the archive is empty"
    );
}

// A destination is compared against paths Writ has resolved, and on macOS a
// temporary folder is handed out as `/var/...` while its resolved form is
// `/private/var/...`. Both refusals have to answer on the resolved pair or
// they never fire on the platform Writ ships on first.
#[test]
fn a_folder_inside_the_notes_folder_is_refused_whichever_way_it_is_spelled() {
    let dir = TempDir::new().expect("temp");
    let state = make_state(&dir);
    std::fs::write(state.notes_root().join("One.md"), "one").expect("seed");

    let inside = dir.path().join("Writ").join("deeper");
    let error = move_notes_folder_to(&state, &inside).expect_err("refused");

    assert_eq!(error, "Pick a folder outside your notes folder.");
    assert!(state.notes_root().join("One.md").exists(), "nothing moved");
}

#[test]
fn a_folder_holding_writs_own_data_is_refused_whichever_way_it_is_spelled() {
    let dir = TempDir::new().expect("temp");
    let state = make_state(&dir);
    std::fs::write(state.notes_root().join("One.md"), "one").expect("seed");
    let before = state.notes_root();

    let error = move_notes_folder_to(&state, dir.path()).expect_err("refused");

    assert_eq!(
        error,
        "Writ keeps its own data in that folder, so it cannot also be your notes folder."
    );
    assert_eq!(state.notes_root(), before);
    assert!(before.join("One.md").exists(), "nothing moved");
}

#[test]
fn a_folder_inside_writs_own_data_folder_is_refused_and_nothing_is_created() {
    let dir = TempDir::new().expect("temp");
    let state = make_state(&dir);
    std::fs::write(state.notes_root().join("One.md"), "one").expect("seed");
    let before = state.notes_root();

    let inside = state.writ_dir.join("archive");
    let error = move_notes_folder_to(&state, &inside).expect_err("refused");

    assert_eq!(
        error,
        "Writ keeps its own data in that folder, so it cannot also be your notes folder."
    );
    assert_eq!(state.notes_root(), before);
    assert!(before.join("One.md").exists(), "nothing moved");
    assert!(
        !inside.exists(),
        "a folder that is refused is not created on the way to refusing it"
    );
}

#[test]
fn a_folder_that_does_not_exist_yet_under_the_data_folder_is_refused_before_it_is_created() {
    let dir = TempDir::new().expect("temp");
    let state = make_state(&dir);
    std::fs::write(state.notes_root().join("One.md"), "one").expect("seed");
    let before = state.notes_root();

    let archive = state.writ_dir.join("archive");
    let two_levels_down = archive.join("new-notes");
    let error = move_notes_folder_to(&state, &two_levels_down).expect_err("refused");

    assert_eq!(
        error,
        "Writ keeps its own data in that folder, so it cannot also be your notes folder."
    );
    assert_eq!(state.notes_root(), before);
    assert!(before.join("One.md").exists(), "nothing moved");
    assert!(
        !two_levels_down.exists(),
        "the picked folder was not created"
    );
    assert!(!archive.exists(), "nor the level above it");
}

#[test]
fn the_archive_can_never_become_its_own_destination() {
    let dir = TempDir::new().expect("temp");
    let state = make_state(&dir);
    let archive = state.writ_dir.join("archive");
    std::fs::create_dir_all(&archive).expect("archive");
    std::fs::write(archive.join("Ideas.md"), "archived").expect("seed");

    move_notes_folder_to(&state, &archive).expect_err("the archive cannot be the notes folder");
    assert_ne!(state.notes_root(), archive);

    let outcome = move_archived_notes_inner(&state).expect("move the archive");
    assert_eq!(outcome.moved, 1);
    assert!(
        outcome.collided.is_empty(),
        "nothing was renamed onto itself"
    );
    assert_eq!(
        std::fs::read_to_string(state.notes_root().join("Ideas.md")).expect("read"),
        "archived"
    );
    assert!(!archive.join("Ideas.md").exists(), "the archive is empty");
}

#[test]
fn the_archive_offer_does_not_come_back_after_the_notes_have_been_moved() {
    let dir = TempDir::new().expect("temp");
    let state = make_state(&dir);
    let archive = state.writ_dir.join("archive");
    std::fs::create_dir_all(&archive).expect("archive");
    std::fs::write(archive.join("Ideas.md"), "archived").expect("seed");

    seed_row(&state, "ideas", None);
    let archived_path = archive.join("Ideas.md").to_string_lossy().into_owned();
    queries::mark_migrated(
        &open_database(&db_path(&state)).expect("open db"),
        "ideas",
        &archived_path,
        0,
    )
    .expect("mark");
    put_report(
        &state,
        &MigrationReport {
            ran_at: Utc::now().to_rfc3339(),
            first_ran_at: Utc::now().to_rfc3339(),
            notes_folder: state.notes_root().to_string_lossy().into_owned(),
            archive_folder: archive.to_string_lossy().into_owned(),
            archived: 1,
            rows: vec![(
                "ideas".to_string(),
                RowOutcome::Archived {
                    path: archived_path,
                },
            )],
            ..MigrationReport::default()
        },
    );

    assert_eq!(
        notes_migration_report(&state)
            .expect("report")
            .map(|report| report.archived),
        Some(1)
    );

    let outcome = move_archived_notes_inner(&state).expect("move the archive");
    assert_eq!(outcome.moved, 1);

    // A second launch over the same database, which is where an offer that was
    // only cleared in memory would come back.
    let relaunched = make_state(&dir);
    let report = notes_migration_report(&relaunched)
        .expect("report")
        .expect("still worth showing");
    assert_eq!(report.archived, 0, "the archive is empty and says so");
    assert_eq!(report.migrated, 1, "the note is now a file in the folder");
}

#[test]
fn dismissed_report_stays_dismissed_across_launches() {
    let dir = TempDir::new().expect("temp");
    let state = make_state(&dir);
    store_report(&state);

    let shown = notes_migration_report(&state).expect("report");
    assert_eq!(shown.map(|report| report.migrated), Some(1));

    dismiss_notes_migration_report_inner(&state).expect("dismiss");
    assert_eq!(notes_migration_report(&state).expect("report"), None);

    // A second launch over the same database, which is what "once" has to
    // survive: the dismissal is a row, not a signal in this process.
    let relaunched = make_state(&dir);
    assert_eq!(notes_migration_report(&relaunched).expect("report"), None);
}

#[test]
fn a_run_that_placed_nothing_has_no_report_to_show() {
    let dir = TempDir::new().expect("temp");
    let state = make_state(&dir);
    put_report(&state, &MigrationReport::default());
    assert_eq!(notes_migration_report(&state).expect("report"), None);
}

#[test]
fn pick_notes_folder_move_updates_disk_state_map() {
    let dir = TempDir::new().expect("temp");
    let state = make_state(&dir);
    let from = state.notes_root();

    let note = from.join("Open.md");
    std::fs::write(&note, "open in a tab").expect("seed");
    seed_row(&state, "open", Some(&note.to_string_lossy()));
    state.record_disk_state_bytes("open", &note, b"open in a tab");

    let closed = from.join("Closed.md");
    std::fs::write(&closed, "not open").expect("seed");
    seed_row(&state, "closed", Some(&closed.to_string_lossy()));

    let before = state.disk_state("open").expect("recorded");

    let to = dir.path().join("Elsewhere");
    move_notes_folder_to(&state, &to).expect("move");
    let to = writ_tauri_lib::security::canonicalize_root(&to).expect("canonical");

    let after = state.disk_state("open").expect("still recorded");
    assert_eq!(after.hash, before.hash, "the bytes did not change");
    assert!(
        state.disk_hash_matches("open", b"open in a tab"),
        "the record describes the file in its new home"
    );
    assert!(
        state.disk_state("closed").is_none(),
        "a note Writ never read is not read now"
    );
    assert!(to.join("Open.md").exists());
}

/// Stores a report describing one migrated note, the way a real run would.
fn store_report(state: &AppState) {
    let report = MigrationReport {
        ran_at: Utc::now().to_rfc3339(),
        first_ran_at: Utc::now().to_rfc3339(),
        notes_folder: state.notes_root().to_string_lossy().into_owned(),
        archive_folder: state
            .writ_dir
            .join("archive")
            .to_string_lossy()
            .into_owned(),
        migrated: 1,
        rows: vec![(
            "one".to_string(),
            RowOutcome::WrittenToNotes {
                path: "One.md".to_string(),
            },
        )],
        ..MigrationReport::default()
    };
    put_report(state, &report);
}

/// Writes a report into the row the migration stores it in.
fn put_report(state: &AppState, report: &MigrationReport) {
    schema_meta::set(
        &open_database(&db_path(state)).expect("open db"),
        KEY_NOTES_MIGRATION_REPORT,
        &serde_json::to_string(report).expect("serialize"),
    )
    .expect("store the report");
}

#[test]
fn a_notes_folder_set_to_the_archive_boots_with_the_default_one() {
    let dir = TempDir::new().expect("temp");
    let writ_dir = dir.path().join("data");
    let archive = writ_dir.join("archive");
    std::fs::create_dir_all(&archive).expect("archive");
    std::fs::write(archive.join("Ideas.md"), "archived").expect("seed");

    let (root, fallback) = resolve_and_create_notes_root(
        writ_core::notes::NotesRootSources {
            env_override: Some(&archive.to_string_lossy()),
            configured: None,
            data_dir: Some(&writ_dir),
            home: Some(dir.path()),
        },
        &writ_dir,
    )
    .expect("startup resolves a notes folder");

    assert_eq!(
        root,
        writ_tauri_lib::security::canonicalize_root(&writ_dir.join("Writ"))
            .expect("the default under the data folder")
    );
    assert_eq!(
        fallback,
        Some(NotesRootFallback {
            from: archive.to_string_lossy().into_owned(),
            reason: NotesRootFallbackReason::HoldsWritData,
        }),
        "the Settings row can say which folder was turned down and why"
    );
    assert!(
        archive.join("Ideas.md").exists(),
        "the archive was left as it was"
    );
    assert_eq!(
        std::fs::read_dir(&archive)
            .expect("read the archive")
            .count(),
        1,
        "and nothing was created inside it"
    );
}

#[test]
fn a_destination_that_climbs_back_into_the_data_folder_creates_nothing() {
    let dir = TempDir::new().expect("temp");
    let state = make_state(&dir);
    let before = state.notes_root();

    let archive = state.writ_dir.join("archive");
    let climbing = archive.join("..");
    let error = move_notes_folder_to(&state, &climbing).expect_err("turned down");

    assert_eq!(error, "That folder cannot be your notes folder.");
    assert_eq!(state.notes_root(), before);
    assert!(
        !archive.exists(),
        "no folder was created inside the data folder"
    );
}

#[test]
fn moving_the_notes_folder_clears_the_row_naming_the_one_writ_could_not_use() {
    let dir = TempDir::new().expect("temp");
    let writ_dir = dir.path().join("data");
    let archive = writ_dir.join("archive");
    std::fs::create_dir_all(&archive).expect("archive");

    // The fallback startup itself would leave behind, not a hand-written one.
    let (_, fallback) = resolve_and_create_notes_root(
        writ_core::notes::NotesRootSources {
            env_override: Some(&archive.to_string_lossy()),
            configured: None,
            data_dir: Some(&writ_dir),
            home: Some(dir.path()),
        },
        &writ_dir,
    )
    .expect("startup resolves a notes folder");
    assert!(fallback.is_some(), "the launch this test starts from");

    let state = make_state_at(&dir, "Writ", fallback);
    std::fs::write(state.notes_root().join("One.md"), "one").expect("seed");
    assert!(
        notes_folder_info(&state).fallback.is_some(),
        "the row says so before the move"
    );

    let destination = dir.path().join("Moved Notes");
    let outcome = move_notes_folder_to(&state, &destination).expect("the move");
    assert_eq!(outcome.moved, 1);

    let info = notes_folder_info(&state);
    assert_eq!(
        info.fallback, None,
        "the settings now name the folder in use, so there is nothing to say"
    );
    assert_eq!(info.path, state.notes_root().to_string_lossy());
}

/// The index's paths for `text`, in rank order.
fn index_paths(state: &AppState, text: &str) -> Vec<String> {
    let query = writ_core::search::to_prefix_match(text).expect("query");
    let terms = writ_core::search::search_terms(text);
    state
        .notes_index
        .search_hits(&query, &terms, 50)
        .expect("search_hits")
        .into_iter()
        .map(|hit| hit.path.expect("a notes-index hit carries its path"))
        .collect()
}

#[test]
fn moving_the_notes_folder_reindexes_under_the_new_root_and_forgets_the_old_paths() {
    let dir = TempDir::new().expect("temp");
    let state = make_state(&dir);
    let from = state.notes_root();

    std::fs::create_dir_all(from.join("projects")).expect("subfolder");
    let before = from.join("projects").join("Kept.md");
    std::fs::write(&before, "portable sentence").expect("seed");
    state
        .notes_index
        .index_path(&before)
        .expect("index the note where it started");
    assert_eq!(
        index_paths(&state, "portable"),
        vec![writ_storage::notes_index::index_key(&before)]
    );

    let to = dir.path().join("Elsewhere");
    move_notes_folder_to(&state, &to).expect("move");
    let to = writ_tauri_lib::security::canonicalize_root(&to).expect("canonical");

    let after = to.join("projects").join("Kept.md");
    assert_eq!(
        index_paths(&state, "portable"),
        vec![writ_storage::notes_index::index_key(&after)],
        "the hit opens the file where it now is"
    );
    let old_prefix = from.to_string_lossy().into_owned();
    assert!(
        !state
            .notes_index
            .snapshot()
            .expect("snapshot")
            .iter()
            .any(|(path, _, _)| path.starts_with(&old_prefix)),
        "no row is left naming the old folder"
    );
}

#[test]
fn a_save_after_a_move_is_indexed_under_the_new_root() {
    let dir = TempDir::new().expect("temp");
    let state = make_state(&dir);
    let from = state.notes_root();
    let seeded = from.join("Later.md");
    std::fs::write(&seeded, "before").expect("seed");
    seed_row(&state, "later", Some(&seeded.to_string_lossy()));

    let to = dir.path().join("Elsewhere");
    move_notes_folder_to(&state, &to).expect("move");
    let to = writ_tauri_lib::security::canonicalize_root(&to).expect("canonical");

    save_buffer_content_inner(&state, "later", "afterwards").expect("save");
    // A save writes the file and defers the index write (ADR-020); the
    // scheduler calls this when the typing stops.
    state
        .store
        .lock()
        .expect("lock")
        .reindex_buffer("later")
        .expect("the deferred index write");

    assert_eq!(
        index_paths(&state, "afterwards"),
        vec![writ_storage::notes_index::index_key(&to.join("Later.md"))],
        "the save path indexes under the folder the state names now"
    );
}

#[test]
fn the_notes_watcher_follows_the_folder_to_its_new_root() {
    use std::time::{Duration, Instant};
    use writ_tauri_lib::watcher::handler::classify_notes_event;

    let dir = TempDir::new().expect("temp");
    let state = make_state(&dir);
    let from = state.notes_root();

    let to = dir.path().join("Elsewhere");
    move_notes_folder_to(&state, &to).expect("move");
    let to = writ_tauri_lib::security::canonicalize_root(&to).expect("canonical");

    assert!(
        state.notes_watcher.lock().expect("lock").is_some(),
        "the move leaves a watcher running"
    );

    // The debouncer's own thread is not driven here: what the restart has to
    // get right is which root the events are judged against, and that is a
    // pure function.
    let arrived = to.join("Arrived.md");
    std::fs::write(&arrived, "new").expect("seed");
    assert!(
        classify_notes_event(
            &arrived,
            &to,
            &state.watcher_ignore,
            Duration::from_secs(5),
            Instant::now(),
        )
        .is_some(),
        "a file created in the new folder is a notes change"
    );
    assert!(
        classify_notes_event(
            &from.join("Arrived.md"),
            &to,
            &state.watcher_ignore,
            Duration::from_secs(5),
            Instant::now(),
        )
        .is_none(),
        "a file in the folder Writ left is not"
    );
}
