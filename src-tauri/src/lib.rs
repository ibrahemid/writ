pub mod commands;
pub mod events;
pub mod first_run;
pub mod fts_scheduler;
pub mod generated;
pub mod hotkey;
pub mod logging;
pub mod notes;
pub mod poison;
pub mod preview;
pub mod quit;
#[cfg(target_os = "macos")]
mod quit_macos;
pub mod security;
pub mod snap_overlay;
pub mod startup;
pub mod startup_failure;
pub mod state;
pub mod watcher;
pub mod window_state;
pub mod workspace_index;

use events::{bus_bridge, emit_event, WritFrontendEvent};
use poison::recover_poison;
use state::AppState;
use tauri::{Listener, Manager};
use tracing::info;
use writ_core::startup::{StartupFailure, StartupStage};

/// How often the running app records a crash-recovery snapshot of the open
/// notes.
///
/// The snapshot is a safety net behind autosave, not a second save path, so
/// the interval trades recovery granularity against the writes it costs. A
/// pass that finds nothing changed writes nothing
/// ([`writ_storage::buffer_store::BufferStore::write_session_snapshot_if_changed`]),
/// which is what keeps an idle session from growing the database.
pub const SNAPSHOT_HEARTBEAT: std::time::Duration = std::time::Duration::from_secs(120);

/// Writes everything the next launch needs before the process goes away.
///
/// Deferred FTS reindexes go first: a reindex still inside its debounce window
/// would otherwise be lost, leaving search stale, because the startup
/// consistency check only removes orphan rows and never adds missing content
/// (ADR-020). The shutdown snapshot follows, and it must be the last thing
/// written, so it records the notes as the flush left them.
///
/// Text the flush could not write is folded in over the files
/// ([`writ_core::recovery::shutdown_snapshot`]), which is what keeps a quit
/// with a failed save from ending with that text nowhere.
pub(crate) fn finish_shutdown(app_handle: &tauri::AppHandle) {
    let state = app_handle.state::<AppState>();

    let pending = state.fts_scheduler.drain_pending();
    if !pending.is_empty() {
        if let Ok(store) = state.store.lock() {
            for id in pending {
                if let Err(e) = store.reindex_buffer(&id) {
                    tracing::debug!(buffer_id = %id, error = %e, "shutdown fts reindex failed");
                }
            }
        }
    }

    let unsaved = state.take_unsaved_on_exit();
    let unsaved_count = unsaved.len();
    let snapshot_result = state.store.lock().ok().map(|store| {
        store.collect_buffer_contents().and_then(|on_disk| {
            let (contents, is_clean) = writ_core::recovery::shutdown_snapshot(on_disk, unsaved);
            store.write_session_snapshot(&contents, is_clean)
        })
    });
    match snapshot_result {
        Some(Ok(())) if unsaved_count > 0 => {
            info!(notes = unsaved_count, "shutdown snapshot kept unsaved text")
        }
        Some(Ok(())) => info!("clean-shutdown snapshot written"),
        Some(Err(e)) => tracing::warn!(error = %e, "shutdown snapshot failed"),
        None => {}
    }
}

/// The Quit item's id. Not in [`MENU_ACTION_IDS`]: quitting is the exit path's
/// own business, not an action forwarded to the frontend.
#[cfg(target_os = "macos")]
const QUIT_MENU_ID: &str = "app.quit";

#[cfg(target_os = "macos")]
const MENU_ACTION_IDS: &[&str] = &[
    "app.check_updates",
    "file.open",
    "note.new",
    "note.today",
    "note.rename",
    "note.saveCopy",
    "buffer.close",
];

#[cfg(target_os = "macos")]
fn menu_action_for_id(id: &str) -> Option<&'static str> {
    MENU_ACTION_IDS
        .iter()
        .copied()
        .find(|&allowed| allowed == id)
}

/// Canonicalize and authorize OS-dropped paths into the set we will open.
///
/// Returns an empty vec when nothing survives, which is exactly what a nil or
/// empty macOS drag pasteboard now yields after the wry nil-safe patch (#113).
/// This must never panic on empty, non-UTF-8, or non-canonicalizable input.
fn dropped_paths_to_open(
    authorized: &security::AuthorizedPaths,
    paths: &[std::path::PathBuf],
) -> Vec<String> {
    let raw_paths: Vec<String> = paths
        .iter()
        .filter_map(|p| p.to_str().map(String::from))
        .collect();
    startup::authorize_and_canonicalize(authorized, &raw_paths)
}

