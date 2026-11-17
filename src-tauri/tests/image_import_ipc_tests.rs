//! Pasting and dropping images into a Markdown file (ADR-035), and the
//! remote images the editor fetches on request (ADR-057).
//!
//! The commands are exercised through their Tauri-free inner functions
//! against the app's state, with a fixed clipboard and clock; the last test
//! reads `lib.rs`, since a command that is not in the invoke handler cannot
//! be called.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use chrono::{NaiveDate, NaiveDateTime, Utc};
use tempfile::TempDir;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_core::config::{ImageFolder, WritConfig};
use writ_core::events::bus::EventBus;
use writ_core::notes::image_import::IMAGE_MAX_BYTES;
use writ_core::preview::remote_image::{HeldImage, RemoteImageRefusal};
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
    close_buffer_inner, close_buffers_inner, delete_buffer_inner,
};
use writ_tauri_lib::commands::images::{
    encode_png, import_dropped_images_inner, paste_clipboard_images_inner, ClipboardContents,
    ClipboardPasteResult, ClipboardPicture, ClipboardReader, ERR_TAB_NOT_OPEN,
};
use writ_tauri_lib::commands::preview::{
    preview_remote_image_lookup_inner, preview_remote_image_store_inner, InlineImageResult,
    RemoteImageLookup,
};
use writ_tauri_lib::preview::handler::{resolve_cached, RenderCache};
use writ_tauri_lib::quit::QuitState;
use writ_tauri_lib::security::AuthorizedPaths;
use writ_tauri_lib::state::AppState;
use writ_tauri_lib::watcher::handler::create_ignore_set;

const LIB_RS: &str = include_str!("../src/lib.rs");

const COMMANDS: &[&str] = &[
    "commands::images::paste_clipboard_images",
    "commands::images::import_dropped_images",
    "commands::preview::preview_remote_image",
];

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDRpixels";
const PIXELS: [u8; 8] = [255, 0, 0, 255, 0, 255, 0, 128];

/// A 2x1 picture as the clipboard holds one.
fn picture() -> ClipboardContents {
    ClipboardContents::Image(ClipboardPicture::new(2, 1, PIXELS.to_vec()).unwrap())
}
const OTHER_PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDRother";

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
        pending_lines: Mutex::new(Default::default()),
        cli_waits: Mutex::new(Default::default()),
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
        buffer_writes: Default::default(),
    }
}

/// A state holding one open tab `id`, at `source_path` when given.
fn state_with(dir: &TempDir, id: &str, source_path: Option<&Path>) -> AppState {
    let state = make_state(dir);
    let now = Utc::now();
    state
        .store
        .lock()
        .expect("store")
        .insert(&BufferDocument {
            id: id.to_string(),
            title: "Trip plan".to_string(),
            filename: "Trip plan.md".to_string(),
            status: BufferStatus::Active,
            language: None,
            source_path: source_path.map(|path| path.to_string_lossy().into_owned()),
            cursor_pos: 0,
            scroll_pos: 0,
            tab_order: 0,
            created_at: now,
            updated_at: now,
            closed_at: None,
            read_only: false,
            size_bytes: 0,
            line_ending: writ_core::notes::line_ending::LineEnding::Lf,
            encoding: None,
        })
        .expect("insert");
    state
}

struct FixedClipboard(ClipboardContents);

impl ClipboardReader for FixedClipboard {
    fn read_contents(&self) -> ClipboardContents {
        self.0.clone()
    }
}

fn at() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 11, 17)
        .unwrap()
        .and_hms_opt(9, 5, 7)
        .unwrap()
}

/// A note at `<notes>/trips/Trip plan.md`, and a folder outside the notes
/// folder holding `shot.png`.
struct Fixture {
    dir: TempDir,
    note_dir: PathBuf,
    note: PathBuf,
    outside: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().expect("temp dir");
        let note_dir = dir.path().join("Writ").join("trips");
        std::fs::create_dir_all(&note_dir).unwrap();
        let note = note_dir.join("Trip plan.md");
        std::fs::write(&note, "# Trip\n").unwrap();
        let outside = dir.path().join("Desktop");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("shot.png"), PNG).unwrap();
        Self {
            dir,
            note_dir,
            note,
            outside,
        }
    }

    fn state(&self) -> AppState {
        state_with(&self.dir, "buf-1", Some(&self.note))
    }

    fn paste(&self, state: &AppState, contents: ClipboardContents) -> ClipboardPasteResult {
        paste_clipboard_images_inner(state, "buf-1", &FixedClipboard(contents), at())
            .expect("answer")
    }
}

