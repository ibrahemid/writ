//! A rename that carries the notes linking to it (spec L3).
//!
//! Each command is exercised through its Tauri-free inner function against a
//! real notes folder and a real index, so what is asserted is what the editor
//! receives: the count before the rename, the files rewritten, and the files
//! named as left alone.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use tempfile::TempDir;
use writ_core::config::WritConfig;
use writ_core::events::bus::EventBus;
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
#[cfg(unix)]
use writ_tauri_lib::commands::buffer::ERR_READ_ONLY_DESTINATION;
use writ_tauri_lib::commands::buffer::{save_buffer_content_inner, ERR_FILE_CHANGED_ON_DISK};
use writ_tauri_lib::commands::notes::{
    count_links_to_inner, new_note_inner, rename_note_with_links_inner,
    undo_rename_with_links_inner, ERR_LINK_NAME_NOT_UNIQUE, ERR_LINK_NOT_FOUND,
};
use writ_tauri_lib::preview::handler::RenderCache;
use writ_tauri_lib::quit::QuitState;
use writ_tauri_lib::security::AuthorizedPaths;
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

    AppState {
        store: Mutex::new(BufferStore::new(conn, buffers_dir.clone())),
        config_store: ConfigStore::new(writ_dir.join("config.toml")),
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

/// A notes folder holding `notes`, indexed once, with the app state over it.
fn seeded(notes: &[(&str, &str)]) -> (TempDir, AppState) {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    for (name, body) in notes {
        let path = state.notes_root().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent");
        }
        std::fs::write(&path, body).expect("write note");
    }
    reindex(&state);
    (dir, state)
}

fn reindex(state: &AppState) {
    state
        .notes_index
        .reconcile(&state.notes_root(), &|| false, &|_| false)
        .expect("reconcile");
}

fn note(state: &AppState, name: &str) -> std::path::PathBuf {
    state.notes_root().join(name)
}

fn path_text(state: &AppState, name: &str) -> String {
    note(state, name).to_string_lossy().into_owned()
}

fn read(state: &AppState, name: &str) -> String {
    std::fs::read_to_string(note(state, name)).expect("read")
}

#[test]
fn the_count_names_the_notes_that_link_here() {
    let (_dir, state) = seeded(&[
        ("Old note.md", "the note itself\n"),
        ("First.md", "see [[Old note]]\n"),
        (
            "Second.md",
            "see [[Old note|the one]] and [[Old note#Later]]\n",
        ),
        ("Third.md", "links [[Somewhere else]]\n"),
    ]);

    let count = count_links_to_inner(&state, &path_text(&state, "Old note.md")).expect("count");

    assert_eq!(count, 2, "two notes link here, across three links");
}

#[test]
fn a_note_nothing_links_to_counts_nothing() {
    let (_dir, state) = seeded(&[("Old note.md", "the note itself\n")]);

    let count = count_links_to_inner(&state, &path_text(&state, "Old note.md")).expect("count");

    assert_eq!(count, 0);
}

#[test]
fn a_rename_rewrites_the_notes_that_link_to_it() {
    let (_dir, state) = seeded(&[
        ("Old note.md", "the note itself\n"),
        ("First.md", "see [[Old note|the one]]\n"),
        ("Second.md", "see [a](Old%20note.md#later)\n"),
    ]);

    let outcome =
        rename_note_with_links_inner(&state, &path_text(&state, "Old note.md"), "New note", true)
            .expect("rename");

    assert_eq!(outcome.renamed_path, path_text(&state, "New note.md"));
    assert_eq!(outcome.updated, 2);
    assert!(outcome.skipped.is_empty());
    assert_eq!(read(&state, "First.md"), "see [[New note|the one]]\n");
    assert_eq!(read(&state, "Second.md"), "see [a](New%20note.md#later)\n");
}