/// Builds the native macOS menu bar.
///
/// macOS-only by design: it hosts the system menu bar that macOS apps are
/// expected to provide, and its `CmdOrCtrl+O/N/W` accelerators are correct
/// there. On Windows/Linux the window runs with `decorations: false`, so this
/// menu would be invisible chrome while its accelerators collide with the
/// platform translator. Every action it exposes (`app.check_updates`,
/// `file.open`, `note.new`, `note.today`, `note.rename`, `note.saveCopy`,
/// `buffer.close`) is also registered as a command palette entry in the
/// frontend, most of them with a keyboard shortcut, so gating it off those
/// platforms removes dead chrome without removing any reachable action.
#[cfg(target_os = "macos")]
fn build_app_menu(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
    use writ_core::events::bus::WritEvent;

    let check_updates =
        MenuItemBuilder::with_id("app.check_updates", "Check for Updates…").build(app)?;

    // Not `PredefinedMenuItem::quit`: muda maps that to AppKit's `terminate:`,
    // which tears the process down without ever reaching the event loop, so no
    // exit request is raised and nothing gets a chance to write. A plain item
    // that calls `AppHandle::exit` goes the long way round, through
    // `RunEvent::ExitRequested` and the flush handshake.
    let quit_item = MenuItemBuilder::with_id(QUIT_MENU_ID, "Quit Writ")
        .accelerator("CmdOrCtrl+Q")
        .build(app)?;

    let app_menu = SubmenuBuilder::new(app, "Writ")
        .items(&[
            &PredefinedMenuItem::about(app, Some("About Writ"), None)?,
            &PredefinedMenuItem::separator(app)?,
            &check_updates,
            &PredefinedMenuItem::separator(app)?,
            &quit_item,
        ])
        .build()?;

    let open_file = MenuItemBuilder::with_id("file.open", "Open File…")
        .accelerator("CmdOrCtrl+O")
        .build(app)?;
    let quick_open = MenuItemBuilder::with_id("notes.quickOpen", "Open Note…")
        .accelerator("CmdOrCtrl+Shift+O")
        .build(app)?;
    let new_note = MenuItemBuilder::with_id("note.new", "New Note")
        .accelerator("CmdOrCtrl+N")
        .build(app)?;
    // No accelerator: the shortcut table is spec K1's.
    let todays_note = MenuItemBuilder::with_id("note.today", "Today's Note").build(app)?;
    let rename_note = MenuItemBuilder::with_id("note.rename", "Rename Note…").build(app)?;
    let save_copy = MenuItemBuilder::with_id("note.saveCopy", "Save a Copy…").build(app)?;
    let close_tab = MenuItemBuilder::with_id("buffer.close", "Close Tab")
        .accelerator("CmdOrCtrl+W")
        .build(app)?;

    let file_menu = SubmenuBuilder::new(app, "File")
        .items(&[
            &new_note,
            &todays_note,
            &quick_open,
            &open_file,
            &PredefinedMenuItem::separator(app)?,
            &rename_note,
            &save_copy,
            &PredefinedMenuItem::separator(app)?,
            &close_tab,
        ])
        .build()?;

    let edit_menu = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;

    let window_menu = SubmenuBuilder::new(app, "Window").minimize().build()?;

    let menu = MenuBuilder::new(app)
        .items(&[&app_menu, &file_menu, &edit_menu, &window_menu])
        .build()?;

    app.set_menu(menu)?;

    app.on_menu_event(move |app_handle, event| {
        let id = event.id().0.as_str();
        if id == QUIT_MENU_ID {
            app_handle.exit(0);
            return;
        }
        if let Some(action) = menu_action_for_id(id) {
            let state = app_handle.state::<AppState>();
            state.event_bus.emit(WritEvent::MenuAction {
                action: action.to_string(),
            });
        }
    });

    Ok(())
}

/// Walks the notes folder and brings the index in line with it, on a thread of
/// its own.
///
/// The walk reads every note, and a folder of a few thousand would hold the
/// caller for seconds. An empty index reconciles to a full one, which is what
/// makes deleting `writ.db` safe.
///
/// `gate` keeps one walk at a time. The sweep the notes watcher raises during
/// a sync catch-up can arrive again while the last walk is still running, and
/// starting a second walk over the same folder would read every note twice for
/// one answer. A walk under way covers most of what lands while it runs — but
/// not a file it has already read and something rewrote behind it, so a
/// request that arrives during a walk earns exactly one walk after it, however
/// many such requests there were.
///
/// The walk retires itself the moment the notes folder moves: one that
/// finished against the old folder would prune every row the move re-keyed.
/// The generation is taken per walk, so the one that follows reads the folder
/// Writ holds now.
fn spawn_notes_reconcile(
    index: std::sync::Arc<writ_storage::notes_index::NotesIndexStore>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    gate: std::sync::Arc<writ_core::watcher::reconcile::ReconcileGate>,
    root: std::path::PathBuf,
) {
    use std::sync::atomic::Ordering;

    if !gate.request() {
        return;
    }
    std::thread::spawn(move || loop {
        let generation = index.generation();
        let cancelled = || cancel.load(Ordering::Relaxed) || index.generation() != generation;
        let outcome = index.reconcile(&root, &cancelled, &writ_storage::notes_index::is_dataless);
        match outcome {
            Ok(outcome) => info!(
                added = outcome.added,
                updated = outcome.updated,
                removed = outcome.removed,
                name_only = outcome.skipped_dataless,
                cancelled = outcome.cancelled,
                "notes index reconciled"
            ),
            Err(e) => tracing::warn!(error = %e, "notes index reconcile failed"),
        }
        // Shutdown takes precedence over anything owed: a walk asked for
        // during the last one is not a reason to keep the process alive.
        if cancel.load(Ordering::Relaxed) {
            gate.release();
            return;
        }
        if !gate.finished() {
            return;
        }
    });
}

/// How long the main window may stay hidden before Rust shows it without the
/// frontend's signal.
const FIRST_PAINT_FALLBACK: std::time::Duration = std::time::Duration::from_secs(3);

/// Shows the main window and gives it focus, on the thread AppKit requires.
///
/// Every reveal goes through here, and every refusal is logged: a window that
/// will not show is the one failure the user cannot see for themselves, since
/// what it leaves behind is a running app with nothing on screen.
fn show_main_window(app: &tauri::AppHandle, window: tauri::WebviewWindow, reason: &'static str) {
    let dispatched = app.run_on_main_thread(move || {
        if let Err(e) = window.show() {
            tracing::warn!(reason, error = %e, "the window could not be shown");
        }
        if let Err(e) = window.set_focus() {
            tracing::warn!(reason, error = %e, "the shown window could not be focused");
        }
    });
    if let Err(e) = dispatched {
        tracing::warn!(reason, error = %e, "the show never reached the main thread");
    }
}

/// The `reason` every log line from a Dock click carries.
#[cfg(target_os = "macos")]
const DOCK_REOPEN: &str = "dock reopen";