fn inserted(result: ClipboardPasteResult) -> (String, Vec<String>) {
    match result {
        ClipboardPasteResult::Inserted { markdown, notices } => (markdown, notices),
        other => panic!("expected an insert, got {other:?}"),
    }
}

fn refused(result: ClipboardPasteResult) -> Vec<String> {
    match result {
        ClipboardPasteResult::Refused { notices } => notices,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn text_and_an_empty_clipboard_paste_as_today() {
    let f = Fixture::new();
    let state = f.state();
    assert_eq!(
        f.paste(&state, ClipboardContents::Text),
        ClipboardPasteResult::Text
    );
    assert_eq!(
        f.paste(&state, ClipboardContents::Empty),
        ClipboardPasteResult::Empty
    );
}

#[test]
fn copied_files_that_are_not_images_paste_as_their_names() {
    let f = Fixture::new();
    let doc = f.outside.join("doc.txt");
    std::fs::write(&doc, "words").unwrap();
    let state = f.state();
    assert_eq!(
        f.paste(&state, ClipboardContents::Files(vec![doc])),
        ClipboardPasteResult::Text
    );
}

#[test]
fn a_copied_image_outside_the_note_folder_is_copied_into_assets() {
    let f = Fixture::new();
    let state = f.state();
    let stamps_before = state.watcher_ignore.lock().unwrap().len();
    let (markdown, notices) = inserted(f.paste(
        &state,
        ClipboardContents::Files(vec![f.outside.join("shot.png")]),
    ));
    assert_eq!(markdown, "![](assets/shot.png)");
    assert!(notices.is_empty());
    assert_eq!(
        std::fs::read(f.note_dir.join("assets/shot.png")).unwrap(),
        PNG
    );
    assert!(state.watcher_ignore.lock().unwrap().len() > stamps_before);
}

#[test]
fn several_files_give_one_reference_per_line_and_skip_what_is_not_an_image() {
    let f = Fixture::new();
    std::fs::write(f.outside.join("b.png"), OTHER_PNG).unwrap();
    let page = f.outside.join("page.png");
    std::fs::write(&page, "<html></html>").unwrap();
    let state = f.state();
    let (markdown, notices) = inserted(f.paste(
        &state,
        ClipboardContents::Files(vec![
            f.outside.join("shot.png"),
            page,
            f.outside.join("b.png"),
        ]),
    ));
    assert_eq!(markdown, "![](assets/shot.png)\n![](assets/b.png)");
    assert_eq!(
        notices,
        vec!["1 file is not an image and was not pasted".to_string()]
    );
}

#[test]
fn an_image_inside_the_note_folder_is_referred_to_in_place() {
    let f = Fixture::new();
    let photos = f.note_dir.join("photos");
    std::fs::create_dir_all(&photos).unwrap();
    std::fs::write(photos.join("day one.png"), PNG).unwrap();
    let state = f.state();
    let (markdown, _) = inserted(f.paste(
        &state,
        ClipboardContents::Files(vec![photos.join("day one.png")]),
    ));
    assert_eq!(markdown, "![](photos/day%20one.png)");
    assert!(!f.note_dir.join("assets").exists());
}

#[test]
fn the_same_image_pasted_twice_reuses_its_file_and_a_different_one_is_numbered() {
    let f = Fixture::new();
    let state = f.state();
    let files = ClipboardContents::Files(vec![f.outside.join("shot.png")]);
    inserted(f.paste(&state, files.clone()));
    let (again, _) = inserted(f.paste(&state, files.clone()));
    assert_eq!(again, "![](assets/shot.png)");
    std::fs::write(f.outside.join("shot.png"), OTHER_PNG).unwrap();
    let (other, _) = inserted(f.paste(&state, files));
    assert_eq!(other, "![](assets/shot-2.png)");
}

#[test]
fn a_clipboard_picture_is_saved_as_png_named_after_the_note_and_the_time() {
    let f = Fixture::new();
    let state = f.state();
    let (markdown, _) = inserted(f.paste(&state, picture()));
    assert_eq!(markdown, "![](assets/Trip-plan-20261117-090507.png)");
    assert_eq!(
        std::fs::read(f.note_dir.join("assets/Trip-plan-20261117-090507.png")).unwrap(),
        encode_png(2, 1, &PIXELS).unwrap()
    );
}

#[test]
fn a_clipboard_picture_with_every_name_taken_is_named_in_the_notice() {
    let f = Fixture::new();
    let assets = f.note_dir.join("assets");
    std::fs::create_dir_all(&assets).unwrap();
    for number in 1..=999 {
        let name = if number == 1 {
            "Trip-plan-20261117-090507.png".to_string()
        } else {
            format!("Trip-plan-20261117-090507-{number}.png")
        };
        std::fs::write(assets.join(name), OTHER_PNG).unwrap();
    }
    let state = f.state();
    let notices = refused(f.paste(&state, picture()));
    assert_eq!(
        notices,
        vec![
            "Could not find a free name for Trip-plan-20261117-090507.png in the folder assets"
                .to_string()
        ]
    );
}

/// Noise with uneven alpha: no JPEG fallback, and a PNG over the cap.
fn oversized_transparent_picture() -> ClipboardContents {
    let side = 2200;
    let mut seed: u32 = 0x9e37_79b9;
    let rgba = (0..side * side * 4)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed.to_le_bytes()[0]
        })
        .collect();
    ClipboardContents::Image(ClipboardPicture::new(side, side, rgba).unwrap())
}