#[test]
fn a_rename_that_was_not_asked_to_update_links_leaves_them_alone() {
    let (_dir, state) = seeded(&[
        ("Old note.md", "the note itself\n"),
        ("First.md", "see [[Old note]]\n"),
    ]);

    let outcome =
        rename_note_with_links_inner(&state, &path_text(&state, "Old note.md"), "New note", false)
            .expect("rename");

    assert_eq!(outcome.updated, 0);
    assert!(outcome.updated_paths.is_empty());
    assert!(outcome.skipped.is_empty());
    assert_eq!(read(&state, "First.md"), "see [[Old note]]\n");
    assert!(note(&state, "New note.md").exists());
}

#[test]
fn the_index_follows_the_rename_so_the_links_can_be_put_back() {
    let (_dir, state) = seeded(&[
        ("Old note.md", "the note itself\n"),
        ("First.md", "see [[Old note]]\n"),
    ]);

    rename_note_with_links_inner(&state, &path_text(&state, "Old note.md"), "New note", true)
        .expect("rename");

    let count = count_links_to_inner(&state, &path_text(&state, "New note.md")).expect("count");
    assert_eq!(count, 1, "the index still names the note by its old path");
}

#[test]
fn a_file_that_cannot_be_read_is_named_rather_than_rewritten() {
    let (_dir, state) = seeded(&[
        ("Old note.md", "the note itself\n"),
        ("First.md", "see [[Old note]]\n"),
    ]);
    // The file goes between the index naming it and the rewrite reaching it.
    // A file that is not downloaded is refused the same way, one layer down,
    // where the eviction flag can be simulated on every platform:
    // `writ-storage`'s `note_link_rewrite_tests`.
    std::fs::remove_file(note(&state, "First.md")).expect("remove");

    let outcome =
        rename_note_with_links_inner(&state, &path_text(&state, "Old note.md"), "New note", true)
            .expect("rename");

    assert_eq!(outcome.updated, 0);
    assert_eq!(outcome.skipped.len(), 1);
    assert_eq!(outcome.skipped[0].path, path_text(&state, "First.md"));
    assert_eq!(outcome.skipped[0].reason, "ERR_FILE_MISSING");
}

#[cfg(unix)]
#[test]
fn a_read_only_note_is_named_rather_than_rewritten() {
    use std::os::unix::fs::PermissionsExt;

    let (_dir, state) = seeded(&[
        ("Old note.md", "the note itself\n"),
        ("First.md", "see [[Old note]]\n"),
    ]);
    let first = note(&state, "First.md");
    std::fs::set_permissions(&first, std::fs::Permissions::from_mode(0o444)).expect("chmod");

    let outcome =
        rename_note_with_links_inner(&state, &path_text(&state, "Old note.md"), "New note", true)
            .expect("rename");

    std::fs::set_permissions(&first, std::fs::Permissions::from_mode(0o644)).expect("chmod");
    assert_eq!(outcome.updated, 0);
    assert_eq!(outcome.skipped.len(), 1);
    assert_eq!(outcome.skipped[0].reason, ERR_READ_ONLY_DESTINATION);
    assert_eq!(read(&state, "First.md"), "see [[Old note]]\n");
    assert!(
        note(&state, "New note.md").exists(),
        "a file that could not be rewritten put the rename back"
    );
}

#[test]
fn an_ambiguous_link_is_left_alone() {
    let (_dir, state) = seeded(&[
        ("one/Note.md", "one of two\n"),
        ("two/Note.md", "two of two\n"),
        ("Reader.md", "see [[Note]]\n"),
    ]);

    let outcome =
        rename_note_with_links_inner(&state, &path_text(&state, "one/Note.md"), "Renamed", true)
            .expect("rename");

    assert_eq!(outcome.updated, 0);
    assert!(outcome.skipped.is_empty());
    assert_eq!(
        read(&state, "Reader.md"),
        "see [[Note]]\n",
        "a link naming two notes was rewritten to one of them"
    );
}