/// Brings the main window back for a Dock click.
///
/// This runs on the thread AppKit delivered the click on, which is the main
/// one, so asking the window about itself here costs nothing. A minimized
/// window has to come out of the Dock before it can be shown, and focus is
/// last either way.
#[cfg(target_os = "macos")]
fn reveal_from_dock(window: &tauri::WebviewWindow) {
    match window.is_minimized() {
        Ok(true) => {
            if let Err(e) = window.unminimize() {
                tracing::warn!(reason = DOCK_REOPEN, error = %e, "the window could not be unminimized");
            }
        }
        Ok(false) => {}
        Err(e) => {
            tracing::warn!(reason = DOCK_REOPEN, error = %e, "the window's minimized state could not be read");
        }
    }
    if let Err(e) = window.show() {
        tracing::warn!(reason = DOCK_REOPEN, error = %e, "the window could not be shown");
    }
    if let Err(e) = window.set_focus() {
        tracing::warn!(reason = DOCK_REOPEN, error = %e, "the shown window could not be focused");
    }
}

/// Brings the main window up once per launch, whoever asks first.
///
/// Writ starts its window hidden to kill the cold-start flash (PR #152), which
/// makes showing it a step that has to happen rather than one that already
/// did. Two callers own that step between them — the frontend after its first
/// paint, and the timer behind it — and the claim in [`AppState`] is what
/// keeps them from both driving AppKit at once.
///
/// Returns whether this call is the one that showed it.
pub fn reveal_main_window(app: &tauri::AppHandle, reason: &'static str) -> bool {
    let Some(window) = app.get_webview_window("main") else {
        tracing::warn!(reason, "there is no main window to show");
        return false;
    };
    let read_visible = || {
        window.is_visible().map_err(|e| {
            tracing::warn!(reason, error = %e, "the window's visibility could not be read");
        })
    };
    if claim_reveal(&app.state::<AppState>(), read_visible) == window_state::RevealAction::Skip {
        return false;
    }
    show_main_window(app, window.clone(), reason);
    true
}

/// Takes the startup reveal for this caller, and answers what it should do.
///
/// The claim is a swap rather than a read: two callers reaching this at once
/// have to come away with different answers, or a launch shows its window
/// twice and AppKit gets the same window from two threads.
///
/// `read_visible` is a closure and not a value because reading the window's
/// visibility means asking the main thread and waiting for the answer, and the
/// launch that needs the timer most is the one whose main thread is too busy
/// to give it: a wait there is a wait for the window itself. It is only asked
/// when the answer can still change what happens.
pub fn claim_reveal(
    state: &AppState,
    read_visible: impl FnOnce() -> Result<bool, ()>,
) -> window_state::RevealAction {
    let already_revealed = state
        .window_revealed
        .swap(true, std::sync::atomic::Ordering::SeqCst);
    let dismissed = state
        .window_dismissed
        .load(std::sync::atomic::Ordering::SeqCst);
    if !already_revealed || dismissed {
        return window_state::decide_reveal(already_revealed, dismissed, Ok(false));
    }
    window_state::decide_reveal(already_revealed, dismissed, read_visible())
}

/// Records that the user put the window away, so the startup timer leaves
/// it where they put it.
///
/// The timer runs for the first seconds of a launch, and a dismissal inside
/// those seconds is still a dismissal: without this, hiding Writ right after
/// opening it would bring it straight back.
pub fn note_window_dismissed(app: &tauri::AppHandle) {
    app.state::<AppState>()
        .window_dismissed
        .store(true, std::sync::atomic::Ordering::SeqCst);
}
/// Applies the saved window geometry to the (hidden) main window before it is
/// first shown, so the frontend never has to await IPC to resize on the cold
/// path. The saved size is fitted to the work area of the monitor the window
/// will land on, so a rect saved from a larger display can never reopen bigger
/// than the screen; positioning then runs that fitted rect through
/// [`window_state::place_window`] against the live monitor layout so a window
/// saved on a now-disconnected display re-centers instead of opening
/// off-screen.
fn restore_main_window_geometry(
    window: &tauri::WebviewWindow,
    cfg: &writ_core::config::WindowConfig,
) {
    // Work areas rather than full monitor rects: a restored window must not
    // land under the taskbar or the menu bar.
    let work_area_of = |m: &tauri::Monitor| {
        let area = m.work_area();
        window_state::logical_rect(
            area.position.x,
            area.position.y,
            area.size.width,
            area.size.height,
            m.scale_factor(),
        )
    };

    let monitors: Vec<window_state::Rect> = window
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(work_area_of)
        .collect();

    // The monitor `window.center()` uses, so a saved rect that overlaps nothing
    // is sized against the display it will actually be centered on.
    let fallback = match window.current_monitor() {
        Ok(Some(m)) => Some(work_area_of(&m)),
        _ => None,
    };

    let plan = window_state::plan_restore(
        cfg.width,
        cfg.height,
        cfg.x,
        cfg.y,
        &monitors,
        fallback,
        window_state::MIN_LOGICAL_WIDTH,
        window_state::MIN_LOGICAL_HEIGHT,
    );

    if let Some((width, height)) = plan.size {
        let _ = window.set_size(tauri::LogicalSize::new(f64::from(width), f64::from(height)));
    }

    match plan.placement {
        Some(window_state::WindowPlacement::At { x, y }) => {
            let _ = window.set_position(tauri::LogicalPosition::new(f64::from(x), f64::from(y)));
        }
        Some(window_state::WindowPlacement::Center) => {
            let _ = window.center();
        }
        None => {}
    }
}