#[test]
fn a_clipboard_picture_over_the_cap_is_refused_and_writes_nothing() {
    let f = Fixture::new();
    let state = f.state();
    let stamps_before = state.watcher_ignore.lock().unwrap().len();
    let notices = refused(f.paste(&state, oversized_transparent_picture()));
    assert_eq!(
        notices,
        vec![format!(
            "The copied picture is over the {} MB limit for images",
            IMAGE_MAX_BYTES / (1024 * 1024)
        )]
    );
    let entries: Vec<_> = std::fs::read_dir(&f.note_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(entries, vec![std::ffi::OsString::from("Trip plan.md")]);
    assert_eq!(state.watcher_ignore.lock().unwrap().len(), stamps_before);
}

#[cfg(unix)]
#[test]
fn an_image_folder_linked_outside_the_file_folder_is_refused() {
    let f = Fixture::new();
    let elsewhere = f.dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, f.note_dir.join("assets")).unwrap();
    let state = f.state();
    let notices = refused(f.paste(
        &state,
        ClipboardContents::Files(vec![f.outside.join("shot.png")]),
    ));
    assert_eq!(
        notices,
        vec!["The folder assets leads outside the file's folder".to_string()]
    );
    assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn a_write_that_does_not_land_leaves_no_mark_for_the_watcher() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let assets = f.note_dir.join("assets");
    std::fs::create_dir_all(&assets).unwrap();
    std::fs::set_permissions(&assets, std::fs::Permissions::from_mode(0o555)).unwrap();
    // Root writes past the permission bits this test relies on.
    if std::fs::write(assets.join("probe"), b"").is_ok() {
        return;
    }
    let state = f.state();
    let stamps_before = state.watcher_ignore.lock().unwrap().len();
    let result = f.paste(&state, picture());
    std::fs::set_permissions(&assets, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        refused(result),
        vec!["Could not write to the folder assets".to_string()]
    );
    assert_eq!(state.watcher_ignore.lock().unwrap().len(), stamps_before);
}

#[test]
fn an_empty_image_folder_saves_beside_the_note() {
    let f = Fixture::new();
    let state = f.state();
    state.config.lock().unwrap().files.image_folder = ImageFolder::parse("").unwrap();
    let (markdown, _) = inserted(f.paste(
        &state,
        ClipboardContents::Files(vec![f.outside.join("shot.png")]),
    ));
    assert_eq!(markdown, "![](shot.png)");
    assert!(f.note_dir.join("shot.png").exists());
}

#[test]
fn a_tab_with_no_file_on_disk_is_told_to_save_first() {
    let f = Fixture::new();
    let state = state_with(&f.dir, "buf-1", None);
    let notices = refused(f.paste(&state, picture()));
    assert_eq!(
        notices,
        vec!["Save the file first, so the image has a folder to go in".to_string()]
    );
}

#[test]
fn an_image_over_the_cap_is_turned_away_with_the_limit_named() {
    let f = Fixture::new();
    let big = f.outside.join("big.png");
    let file = std::fs::File::create(&big).unwrap();
    file.set_len(IMAGE_MAX_BYTES + 1).unwrap();
    drop(file);
    let mut bytes = std::fs::OpenOptions::new().write(true).open(&big).unwrap();
    std::io::Write::write_all(&mut bytes, PNG).unwrap();
    let state = f.state();
    let notices = refused(f.paste(&state, ClipboardContents::Files(vec![big])));
    assert_eq!(
        notices,
        vec!["big.png is over the 16 MB limit for images".to_string()]
    );
}