/// The mixed file: one link written with the folder, one bare link that
/// reaches the *other* note of that name. The file is rewritten for the first
/// and the second is left where it points.
#[test]
fn a_bare_link_reaching_another_note_of_the_same_name_is_left_alone() {
    let (_dir, state) = seeded(&[
        ("one/Note.md", "one of two\n"),
        ("two/Note.md", "two of two\n"),
        ("two/Reader.md", "see [[one/Note]] and [[Note]]\n"),
    ]);

    let outcome =
        rename_note_with_links_inner(&state, &path_text(&state, "one/Note.md"), "Renamed", true)
            .expect("rename");

    assert_eq!(outcome.updated, 1);
    assert!(outcome.skipped.is_empty());
    assert_eq!(
        read(&state, "two/Reader.md"),
        "see [[one/Renamed]] and [[Note]]\n",
        "a link reaching the other note of this name was repointed"
    );
    assert_eq!(read(&state, "two/Note.md"), "two of two\n");
}

/// The renamed note is never in the list of notes left unchanged: that list
/// names the other notes, and a note cannot be left holding a link to itself
/// under a name it no longer has.
#[test]
fn the_renamed_note_is_not_named_among_the_notes_left_unchanged() {
    let (_dir, state) = seeded(&[("Old note.md", "this is [[Old note]] itself\n")]);
    // The index says the note links to itself; the file no longer does.
    std::fs::write(note(&state, "Old note.md"), "nothing links anywhere\n").expect("write");

    let outcome =
        rename_note_with_links_inner(&state, &path_text(&state, "Old note.md"), "New note", false)
            .expect("rename");

    assert_eq!(outcome.updated, 0);
    assert!(
        outcome.skipped.is_empty(),
        "the renamed note named itself as left unchanged: {:?}",
        outcome.skipped
    );
}

/// A second note of the same name that the index will not hold, because it is
/// over the size the index takes. It is still on disk, so a bare link might
/// mean it, and the file holding that link is left as it is and named.
#[test]
fn a_note_too_big_for_the_index_still_makes_a_bare_link_ambiguous() {
    let (_dir, state) = seeded(&[
        ("one/Note.md", "one of two\n"),
        ("two/Reader.md", "see [[one/Note]] and [[Note]]\n"),
    ]);
    std::fs::write(note(&state, "two/Note.md"), "x".repeat(5 * 1024 * 1024 + 1)).expect("write");
    reindex(&state);

    let outcome =
        rename_note_with_links_inner(&state, &path_text(&state, "one/Note.md"), "Renamed", true)
            .expect("rename");

    assert_eq!(outcome.updated, 0);
    assert_eq!(outcome.skipped.len(), 1);
    assert_eq!(outcome.skipped[0].path, path_text(&state, "two/Reader.md"));
    assert_eq!(outcome.skipped[0].reason, ERR_LINK_NAME_NOT_UNIQUE);
    assert_eq!(
        read(&state, "two/Reader.md"),
        "see [[one/Note]] and [[Note]]\n",
        "a link that might mean a note the index never saw was rewritten"
    );
}

/// The same, for a note under a folder name the index walks past. The rename
/// reads the folder, not only the table.
#[test]
fn a_note_under_a_folder_the_index_skips_still_makes_a_bare_link_ambiguous() {
    let (_dir, state) = seeded(&[
        ("one/Note.md", "one of two\n"),
        (".trash/Note.md", "kept out of the index\n"),
        ("two/Reader.md", "see [[one/Note]] and [[Note]]\n"),
    ]);

    let outcome =
        rename_note_with_links_inner(&state, &path_text(&state, "one/Note.md"), "Renamed", true)
            .expect("rename");

    assert_eq!(outcome.updated, 0);
    assert_eq!(outcome.skipped.len(), 1);
    assert_eq!(outcome.skipped[0].reason, ERR_LINK_NAME_NOT_UNIQUE);
    assert_eq!(
        read(&state, "two/Reader.md"),
        "see [[one/Note]] and [[Note]]\n"
    );
}