pub fn run() {
    let writ_dir =
        match state::resolve_writ_dir(std::env::var("WRIT_DATA_DIR").ok(), dirs::home_dir()) {
            Ok(dir) => dir,
            Err(error) => startup_failure::abort_with_report(
                &StartupFailure::new(
                    StartupStage::DataDirectory,
                    error.to_string(),
                    None,
                    startup_failure::timestamp(),
                ),
                None,
            ),
        };

    // Runs before anything opens the database: SQLite's write-ahead log does
    // not survive a sync provider. The report goes to the temporary directory
    // because the logs directory is inside the folder being turned down.
    let verdict = startup::data_dir_verdict(&writ_dir, dirs::home_dir().as_deref(), None);
    if verdict != writ_core::startup::DataDirVerdict::Ok {
        startup_failure::abort_with_report(
            &StartupFailure::new(
                StartupStage::DataDirectoryLocation,
                writ_core::startup::data_dir_refusal_message(&verdict),
                Some(writ_dir.clone()),
                startup_failure::timestamp(),
            ),
            None,
        );
    }

    let logs_dir = writ_dir.join("logs");
    // Installed first: setting the hook touches nothing, while opening the
    // log file can fail on the same unwritable directory.
    logging::panic_handler::install_panic_handler(&logs_dir);
    logging::init_logging(&logs_dir);

    info!("writ starting");

    let app_state = match AppState::initialize() {
        Ok(app_state) => app_state,
        Err(error) => {
            // A refusal from the data-folder guard is a location failure, not
            // a permission one, and its report belongs outside the folder
            // being turned down. The stage decides both the step the report
            // names and the remedy under it, and `StartupStage::AppState`
            // would tell the user to fix the folder's permissions.
            let refused = error
                .downcast_ref::<writ_core::startup::DataDirRefused>()
                .is_some();
            let (stage, report_dir) = if refused {
                (StartupStage::DataDirectoryLocation, None)
            } else {
                (StartupStage::AppState, Some(logs_dir.as_path()))
            };
            startup_failure::abort_with_report(
                &StartupFailure::new(
                    stage,
                    error.to_string(),
                    Some(writ_dir.clone()),
                    startup_failure::timestamp(),
                ),
                report_dir,
            )
        }
    };
    let config_path = app_state.writ_dir.join("config.toml");
    let watcher_ignore = app_state.watcher_ignore.clone();

    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    let builder =
        tauri::Builder::default().plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            let args: Vec<std::ffi::OsString> = argv
                .into_iter()
                .skip(1)
                .map(std::ffi::OsString::from)
                .collect();

            let raw_paths = writ_core::file_ops::arg_paths_from_iter(args)
                .into_iter()
                .filter_map(|p| p.to_str().map(String::from))
                .collect::<Vec<String>>();

            if !raw_paths.is_empty() {
                let state = app.state::<AppState>();
                let paths =
                    startup::authorize_and_canonicalize(&state.authorized_paths, &raw_paths);
                if !paths.is_empty() {
                    info!(
                        count = paths.len(),
                        "files forwarded from secondary instance"
                    );
                    let ready = state
                        .frontend_ready
                        .load(std::sync::atomic::Ordering::SeqCst);

                    if ready {
                        let _ = emit_event(app, WritFrontendEvent::PendingOpens { paths });
                    } else {
                        let mut pending = recover_poison(
                            state.pending_opens.lock(),
                            "lib::single_instance:forward",
                        );
                        pending.extend(paths);
                    }
                }
            }

            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }));

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    let builder = tauri::Builder::default();

    let app = builder
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        // Registered for the Rust-side `OpenerExt` only. `opener:allow-open-url`
        // stays out of capabilities/default.json so the frontend cannot reach
        // the plugin's own command and skip the link policy (ADR-025).
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .register_uri_scheme_protocol("writ-preview", preview::handler::serve)
        .manage(app_state)
        .manage(commands::ai::AiState::default())
        .manage(commands::materialise::MaterialiseState::default())
        .invoke_handler(tauri::generate_handler![
            commands::buffer::create_buffer,
            commands::buffer::get_buffer,
            commands::buffer::save_buffer_content,
            commands::buffer::save_buffer_content_unindexed,
            commands::buffer::restore_note_file,
            commands::buffer::read_buffer_content,
            commands::buffer::note_disk_state,
            commands::buffer::resolve_external_change,
            commands::buffer::record_unsaved_notes,
            commands::buffer::list_active_buffers,
            commands::buffer::close_buffer,
            commands::buffer::close_buffers,
            commands::buffer::delete_buffer,
            commands::buffer::update_tab_order,
            commands::buffer::rename_buffer,
            commands::notes::new_note,
            commands::notes::todays_note,
            commands::notes::new_note_from_link,
            commands::notes::rename_note,
            commands::notes::count_links_to,
            commands::notes::rename_note_with_links,
            commands::notes::undo_rename_with_links,
            commands::notes::delete_note,
            commands::notes::save_note_copy,
            commands::notes::show_note_in_file_manager,
            commands::notes::show_notes_file_in_file_manager,
            commands::notes::get_notes_root,
            commands::first_run::first_run_state,
            commands::first_run::dismiss_first_run_hint,
            commands::first_run::auto_retitle_note,
            commands::notes::get_notes_folder,
            commands::notes::show_notes_folder_in_finder,
            commands::notes::pick_notes_folder,
            commands::notes::get_notes_migration_report,
            commands::notes::dismiss_notes_migration_report,
            commands::notes::move_archived_notes,
            commands::note_index::resolve_note_link,
            commands::note_index::note_facts,
            commands::note_index::note_name_candidates,
            commands::note_index::note_backlinks,
            commands::note_index::note_heading_line,
            commands::note_index::note_all_tags,
            commands::note_index::note_paths_for_tag,
            commands::note_index::note_graph,
            commands::file::open_file,
            commands::file::open_file_confirmed,
            commands::file::pick_files_to_open,
            commands::materialise::materialise_note,
            commands::materialise::cancel_materialise_note,
            commands::history::list_history,
            commands::history::restore_buffer,
            commands::history::clear_history,
            commands::history::search_buffers,
            commands::history::search_notes_by_name,
            commands::config::get_config,
            commands::config::update_config,
            commands::window::confirm_quit_flush,
            commands::window::reveal_window,
            commands::window::toggle_window,
            commands::window::compute_window_placement,
            commands::window::set_caption_button_metrics,
            commands::transforms::list_transforms,
            commands::transforms::apply_transform,
            commands::prompt::prompt_estimate_tokens,
            commands::prompt::prompt_scan_placeholders,
            commands::prompt::prompt_fill_placeholders,
            commands::spelling::check_spelling,
            commands::spelling::spelling_add_ignored_word,
            commands::perf::report_first_paint,
            commands::update::check_for_update,
            commands::update::download_and_install_update,
            commands::update::dismiss_update,
            commands::update::restart_app,
            commands::preview::preview_list_renderers,
            commands::preview::preview_close,
            commands::preview::preview_render,
            commands::preview::preview_force_render,
            commands::preview::preview_set_layout,
            commands::preview::preview_get_layout,
            commands::recovery::get_recovered_buffers,
            commands::workspace::pick_workspace_folder,
            commands::workspace::clear_workspace_root,
            commands::workspace::list_workspace_dir,
            commands::workspace::get_workspace_root,
            commands::workspace::search_workspace_files,
            commands::workspace::workspace_index_status,
            commands::workspace::search_workspace_content,
            commands::cli::cli_status,
            commands::cli::install_cli,
            commands::default_app::list_default_app_types,
            commands::default_app::get_default_app_status,
            commands::default_app::set_default_app,
            commands::inbox::pick_inbox_folder,
            commands::inbox::clear_inbox,
            commands::inbox::get_inbox_path,
            commands::inbox::list_inbox_files,
            commands::link::open_external_url,
            commands::link::classify_external_url,
            commands::storage::get_storage_info,
            commands::storage::reveal_storage_path,
            commands::notices::open_third_party_notices,
            commands::ai::ai_rewrite,
            commands::ai::ai_cancel,
            commands::ai::ai_check_connection,
            commands::ai::ai_set_api_key,
            commands::ai::ai_clear_api_key,
            commands::ai::ai_has_api_key,
            commands::ai::ai_endpoint_state,
            commands::ai::ai_consent_host,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();

            {
                let state = app.state::<AppState>();
                let bridge_handle = handle.clone();
                bus_bridge::attach_bridge(&state.event_bus, move |frontend_event| {
                    let _ = emit_event(&bridge_handle, frontend_event);
                });
                info!("event bus bridge attached");
            }

            // Maintain the workspace file-name index from watcher events
            // (ADR-026). A separate subscriber from the frontend bridge, so
            // both observe every WorkspaceChanged.
            {
                use writ_core::events::bus::WritEvent;
                let state = app.state::<AppState>();
                let index = state.workspace_index.clone();
                state.event_bus.subscribe(move |event| {
                    if let WritEvent::WorkspaceChanged { path, removed } = event {
                        workspace_index::on_workspace_changed(&index, path, *removed);
                    }
                });
            }

            // Keep the notes index honest about a file another program wrote
            // (ADR-028 section 7). A separate subscriber from the frontend
            // bridge, so both observe every NotesChanged.
            //
            // It patches the one path and nothing else. It must never reload
            // the document registry: that recreates a loaded
            // `writ-preview://` iframe and hard-freezes the macOS webview
            // (PR #127).
            {
                use writ_core::events::bus::WritEvent;
                let state = app.state::<AppState>();
                let index = state.notes_index.clone();
                let cancel = state.notes_index_cancel.clone();
                let reconcile_gate = state.notes_reconcile.clone();
                // The live root, read per event rather than captured: moving
                // the notes folder restarts the watcher, and a marker carrying
                // the new root has to still read as a marker.
                let handle = app.handle().clone();
                state.event_bus.subscribe(move |event| {
                    // More changed in one window than was worth listing, so
                    // the index walks the folder instead of patching a path.
                    // The root is read live rather than captured: moving the
                    // notes folder restarts the watcher, and the walk has to
                    // follow.
                    if let WritEvent::NotesSwept { .. } = event {
                        spawn_notes_reconcile(
                            index.clone(),
                            cancel.clone(),
                            reconcile_gate.clone(),
                            handle.state::<AppState>().notes_root(),
                        );
                        return;
                    }
                    let WritEvent::NotesChanged { path, removed } = event else {
                        return;
                    };
                    let path = std::path::Path::new(path);
                    // A rename arrives as one delete and one create, so both
                    // arms have to be replayable: forgetting a path the index
                    // does not hold, and indexing one it already has, are both
                    // no-ops rather than errors.
                    let result = if *removed {
                        index.forget_path(path)
                    } else {
                        index.index_path(path).map(|_| ())
                    };
                    if let Err(e) = result {
                        tracing::warn!(path = %path.display(), error = %e, "notes index update failed");
                    }
                });
            }

            // A rename the note's own first line asks for is only safe while
            // nothing outside Writ has looked at the note. The bus is where
            // that is seen: every write Writ made has already been dropped by
            // the watcher's ignore set before an event reaches here.
            {
                use writ_core::events::bus::WritEvent;
                let state = app.state::<AppState>();
                let watch = state.retitle_watch.clone();
                state.event_bus.subscribe(move |event| {
                    if let WritEvent::NotesChanged { path, .. } = event {
                        watch.changed_outside(std::path::Path::new(path));
                    }
                });
            }

            // First launch opens a note the person can type in, and asks
            // nothing on the way (spec O1). It runs before the window is
            // shown, so the frontend's first load already finds the note in
            // the store and takes its ordinary "open the last tab" path.
            {
                let state = app.state::<AppState>();
                first_run::open_first_note(&state);
            }

            // The window is created hidden (tauri.conf `visible: false`) to kill
            // the cold-start flash. Restore its saved geometry from the cached
            // config here, while still hidden, so the frontend never has to
            // round-trip IPC just to resize; the frontend calls show() after its
            // first paint, and the fallback below guarantees it appears even if
            // the frontend never signals.
            {
                let cfg_window = {
                    let state = app.state::<AppState>();
                    let cfg = recover_poison(state.config.lock(), "lib::setup:window_geometry");
                    cfg.window.clone()
                };
                if let Some(window) = app.get_webview_window("main") {
                    restore_main_window_geometry(&window, &cfg_window);
                }
            }

            let fallback_handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                tauri::async_runtime::spawn_blocking(|| {
                    std::thread::sleep(FIRST_PAINT_FALLBACK);
                })
                .await
                .ok();
                if reveal_main_window(&fallback_handle, "startup fallback") {
                    tracing::warn!(
                        "frontend did not signal first paint; showing window via fallback"
                    );
                }
            });

            #[cfg(not(any(target_os = "macos", target_os = "ios")))]
            {
                let state = handle.state::<AppState>();
                let args = std::env::args_os().skip(1);
                let count = startup::push_arg_paths_into_pending(
                    &state.pending_opens,
                    &state.authorized_paths,
                    args,
                );
                if count > 0 {
                    info!(count, "files opened from OS via argv");
                }
            }

            {
                let ready_handle = handle.clone();
                app.listen("frontend-ready", move |_event| {
                    let state = ready_handle.state::<AppState>();
                    state
                        .frontend_ready
                        .store(true, std::sync::atomic::Ordering::SeqCst);
                    let drained: Vec<String> = {
                        let mut pending = recover_poison(
                            state.pending_opens.lock(),
                            "lib::setup:frontend_ready_drain",
                        );
                        std::mem::take(&mut *pending)
                    };
                    info!(
                        count = drained.len(),
                        "frontend-ready: draining pending opens"
                    );
                    if !drained.is_empty() {
                        let _ = emit_event(
                            &ready_handle,
                            WritFrontendEvent::PendingOpens { paths: drained },
                        );
                    }
                });
            }

            #[cfg(target_os = "macos")]
            if let Err(e) = build_app_menu(app) {
                tracing::warn!(error = %e, "failed to build application menu");
            }

            // The Dock, an Apple Event quit and a logout all reach the app
            // through `terminate:`, which never raises an exit request.
            #[cfg(target_os = "macos")]
            quit_macos::install(&handle);

            if let Err(e) = hotkey::setup_global_hotkey(&handle) {
                tracing::warn!(error = %e, "failed to register global hotkey");
            }

            let watcher_bus = app.state::<AppState>().event_bus.clone();
            match watcher::handler::start_file_watcher(watcher_bus, config_path, watcher_ignore) {
                Ok(handle) => {
                    let state = app.state::<AppState>();
                    let mut slot =
                        recover_poison(state.watcher.lock(), "lib::setup:watcher_handle_stash");
                    *slot = Some(handle);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "failed to start file watcher");
                }
            }

            {
                let state = app.state::<AppState>();
                let restored_root = state
                    .workspace_root
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                if let Some(root) = restored_root {
                    // Build the file-name index for the restored root (ADR-026).
                    workspace_index::spawn_rebuild(state.workspace_index.clone());
                    match watcher::handler::start_workspace_watcher(state.event_bus.clone(), root) {
                        Ok(handle) => {
                            let mut slot = recover_poison(
                                state.workspace_watcher.lock(),
                                "lib::setup:workspace_watcher_stash",
                            );
                            *slot = Some(handle);
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "failed to start workspace watcher");
                        }
                    }
                }
            }

            {
                let state = app.state::<AppState>();
                let notes_root = state.notes_root();

                // Bring the index in line with the folder before anything
                // searches it.
                spawn_notes_reconcile(
                    state.notes_index.clone(),
                    state.notes_index_cancel.clone(),
                    state.notes_reconcile.clone(),
                    notes_root.clone(),
                );

                // The registry of open files goes up first: it is how the
                // notes watcher answers "which tab holds this file", and that
                // watcher is the only route by which a note inside the notes
                // folder reaches its tab. It watches no folder until a tab
                // asks for one, so starting it here costs one channel and two
                // idle backends.
                // What a file leaving its path means, and where the answer
                // is recorded. Both watchers take the same one, so a note that
                // moves between the notes folder and anywhere else is decided
                // the same way whichever watcher sees it go.
                let tracking = watcher::moves::FileTracking::of_app(app.handle().clone());
                {
                    let mut slot = recover_poison(
                        state.file_tracking.lock(),
                        "lib::setup:file_tracking_stash",
                    );
                    *slot = Some(tracking.clone());
                }

                let open_notes: std::sync::Arc<dyn watcher::open_files::OpenNotes> =
                    match watcher::open_files::start_open_file_watcher(
                        state.event_bus.clone(),
                        state.watcher_ignore.clone(),
                        &notes_root,
                        tracking.clone(),
                    ) {
                        Ok(handle) => {
                            let lookup = handle.open_notes();
                            let mut slot = recover_poison(
                                state.open_file_watcher.lock(),
                                "lib::setup:open_file_watcher_stash",
                            );
                            *slot = Some(handle);
                            lookup
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "failed to start open file watcher");
                            std::sync::Arc::new(watcher::open_files::NoOpenNotes)
                        }
                    };

                match watcher::handler::start_notes_watcher(
                    state.event_bus.clone(),
                    notes_root,
                    state.watcher_ignore.clone(),
                    open_notes,
                    tracking,
                ) {
                    Ok(handle) => {
                        let mut slot = recover_poison(
                            state.notes_watcher.lock(),
                            "lib::setup:notes_watcher_stash",
                        );
                        *slot = Some(handle);
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "failed to start notes watcher");
                    }
                }
            }

            {
                let state = app.state::<AppState>();
                let restored_inbox = state
                    .inbox_root
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                if let Some(root) = restored_inbox {
                    match watcher::handler::start_inbox_watcher(
                        state.event_bus.clone(),
                        root,
                        state.watcher_ignore.clone(),
                    ) {
                        Ok(handle) => {
                            let mut slot = recover_poison(
                                state.inbox_watcher.lock(),
                                "lib::setup:inbox_watcher_stash",
                            );
                            *slot = Some(handle);
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "failed to start inbox watcher");
                        }
                    }
                }
            }

            let snapshot_handle = handle.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(SNAPSHOT_HEARTBEAT);
                let snapshot_error = {
                    let state = snapshot_handle.state::<AppState>();
                    state.store.lock().ok().map(|mut store| {
                        store
                            .collect_buffer_contents()
                            .and_then(|contents| store.write_session_snapshot_if_changed(&contents))
                    })
                };
                if let Some(Err(e)) = snapshot_error {
                    tracing::warn!(error = %e, "periodic snapshot failed");
                }
            });

            let updater_auto_check = {
                let state = app.state::<AppState>();
                let cfg = recover_poison(state.config.lock(), "lib::setup:updater_auto_check");
                cfg.updater.auto_check
            };
            let update_writ_dir = app.state::<AppState>().writ_dir.clone();
            tauri::async_runtime::spawn(async move {
                tauri::async_runtime::spawn_blocking(|| {
                    std::thread::sleep(std::time::Duration::from_secs(5));
                })
                .await
                .ok();
                // Gate the silent check: only when the user has auto-check on
                // and the last silent check is older than the interval, so a
                // quick launch-and-quit no longer phones the release endpoint.
                let now_ms = commands::update::now_epoch_ms();
                let last_ms = commands::update::read_last_check_ms(&update_writ_dir);
                if writ_core::update::auto_check::should_auto_check(
                    updater_auto_check,
                    last_ms,
                    now_ms,
                    writ_core::update::auto_check::MIN_CHECK_INTERVAL_MS,
                ) {
                    commands::update::write_last_check_ms(&update_writ_dir, now_ms);
                    commands::update::run_update_check(handle, false).await;
                } else {
                    tracing::debug!(
                        "silent update check skipped (auto_check off or within interval)"
                    );
                }
            });

            info!("writ ready");
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build writ");

    app.run(move |app_handle, event| {
        if let tauri::RunEvent::ExitRequested { api, code, .. } = &event {
            // A restart is not a quit the handshake can hold: `prevent_exit`
            // is a documented no-op for it, so asking the frontend to flush
            // would leave the answer arriving after the runtime had gone.
            // Write what Rust holds and let it through.
            if *code == Some(tauri::RESTART_EXIT_CODE) {
                let state = app_handle.state::<AppState>();
                if state.quit.claim_final_shutdown() {
                    finish_shutdown(app_handle);
                    state.quit.finish();
                }
                return;
            }

            let state = app_handle.state::<AppState>();
            match state.quit.begin(*code) {
                // Already written, this is the exit the waiter asked for.
                quit::QuitDecision::Proceed => return,
                // A second Cmd+Q on top of a quit that is still writing. Hold
                // it: letting this one through would end the process mid-write.
                quit::QuitDecision::Wait => {
                    api.prevent_exit();
                    return;
                }
                quit::QuitDecision::StartFlush => {}
            }

            // Stop the reconcile walk rather than waiting for it: a cancelled
            // pass removes nothing, and the next launch reconciles again.
            state
                .notes_index_cancel
                .store(true, std::sync::atomic::Ordering::Relaxed);

            // Nothing has asked the frontend to write what the user typed
            // inside the last debounce window yet. Ask, and let the event loop
            // run: the webview handles the request on this very thread, so
            // waiting here is what would make the answer impossible.
            if state
                .frontend_ready
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                api.prevent_exit();
                state
                    .event_bus
                    .emit(writ_core::events::bus::WritEvent::FlushBeforeQuit);

                let quit_state = state.quit.clone();
                let shutdown_handle = app_handle.clone();
                std::thread::spawn(move || {
                    if !quit_state.wait_for_flush() {
                        tracing::warn!("frontend did not confirm its flush before the timeout");
                    }
                    finish_shutdown(&shutdown_handle);
                    // Only now may a repeat request exit: until the writes are
                    // on disk, one would take the process down mid-snapshot.
                    quit_state.finish();
                    shutdown_handle.exit(quit_state.exit_code());
                });
                return;
            }

            finish_shutdown(app_handle);
            state.quit.finish();
        }

        // macOS quits that never raise a request still land here: `NSApp
        // terminate:`, which the Dock and an `osascript quit` both send, runs
        // this from inside `applicationWillTerminate:`. Nothing can be asked
        // of the webview on that thread, but what Rust holds is still ours to
        // write.
        if matches!(event, tauri::RunEvent::Exit) {
            let state = app_handle.state::<AppState>();
            if state.quit.claim_final_shutdown() {
                state
                    .notes_index_cancel
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                finish_shutdown(app_handle);
                state.quit.finish();
            }
        }

        #[cfg(any(target_os = "macos", target_os = "ios"))]
        if let tauri::RunEvent::Opened { urls } = &event {
            let raw_paths: Vec<String> = urls
                .iter()
                .filter_map(|url| {
                    if url.scheme() == "file" {
                        url.to_file_path()
                            .ok()
                            .and_then(|p| p.to_str().map(String::from))
                    } else {
                        None
                    }
                })
                .collect();

            if !raw_paths.is_empty() {
                let state = app_handle.state::<AppState>();
                let paths =
                    startup::authorize_and_canonicalize(&state.authorized_paths, &raw_paths);
                if !paths.is_empty() {
                    let (dirs, files): (Vec<String>, Vec<String>) = paths
                        .into_iter()
                        .partition(|p| std::path::Path::new(p).is_dir());

                    if let Some(dir_path) = dirs.last() {
                        info!(path = %dir_path, "workspace folder opened from OS");
                        if let Err(e) = commands::workspace::set_workspace_root_from_path(
                            &state,
                            std::path::Path::new(dir_path),
                        ) {
                            tracing::warn!(error = %e, "failed to set workspace root from OS open");
                        }
                    }

                    if !files.is_empty() {
                        info!(count = files.len(), "files opened from OS");

                        let ready = state
                            .frontend_ready
                            .load(std::sync::atomic::Ordering::SeqCst);

                        if ready {
                            let _ = emit_event(
                                app_handle,
                                WritFrontendEvent::PendingOpens { paths: files },
                            );
                        } else {
                            let mut pending = recover_poison(
                                state.pending_opens.lock(),
                                "lib::run_event:opened_files",
                            );
                            pending.extend(files);
                        }
                    }

                    if let Some(window) = app_handle.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
            }
        }

        // Clicking the Dock icon of an app whose only window is hidden asks
        // AppKit to reopen it, and AppKit has nothing to reopen: Writ hides its
        // window rather than closing it, so the click reaches here or it does
        // nothing at all. Answering it is what keeps a launch that ended with
        // no window recoverable without quitting.
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen {
            has_visible_windows,
            ..
        } = &event
        {
            if let Some(window) = app_handle.get_webview_window("main") {
                let is_visible = window.is_visible().map_err(|e| {
                    tracing::warn!(error = %e, "the window's visibility could not be read");
                });
                match window_state::decide_reopen(*has_visible_windows, is_visible) {
                    window_state::ReopenAction::Reveal => {
                        info!("window shown from the dock");
                        reveal_from_dock(&window);
                    }
                    window_state::ReopenAction::Focus => {
                        if let Err(e) = window.set_focus() {
                            tracing::warn!(reason = DOCK_REOPEN, error = %e, "the window could not be focused");
                        }
                    }
                }
            }
        }

        if let tauri::RunEvent::WindowEvent {
            event: tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }),
            ..
        } = &event
        {
            let state = app_handle.state::<AppState>();
            let canonical_paths = dropped_paths_to_open(&state.authorized_paths, paths);
            if !canonical_paths.is_empty() {
                info!(count = canonical_paths.len(), "files dropped onto window");
                let _ = emit_event(
                    app_handle,
                    WritFrontendEvent::FilesDropped {
                        paths: canonical_paths,
                    },
                );
            }
        }

        let _ = (&app_handle, &event);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn menu_action_for_id_returns_each_whitelisted_id() {
        for id in MENU_ACTION_IDS {
            assert_eq!(menu_action_for_id(id), Some(*id));
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_quit_item_is_not_forwarded_to_the_frontend() {
        assert_eq!(
            menu_action_for_id(QUIT_MENU_ID),
            None,
            "quit must reach the exit path, not the frontend's menu handler"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn menu_action_for_id_returns_none_for_unknown_ids() {
        assert_eq!(menu_action_for_id(""), None);
        assert_eq!(menu_action_for_id("unknown.command"), None);
        assert_eq!(menu_action_for_id("file.open "), None);
    }

    #[test]
    fn capabilities_grant_window_control_actions() {
        // The frontend drives every window control through getCurrentWindow()
        // (services/tauri.ts). Tauri v2 gates each call behind a capability
        // permission; a missing action permission makes the IPC reject and the
        // try/catch swallows it, so the button silently does nothing. The build
        // gates do not catch this, so pin the action permissions the chrome
        // depends on. core:default already covers the read permissions
        // (is-maximized, is-fullscreen, scale-factor, sizes/positions).
        let caps = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("capabilities")
                .join("default.json"),
        )
        .expect("read capabilities/default.json");
        for permission in [
            "core:window:allow-maximize",
            "core:window:allow-unmaximize",
            "core:window:allow-set-fullscreen",
            "core:window:allow-set-size",
            "core:window:allow-set-position",
            "core:window:allow-center",
        ] {
            assert!(
                caps.contains(permission),
                "capabilities/default.json must grant {permission} or the matching window control is a silent no-op"
            );
        }
    }

    // --- #113: drag-drop nil-pasteboard regression guards ---

    #[test]
    fn dropped_paths_empty_input_yields_nothing() {
        // A nil/empty macOS pasteboard now reaches us as an empty path slice
        // (wry nil-safe patch). The handler must produce nothing, not panic.
        let authorized = security::AuthorizedPaths::new();
        assert!(dropped_paths_to_open(&authorized, &[]).is_empty());
    }

    #[test]
    fn dropped_paths_malformed_input_yields_nothing() {
        // Garbage / non-existent payloads (the malformed-pasteboard case) must
        // be dropped silently, never canonicalized, never panic.
        let authorized = security::AuthorizedPaths::new();
        let junk = [
            std::path::PathBuf::from("/no/such/file/anywhere-113"),
            std::path::PathBuf::from(""),
            std::path::PathBuf::from("relative/missing"),
        ];
        assert!(dropped_paths_to_open(&authorized, &junk).is_empty());
    }

    #[test]
    fn dropped_paths_real_file_preserves_open_by_path() {
        // The fix must not weaken the happy path: a real dropped file still
        // canonicalizes through to an authorized path to open by path.
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("dropped-113.txt");
        std::fs::write(&file, b"hi").expect("write");
        let authorized = security::AuthorizedPaths::new();
        let opened = dropped_paths_to_open(&authorized, &[file]);
        assert_eq!(opened.len(), 1);
        assert!(opened[0].ends_with("dropped-113.txt"));
    }

    #[test]
    fn wry_dragdrop_patch_is_pinned() {
        // The actual nil-unwrap panic lives in wry; we fix it via a pinned
        // [patch.crates-io] fork. If this stanza is ever dropped, cargo silently
        // falls back to the panicking crates.io wry and #113 regresses. Lock it.
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("Cargo.toml");
        let toml = std::fs::read_to_string(&manifest).expect("read workspace Cargo.toml");
        let patch = toml
            .split_once("[patch.crates-io]")
            .map(|(_, rest)| rest)
            .expect("[patch.crates-io] stanza present");
        assert!(
            patch.contains("wry") && patch.contains("ibrahemid/wry") && patch.contains("rev"),
            "wry must stay pinned to the nil-safe fork rev (#113)"
        );

        // The manifest stanza is moot if the lock silently falls back to the
        // panicking crates.io wry. Assert the resolved source is the fork.
        let lock = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("Cargo.lock"),
        )
        .expect("read workspace Cargo.lock");
        assert!(
            lock.contains("git+https://github.com/ibrahemid/wry.git"),
            "Cargo.lock must resolve wry to the nil-safe fork, not crates.io (#113)"
        );
    }
}