#[test]
fn a_folder_that_cannot_be_written_is_named() {
    let f = Fixture::new();
    std::fs::write(f.note_dir.join("assets"), "a file").unwrap();
    let state = f.state();
    let notices = refused(f.paste(
        &state,
        ClipboardContents::Files(vec![f.outside.join("shot.png")]),
    ));
    assert_eq!(
        notices,
        vec!["Could not write to the folder assets".to_string()]
    );
}

#[test]
fn a_tab_that_is_not_open_is_an_error() {
    let f = Fixture::new();
    let state = f.state();
    let err = paste_clipboard_images_inner(&state, "buf-9", &FixedClipboard(picture()), at())
        .unwrap_err();
    assert_eq!(err, ERR_TAB_NOT_OPEN);
}

fn authorize(state: &AppState, path: &Path) -> String {
    let canonical = std::fs::canonicalize(path)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    state.authorized_paths.record_for_open(canonical.clone());
    canonical
}

#[test]
fn a_drop_takes_the_images_and_leaves_the_rest_to_open() {
    let f = Fixture::new();
    let doc = f.outside.join("doc.md");
    std::fs::write(&doc, "words").unwrap();
    let state = f.state();
    let image = authorize(&state, &f.outside.join("shot.png"));
    let other = authorize(&state, &doc);
    let result =
        import_dropped_images_inner(&state, "buf-1", &[image.clone(), other.clone()]).unwrap();
    assert_eq!(result.markdown, "![](assets/shot.png)");
    assert_eq!(result.others, vec![other.clone()]);
    assert!(result.notices.is_empty());
    assert!(!state.authorized_paths.is_pending_open(&image));
    assert!(state.authorized_paths.is_pending_open(&other));
}

#[test]
fn a_dropped_path_the_drop_did_not_authorize_is_never_read() {
    let f = Fixture::new();
    let state = f.state();
    let path = f.outside.join("shot.png").to_string_lossy().into_owned();
    let result = import_dropped_images_inner(&state, "buf-1", std::slice::from_ref(&path)).unwrap();
    assert_eq!(result.markdown, "");
    assert_eq!(result.others, vec![path]);
    assert!(!f.note_dir.join("assets").exists());
}

#[test]
fn images_dropped_on_a_tab_with_no_file_are_named_and_not_opened() {
    let f = Fixture::new();
    let state = state_with(&f.dir, "buf-1", None);
    let image = authorize(&state, &f.outside.join("shot.png"));
    let result =
        import_dropped_images_inner(&state, "buf-1", std::slice::from_ref(&image)).unwrap();
    assert_eq!(result.markdown, "");
    assert!(result.others.is_empty());
    assert_eq!(
        result.notices,
        vec!["Save the file first, so the image has a folder to go in".to_string()]
    );
    assert!(!state.authorized_paths.is_pending_open(&image));
}

fn ready_url(answer: InlineImageResult) -> String {
    match answer {
        InlineImageResult::Ready { url, .. } => url,
        InlineImageResult::Refused { reason } => panic!("refused: {reason}"),
    }
}

fn reason(answer: InlineImageResult) -> String {
    match answer {
        InlineImageResult::Refused { reason } => reason,
        InlineImageResult::Ready { url, .. } => panic!("expected a refusal, got {url}"),
    }
}

fn answered(lookup: RemoteImageLookup) -> InlineImageResult {
    match lookup {
        RemoteImageLookup::Answered(answer) => answer,
        RemoteImageLookup::Fetch(pending) => {
            panic!("expected an answer, got a fetch of {}", pending.url)
        }
    }
}