/// The reindex after a rename can fail, and the undo still has to work: it
/// knows the files the rename rewrote and where the note is, and asks the
/// index for nothing it cannot do without.
#[test]
fn undo_puts_the_links_back_with_the_note_gone_from_the_index() {
    let before_first = "see [[Old note]]\n";
    let (_dir, state) = seeded(&[
        ("Old note.md", "the note itself\n"),
        ("First.md", before_first),
    ]);

    let renamed =
        rename_note_with_links_inner(&state, &path_text(&state, "Old note.md"), "New note", true)
            .expect("rename");
    assert_eq!(renamed.updated, 1);

    // What a failed `index_path` after the rename leaves behind.
    state
        .notes_index
        .forget_path(std::path::Path::new(&renamed.renamed_path))
        .expect("forget");

    let undone = undo_rename_with_links_inner(
        &state,
        &renamed.renamed_path,
        "Old note",
        &renamed.updated_paths,
    )
    .expect("undo");

    assert_eq!(undone.updated, 1);
    assert!(undone.skipped.is_empty());
    assert_eq!(read(&state, "First.md"), before_first);
    assert!(note(&state, "Old note.md").exists());
}

/// A file the index named that holds no link to rewrite: nothing failed and
/// nothing was written, and it is named anyway, because from the outside it is
/// a file left holding its old links.
#[test]
fn a_file_the_index_named_that_holds_no_link_is_reported() {
    let (_dir, state) = seeded(&[
        ("Old note.md", "the note itself\n"),
        ("First.md", "see [[Old note]]\n"),
    ]);
    std::fs::write(note(&state, "First.md"), "somebody took the link out\n").expect("write");

    let outcome =
        rename_note_with_links_inner(&state, &path_text(&state, "Old note.md"), "New note", true)
            .expect("rename");

    assert_eq!(outcome.updated, 0);
    assert_eq!(outcome.skipped.len(), 1);
    assert_eq!(outcome.skipped[0].path, path_text(&state, "First.md"));
    assert_eq!(outcome.skipped[0].reason, ERR_LINK_NOT_FOUND);
}

#[test]
fn a_link_carrying_another_folder_is_left_alone() {
    let (_dir, state) = seeded(&[
        ("ideas/Note.md", "the note itself\n"),
        ("Reader.md", "see [[archive/Note]] and [[ideas/Note]]\n"),
    ]);

    let outcome =
        rename_note_with_links_inner(&state, &path_text(&state, "ideas/Note.md"), "Renamed", true)
            .expect("rename");

    assert_eq!(outcome.updated, 1);
    assert_eq!(
        read(&state, "Reader.md"),
        "see [[archive/Note]] and [[ideas/Renamed]]\n"
    );
}

#[test]
fn undo_puts_the_name_and_every_rewritten_link_back() {
    let before_first = "see [[Old note|the one]]\n";
    let before_second = "see [a](Old%20note.md#later)\n";
    let (_dir, state) = seeded(&[
        ("Old note.md", "the note itself\n"),
        ("First.md", before_first),
        ("Second.md", before_second),
    ]);

    let renamed =
        rename_note_with_links_inner(&state, &path_text(&state, "Old note.md"), "New note", true)
            .expect("rename");

    let undone = undo_rename_with_links_inner(
        &state,
        &renamed.renamed_path,
        "Old note",
        &renamed.updated_paths,
    )
    .expect("undo");

    assert_eq!(undone.renamed_path, path_text(&state, "Old note.md"));
    assert_eq!(undone.updated, 2);
    assert!(undone.skipped.is_empty());
    assert_eq!(read(&state, "First.md"), before_first);
    assert_eq!(read(&state, "Second.md"), before_second);
    assert!(!note(&state, "New note.md").exists());
}

