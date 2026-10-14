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
    count_links_to_inner, new_note_named_inner, rename_note_with_links_inner,
    undo_rename_with_links_inner, ERR_LINK_NAME_NOT_UNIQUE, ERR_LINK_NOT_FOUND,
    NOT_IN_FOLDER_TO_RENAME, NO_RENAME_TO_UNDO,
};
use writ_tauri_lib::preview::handler::RenderCache;
use writ_tauri_lib::quit::QuitState;
use writ_tauri_lib::security::{AuthorizedPaths, RenameUndo};
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

    AppState {
        store: Mutex::new(BufferStore::new(conn, buffers_dir.clone())),
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

/// A notes folder holding `notes`, indexed once, with the app state over it.
fn seeded(notes: &[(&str, &str)]) -> (TempDir, AppState) {
    let dir = TempDir::new().expect("temp dir");
    let state = make_state(&dir);
    for (name, body) in notes {
        let path = note(&state, name);
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
    name.split('/')
        .fold(state.notes_root().to_path_buf(), |path, part| {
            path.join(part)
        })
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

/// The other side of that: the rename reads the folder the way the index
/// reads it. A file in a folder the index prunes is not a note a link can
/// reach, and a file that is not note text never makes a name ambiguous — a
/// notes folder kept in git holds a `.git/index`, and a note called
/// `index.md` is ordinary.
#[test]
fn a_file_the_index_would_never_hold_does_not_make_a_bare_link_ambiguous() {
    let (_dir, state) = seeded(&[
        ("one/index.md", "one of one\n"),
        (".obsidian/index.md", "a key map\n"),
        ("two/Reader.md", "see [[one/index]] and [[index]]\n"),
    ]);
    std::fs::create_dir_all(note(&state, ".git")).expect("folder");
    std::fs::write(note(&state, ".git/index"), [0u8, 1, 2, 3, 4]).expect("write");

    let outcome =
        rename_note_with_links_inner(&state, &path_text(&state, "one/index.md"), "Renamed", true)
            .expect("rename");

    assert_eq!(outcome.updated, 1);
    assert!(outcome.skipped.is_empty(), "{:?}", outcome.skipped);
    assert_eq!(
        read(&state, "two/Reader.md"),
        "see [[one/Renamed]] and [[Renamed]]\n"
    );
}

/// The same folder read for kind rather than for where it is. A file that is
/// not note text is not a second note, in a folder nothing prunes: a notes
/// folder can hold a `bin/Note` and a note called `Note.md` at once.
#[test]
fn a_file_that_is_not_note_text_does_not_make_a_bare_link_ambiguous() {
    let (_dir, state) = seeded(&[
        ("one/Note.md", "the only note of that name\n"),
        ("two/Reader.md", "see [[Note]]\n"),
    ]);
    std::fs::create_dir_all(note(&state, "bin")).expect("folder");
    std::fs::write(note(&state, "bin/Note"), [0u8, 159, 146, 150, 0, 1]).expect("write");

    let outcome =
        rename_note_with_links_inner(&state, &path_text(&state, "one/Note.md"), "Renamed", true)
            .expect("rename");

    assert_eq!(outcome.updated, 1);
    assert!(outcome.skipped.is_empty(), "{:?}", outcome.skipped);
    assert_eq!(read(&state, "two/Reader.md"), "see [[Renamed]]\n");
}

/// A symlinked note is a note under the name the symlink carries. The index
/// walk does not follow it, so nothing else in the app resolves a link to it,
/// and a rename that repointed the link past it would be the one surface that
/// did.
#[cfg(unix)]
#[test]
fn a_note_reached_through_a_symlink_makes_a_bare_link_ambiguous() {
    let outside = TempDir::new().expect("temp dir");
    let real = outside.path().join("Note.md");
    std::fs::write(&real, "the other note\n").expect("write");
    let (_dir, state) = seeded(&[
        ("one/Note.md", "one of two\n"),
        ("two/Reader.md", "see [[one/Note]] and [[Note]]\n"),
    ]);
    std::os::unix::fs::symlink(&real, note(&state, "two/Note.md")).expect("symlink");

    let outcome =
        rename_note_with_links_inner(&state, &path_text(&state, "one/Note.md"), "Renamed", true)
            .expect("rename");

    assert_eq!(outcome.updated, 0);
    assert_eq!(outcome.skipped.len(), 1);
    assert_eq!(outcome.skipped[0].reason, ERR_LINK_NAME_NOT_UNIQUE);
    assert_eq!(
        outcome.skipped[0].other_path.as_deref(),
        Some(path_text(&state, "two/Note.md").as_str()),
        "the note the link could have meant is named where a person can find it"
    );
    assert_eq!(
        read(&state, "two/Reader.md"),
        "see [[one/Note]] and [[Note]]\n"
    );
}

/// A link writes a name, and the name a symlink answers to is its own. What it
/// points at decides which file it is, never which links reach it.
#[cfg(unix)]
#[test]
fn a_symlink_is_a_twin_under_the_name_it_carries() {
    let outside = TempDir::new().expect("temp dir");
    let real = outside.path().join("Elsewhere.md");
    std::fs::write(&real, "the other note\n").expect("write");
    let (_dir, state) = seeded(&[
        ("one/Note.md", "one of two\n"),
        ("two/Reader.md", "see [[Note]]\n"),
    ]);
    std::os::unix::fs::symlink(&real, note(&state, "two/Note.md")).expect("symlink");

    let outcome =
        rename_note_with_links_inner(&state, &path_text(&state, "one/Note.md"), "Renamed", true)
            .expect("rename");

    assert_eq!(outcome.updated, 0);
    assert_eq!(outcome.skipped.len(), 1);
    assert_eq!(outcome.skipped[0].reason, ERR_LINK_NAME_NOT_UNIQUE);
    assert_eq!(
        outcome.skipped[0].other_path.as_deref(),
        Some(path_text(&state, "two/Note.md").as_str())
    );
    assert_eq!(read(&state, "two/Reader.md"), "see [[Note]]\n");
}

/// A symlink pointing at the renamed note is another name for it, not a second
/// note, so it settles nothing and the rename goes through.
#[cfg(unix)]
#[test]
fn a_symlink_to_the_renamed_note_is_not_a_second_note() {
    let (_dir, state) = seeded(&[
        ("one/Note.md", "the note itself\n"),
        ("two/Reader.md", "see [[one/Note]] and [[Note]]\n"),
    ]);
    std::os::unix::fs::symlink(note(&state, "one/Note.md"), note(&state, "two/Note.md"))
        .expect("symlink");

    let outcome =
        rename_note_with_links_inner(&state, &path_text(&state, "one/Note.md"), "Renamed", true)
            .expect("rename");

    assert_eq!(outcome.updated, 1);
    assert!(outcome.skipped.is_empty(), "{:?}", outcome.skipped);
    assert_eq!(
        read(&state, "two/Reader.md"),
        "see [[one/Renamed]] and [[Renamed]]\n"
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
        renamed.undo_token.as_deref().expect("an undo token"),
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
        renamed.undo_token.as_deref().expect("an undo token"),
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
        renamed.undo_token.as_deref().expect("an undo token"),
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
    // Markdown, because a plain-text file holds no links (ADR-041 section 2).
    let linking = new_note_named_inner(&state, "Linking.md").expect("new note");
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

/// Where the walk below stands with the note it reached first.
#[derive(Debug)]
enum RenameDuringWalk {
    /// Neither note reached yet.
    Waiting,
    /// The walk reached this note, and it was renamed with its links.
    Renamed(
        std::path::PathBuf,
        writ_tauri_lib::commands::notes::RenamePropagationDto,
    ),
    /// The walk moved on, and the rename was undone.
    Undone(std::path::PathBuf),
}

/// A rename and its undo made while a walk is under way neither stop the walk
/// nor lose the note. The walk reads every note it had not read yet, and the
/// note the undo put back is in the index when the walk ends, under its own
/// name, with the link to it resolved.
///
/// The store is wired the way `AppState::initialize` wires it, and the walk
/// polls what the app's walk polls: shutdown, and whether the notes folder
/// moved since it started. The renamed note is whichever of two the walk
/// reaches first, so the walk always reaches the other one after it.
#[test]
fn a_rename_and_its_undo_during_a_walk_leave_the_walk_running_and_the_note_indexed() {
    use std::cell::RefCell;
    use std::sync::atomic::Ordering;
    use writ_core::notes::links::Resolution;
    use writ_storage::notes_index::index_key;

    let (_dir, state) = seeded(&[
        ("Home.md", "see [[Plan]] and [[Kestrel]]\n"),
        ("a/Plan.md", "plan body\n"),
        ("b/Kestrel.md", "kestrel body\n"),
    ]);
    {
        let mut store = state.store.lock().expect("store");
        store.set_notes_root(state.notes_root());
        store.set_notes_index(Arc::clone(&state.notes_index));
    }
    let late: Vec<std::path::PathBuf> = (0..12)
        .map(|idx| {
            let path = note(&state, &format!("c/late-{idx:02}.md"));
            std::fs::create_dir_all(path.parent().expect("parent")).expect("folder");
            std::fs::write(&path, "written after the last walk\n").expect("write");
            path
        })
        .collect();
    let candidates = [note(&state, "a/Plan.md"), note(&state, "b/Kestrel.md")];

    let index = Arc::clone(&state.notes_index);
    let cancel = Arc::clone(&state.notes_index_cancel);
    let generation = index.generation();
    let cancelled = || cancel.load(Ordering::Relaxed) || index.generation() != generation;
    let step = RefCell::new(RenameDuringWalk::Waiting);
    let hook = |path: &std::path::Path| {
        let mut step = step.borrow_mut();
        match &*step {
            RenameDuringWalk::Waiting if candidates.iter().any(|note| note == path) => {
                let stem = path.file_stem().expect("stem").to_string_lossy();
                let renamed = rename_note_with_links_inner(
                    &state,
                    &path.to_string_lossy(),
                    &format!("{stem} 2"),
                    true,
                )
                .expect("rename during the walk");
                *step = RenameDuringWalk::Renamed(path.to_path_buf(), renamed);
            }
            RenameDuringWalk::Renamed(original, renamed) => {
                let original = original.clone();
                undo_rename_with_links_inner(
                    &state,
                    renamed.undo_token.as_deref().expect("an undo token"),
                )
                .expect("undo during the walk");
                *step = RenameDuringWalk::Undone(original);
            }
            _ => {}
        }
        false
    };
    let outcome = index
        .reconcile(&state.notes_root(), &cancelled, &hook)
        .expect("walk");

    assert!(!outcome.cancelled, "a rename stopped the walk under way");
    let RenameDuringWalk::Undone(original) = step.into_inner() else {
        panic!("the walk did not reach a note after the renamed one");
    };
    let indexed: Vec<String> = index
        .snapshot()
        .expect("snapshot")
        .into_iter()
        .map(|(path, ..)| path)
        .collect();
    for path in &late {
        assert!(
            indexed.contains(&index_key(path)),
            "{} was never read",
            path.display()
        );
    }
    assert!(original.exists());
    assert!(
        indexed.contains(&index_key(&original)),
        "the note the undo put back is missing from the index"
    );
    let stem = original
        .file_stem()
        .expect("stem")
        .to_string_lossy()
        .into_owned();
    let renamed_key = index_key(&original.with_file_name(format!("{stem} 2.md")));
    assert!(
        !indexed.contains(&renamed_key),
        "the name the undo left is still indexed"
    );
    assert_eq!(
        index
            .resolve_link(&path_text(&state, "Home.md"), &stem)
            .expect("resolve"),
        Resolution::Resolved(index_key(&original))
    );
}

/// The path comes from the webview, so a file outside the notes folder is
/// refused before anything moves: a renamed `~/.zshrc` is the attack.
#[test]
fn a_rename_with_links_refuses_a_file_outside_the_notes_folder() {
    let (dir, state) = seeded(&[("First.md", "see [[outside]]\n")]);
    let outside = dir.path().join("outside.md");
    std::fs::write(&outside, "not a note\n").expect("write");

    let refused = rename_note_with_links_inner(&state, &outside.to_string_lossy(), "moved", true);

    assert_eq!(refused, Err(NOT_IN_FOLDER_TO_RENAME.to_string()));
    assert!(outside.exists(), "the file outside the folder was renamed");
    assert!(!dir.path().join("moved.md").exists());
    assert_eq!(read(&state, "First.md"), "see [[outside]]\n");
}

/// A link inside the folder that resolves outside it is the same file, and is
/// refused the same way.
#[cfg(unix)]
#[test]
fn a_rename_with_links_refuses_a_symlink_that_leaves_the_folder() {
    let (dir, state) = seeded(&[]);
    let outside = dir.path().join("target.md");
    std::fs::write(&outside, "not a note\n").expect("write");
    let link = note(&state, "Linked.md");
    std::os::unix::fs::symlink(&outside, &link).expect("symlink");

    let refused = rename_note_with_links_inner(&state, &link.to_string_lossy(), "moved", true);

    assert_eq!(refused, Err(NOT_IN_FOLDER_TO_RENAME.to_string()));
    assert!(link.exists());
    assert_eq!(
        std::fs::read_to_string(&outside).expect("read"),
        "not a note\n"
    );
}

/// The undo names no path: a token the rename did not hand out puts nothing
/// back, and the one it did is spent by the first undo.
#[test]
fn an_undo_runs_once_and_only_for_the_token_its_rename_handed_out() {
    let before_first = "see [[Old note]]\n";
    let (_dir, state) = seeded(&[
        ("Old note.md", "the note itself\n"),
        ("First.md", before_first),
    ]);
    let renamed =
        rename_note_with_links_inner(&state, &path_text(&state, "Old note.md"), "New note", true)
            .expect("rename");
    let token = renamed.undo_token.clone().expect("an undo token");

    assert_eq!(
        undo_rename_with_links_inner(&state, "forged"),
        Err(NO_RENAME_TO_UNDO.to_string())
    );
    assert!(
        note(&state, "New note.md").exists(),
        "a forged token undid the rename"
    );

    let undone = undo_rename_with_links_inner(&state, &token).expect("undo");
    assert_eq!(undone.undo_token, None);
    assert_eq!(read(&state, "First.md"), before_first);
    assert!(note(&state, "Old note.md").exists());

    assert_eq!(
        undo_rename_with_links_inner(&state, &token),
        Err(NO_RENAME_TO_UNDO.to_string())
    );
}

/// A rename that changed nothing has nothing to undo.
#[test]
fn a_rename_to_the_same_name_hands_out_no_undo() {
    let (_dir, state) = seeded(&[("Note.md", "the note itself\n")]);

    let outcome = rename_note_with_links_inner(&state, &path_text(&state, "Note.md"), "Note", true)
        .expect("rename");

    assert_eq!(outcome.undo_token, None);
}

/// What an undo holds was recorded inside the folder, and the folder can move
/// before the undo runs. A held rename that no longer sits in it is refused.
#[test]
fn an_undo_refuses_a_held_rename_the_folder_no_longer_holds() {
    let (dir, state) = seeded(&[]);
    let outside = dir.path().join("Elsewhere.md");
    std::fs::write(&outside, "kept\n").expect("write");
    let token = state
        .authorized_paths
        .hold_rename_undo(RenameUndo {
            renamed: outside.clone(),
            previous_name: "Old.md".to_string(),
            updated: Vec::new(),
        })
        .expect("a token");

    assert_eq!(
        undo_rename_with_links_inner(&state, &token),
        Err(NOT_IN_FOLDER_TO_RENAME.to_string())
    );
    assert!(outside.exists());
    assert!(!dir.path().join("Old.md").exists());
}

/// A rewritten file the folder no longer holds is left as it is, byte for
/// byte, while the note itself still takes its name back.
#[test]
fn an_undo_leaves_a_rewritten_file_the_folder_no_longer_holds() {
    let (dir, state) = seeded(&[("New note.md", "the note itself\n")]);
    let outside = dir.path().join("Outside.md");
    let before = "see [[New note]]\n";
    std::fs::write(&outside, before).expect("write");
    let token = state
        .authorized_paths
        .hold_rename_undo(RenameUndo {
            renamed: note(&state, "New note.md"),
            previous_name: "Old note.md".to_string(),
            updated: vec![outside.to_string_lossy().into_owned()],
        })
        .expect("a token");

    let undone = undo_rename_with_links_inner(&state, &token).expect("undo");

    assert_eq!(std::fs::read_to_string(&outside).expect("read"), before);
    assert_eq!(undone.updated, 0);
    assert!(note(&state, "Old note.md").exists());
}