#[test]
fn a_remote_reference_is_fetched_once_and_then_served_from_memory() {
    let f = Fixture::new();
    let state = f.state();
    let lookup =
        preview_remote_image_lookup_inner(&state, "buf-1", "https://example.com/a.png").unwrap();
    let RemoteImageLookup::Fetch(pending) = lookup else {
        panic!("expected a fetch");
    };
    assert_eq!(pending.url.as_str(), "https://example.com/a.png");
    let url = ready_url(preview_remote_image_store_inner(
        &state,
        &pending,
        Ok(HeldImage {
            mime: "image/png",
            bytes: PNG.into(),
        }),
    ));
    assert!(url.contains("/_remote-image/buf-1/"), "{url}");
    let served = resolve_cached(
        &url.replace("http://writ-preview.", "writ-preview://"),
        &state.preview_render_cache,
        &|_: &Path| false,
    );
    assert_eq!(served.status, 200);
    assert_eq!(served.body.as_ref(), PNG);
    let again = answered(
        preview_remote_image_lookup_inner(&state, "buf-1", "https://example.com/a.png").unwrap(),
    );
    assert_eq!(ready_url(again), url);
}

#[test]
fn a_remote_reference_that_is_not_a_web_address_or_an_open_tabs_is_refused() {
    let f = Fixture::new();
    let state = f.state();
    for reference in ["assets/a.png", "file:///etc/passwd", "javascript:alert(1)"] {
        let answer =
            answered(preview_remote_image_lookup_inner(&state, "buf-1", reference).unwrap());
        assert_eq!(reason(answer), "not_remote", "{reference}");
    }
    let answer = answered(
        preview_remote_image_lookup_inner(&state, "buf-9", "https://example.com/a.png").unwrap(),
    );
    assert_eq!(reason(answer), "outside_root");
}

fn fetch_for(
    state: &AppState,
    buffer_id: &str,
) -> writ_tauri_lib::commands::preview::PendingRemoteImage {
    match preview_remote_image_lookup_inner(state, buffer_id, "https://example.com/a.png").unwrap()
    {
        RemoteImageLookup::Fetch(pending) => pending,
        RemoteImageLookup::Answered(answer) => panic!("expected a fetch, got {answer:?}"),
    }
}

fn png_image() -> HeldImage {
    HeldImage {
        mime: "image/png",
        bytes: PNG.into(),
    }
}

#[test]
fn closing_a_tab_releases_the_remote_images_it_was_granted() {
    for close in [
        (|state: &AppState| close_buffer_inner(state, "buf-1"))
            as fn(&AppState) -> Result<(), String>,
        |state| close_buffers_inner(state, &["buf-1".to_string()]),
        |state| delete_buffer_inner(state, "buf-1"),
    ] {
        let f = Fixture::new();
        let state = f.state();
        let pending = fetch_for(&state, "buf-1");
        ready_url(preview_remote_image_store_inner(
            &state,
            &pending,
            Ok(png_image()),
        ));
        assert!(state
            .preview_render_cache
            .remote_image(&pending.key, "buf-1")
            .is_some());
        close(&state).unwrap();
        assert!(state
            .preview_render_cache
            .remote_image(&pending.key, "buf-1")
            .is_none());
        assert!(state
            .preview_render_cache
            .grant_remote_image(&pending.key, "buf-2")
            .is_none());
    }
}

#[test]
fn a_fetch_that_finishes_after_its_tab_closed_is_not_held() {
    let f = Fixture::new();
    let state = f.state();
    let pending = fetch_for(&state, "buf-1");
    close_buffer_inner(&state, "buf-1").unwrap();
    preview_remote_image_store_inner(&state, &pending, Ok(png_image()));
    assert!(state
        .preview_render_cache
        .remote_image(&pending.key, "buf-1")
        .is_none());
    assert!(state
        .preview_render_cache
        .grant_remote_image(&pending.key, "buf-2")
        .is_none());
}

#[test]
fn a_failed_fetch_is_answered_with_its_reason() {
    let f = Fixture::new();
    let state = f.state();
    let RemoteImageLookup::Fetch(pending) =
        preview_remote_image_lookup_inner(&state, "buf-1", "https://example.com/a.png").unwrap()
    else {
        panic!("expected a fetch");
    };
    for (refusal, expected) in [
        (RemoteImageRefusal::Unreachable, "unreachable"),
        (RemoteImageRefusal::TooLarge, "too_large"),
        (RemoteImageRefusal::NotAnImage, "not_an_image"),
        (RemoteImageRefusal::LocalAddress, "local_address"),
        (RemoteImageRefusal::NotSecure, "not_secure"),
    ] {
        let answer = preview_remote_image_store_inner(&state, &pending, Err(refusal));
        assert_eq!(reason(answer), expected);
    }
}

#[test]
fn every_command_is_in_the_invoke_handler() {
    for command in COMMANDS {
        assert!(
            LIB_RS.contains(command),
            "{command} is not in the invoke handler, so the editor cannot call it"
        );
    }
}