#[test]
fn undo_names_a_file_that_changed_since_the_rename() {
    let (_dir, state) = seeded(&[
        ("Old note.md", "the note itself\n"),
        ("First.md", "see [[Old note]]\n"),
    ]);
    let renamed =
        rename_note_with_links_inner(&state, &path_text(&state, "Old note.md"), "New note", true)
            .expect("rename");
    std::fs::remove_file(note(&state, "First.md")).expect("remove");

    let undone = undo_rename_with_links_inner(
        &state,
        &renamed.renamed_path,
        "Old note",
        &renamed.updated_paths,
    )
    .expect("undo");

    assert_eq!(undone.updated, 0);
    assert_eq!(undone.skipped.len(), 1);
    assert_eq!(undone.skipped[0].reason, "ERR_FILE_MISSING");
    assert!(
        note(&state, "Old note.md").exists(),
        "the file kept the name the undo was meant to take off it"
    );
}

#[test]
fn a_note_that_links_to_itself_follows_its_own_rename() {
    let (_dir, state) = seeded(&[("Old note.md", "this is [[Old note]] itself\n")]);

    let count = count_links_to_inner(&state, &path_text(&state, "Old note.md")).expect("count");
    let outcome =
        rename_note_with_links_inner(&state, &path_text(&state, "Old note.md"), "New note", true)
            .expect("rename");

    assert_eq!(count, 0, "the note itself is not another note linking here");
    assert_eq!(outcome.updated, 1);
    assert_eq!(read(&state, "New note.md"), "this is [[New note]] itself\n");
}

/// Turning the offer down answers for the other notes. The renamed note's own
/// link to itself is not one of them, and is carried anyway.
#[test]
fn a_self_link_follows_the_rename_even_when_the_offer_is_turned_down() {
    let (_dir, state) = seeded(&[
        ("Old note.md", "this is [[Old note]] itself\n"),
        ("First.md", "see [[Old note]]\n"),
    ]);

    let outcome =
        rename_note_with_links_inner(&state, &path_text(&state, "Old note.md"), "New note", false)
            .expect("rename");

    assert_eq!(outcome.updated, 1);
    assert_eq!(read(&state, "New note.md"), "this is [[New note]] itself\n");
    assert_eq!(
        read(&state, "First.md"),
        "see [[Old note]]\n",
        "the other note was left as the person asked"
    );
}

/// The codes the editor writes its sentences from, kept honest: a reason is a
/// bare code, never a code followed by an operating system's message.
#[test]
fn every_reason_is_a_bare_failure_code() {
    let (_dir, state) = seeded(&[
        ("Old note.md", "the note itself\n"),
        ("First.md", "see [[Old note]]\n"),
    ]);
    std::fs::remove_file(note(&state, "First.md")).expect("remove");

    let outcome =
        rename_note_with_links_inner(&state, &path_text(&state, "Old note.md"), "New note", true)
            .expect("rename");

    for skipped in &outcome.skipped {
        assert!(
            skipped.reason.starts_with("ERR_") && !skipped.reason.contains(' '),
            "{} is not a bare code",
            skipped.reason
        );
    }
}

#[test]
fn a_note_something_else_rewrote_is_named_rather_than_rewritten() {
    let (_dir, state) = seeded(&[("Old note.md", "the note itself\n")]);
    let linking = new_note_inner(&state).expect("new note");
    save_buffer_content_inner(&state, &linking.id, "see [[Old note]]\n").expect("save");
    let path = {
        let store = state.store.lock().expect("lock");
        store
            .get(&linking.id)
            .expect("row")
            .source_path
            .expect("file")
    };
    reindex(&state);
    std::fs::write(&path, "somebody else wrote this, and [[Old note]]\n").expect("outside write");

    let outcome =
        rename_note_with_links_inner(&state, &path_text(&state, "Old note.md"), "New note", true)
            .expect("rename");

    assert_eq!(outcome.updated, 0);
    assert_eq!(outcome.skipped.len(), 1);
    assert_eq!(outcome.skipped[0].path, path);
    assert_eq!(outcome.skipped[0].reason, ERR_FILE_CHANGED_ON_DISK);
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        "somebody else wrote this, and [[Old note]]\n",
        "the rewrite carried text Writ had never seen"
    );
}
