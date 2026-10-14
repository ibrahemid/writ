pub mod app_menu;
pub mod bus_subscribers;
pub mod commands;
pub mod events;
pub mod first_run;
pub mod fts_scheduler;
pub mod generated;
pub mod heartbeat;
pub mod hotkey;
pub mod instance;
pub mod logging;
pub mod menu;
pub mod navigation;
pub mod notes;
pub mod poison;
pub mod preview;
pub mod quit;
#[cfg(target_os = "macos")]
mod quit_macos;
pub mod relaunch;
pub mod security;
pub mod snap_overlay;
pub mod startup;
pub mod startup_failure;
pub mod state;
pub mod watcher;
pub mod window_state;
pub mod workspace_index;

use events::{emit_event, WritFrontendEvent};
use poison::recover_poison;
use state::AppState;
use tauri::{Listener, Manager};
use tracing::info;
use writ_core::startup::{StartupFailure, StartupStage};

/// Writes everything the next launch needs before the process goes away.
///
/// A live reply is stopped first: its partial text is the streaming task's
/// until that task saves it (ADR-040 section 7).
///
/// Deferred FTS reindexes go first: a reindex still inside its debounce window
/// would otherwise be lost, leaving search stale, because the startup
/// consistency check only removes orphan rows and never adds missing content
/// (ADR-020). The clean marker follows, and it must be the last thing written,
/// so a quit cut short reads as unclean.
///
/// The marker reads no file. Text a save could not write is already in the
/// unsaved-text journal, put there as each save returned or as the frontend
/// handed it over on its way out (ADR-044), so the next launch restores it
/// whatever this does.
pub(crate) fn finish_shutdown(app_handle: &tauri::AppHandle) {
    let state = app_handle.state::<AppState>();

    // A reply still arriving holds its text in the task streaming it, and that
    // text reaches the conversation file only when the task ends. Stopping the
    // replies first and waiting briefly is what turns a quit mid-reply into a
    // stopped reply, rather than a conversation ending on a question.
    commands::chat::stop_live_chats(
        &app_handle.state::<commands::chat::ChatState>(),
        commands::chat::CHAT_SHUTDOWN_BUDGET,
    );

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

    let marked = state.store.lock().ok().map(|store| {
        let kept = store.unsaved_text_count();
        store.mark_session_clean().map(|()| kept)
    });
    match marked {
        Some(Ok(Ok(kept))) if kept > 0 => {
            info!(
                notes = kept,
                "clean-shutdown marker written; unsaved text stays in the journal"
            )
        }
        Some(Ok(_)) => info!("clean-shutdown marker written"),
        Some(Err(e)) => {
            tracing::warn!(error = %e, "the clean-shutdown marker could not be written")
        }
        None => {}
    }
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
/// The generation and the folder are taken per walk, from `root`, so the one
/// that follows reads the folder Writ holds now, including a walk owed by a
/// request the move made while the old walk ran.
pub(crate) fn spawn_notes_reconcile(
    index: std::sync::Arc<writ_storage::notes_index::NotesIndexStore>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    gate: std::sync::Arc<writ_core::watcher::reconcile::ReconcileGate>,
    root: impl Fn() -> std::path::PathBuf + Send + 'static,
    bus: std::sync::Arc<writ_core::events::bus::EventBus>,
) {
    if !gate.request() {
        return;
    }
    std::thread::spawn(move || reconcile_until_settled(&index, &cancel, &gate, &root, &bus));
}

/// The walks [`spawn_notes_reconcile`] runs once it holds the gate: one, then
/// one more for every request that arrived while the last one ran, each over
/// the folder `root` names when it starts.
fn reconcile_until_settled(
    index: &writ_storage::notes_index::NotesIndexStore,
    cancel: &std::sync::atomic::AtomicBool,
    gate: &writ_core::watcher::reconcile::ReconcileGate,
    root: &dyn Fn() -> std::path::PathBuf,
    bus: &writ_core::events::bus::EventBus,
) {
    use std::sync::atomic::Ordering;

    loop {
        let root = root();
        let generation = index.generation();
        let cancelled = || cancel.load(Ordering::Relaxed) || index.generation() != generation;
        let outcome = index.reconcile(&root, &cancelled, &writ_storage::notes_index::is_dataless);
        match outcome {
            Ok(outcome) => {
                info!(
                    added = outcome.added,
                    updated = outcome.updated,
                    removed = outcome.removed,
                    name_only = outcome.skipped_dataless,
                    cancelled = outcome.cancelled,
                    "notes index reconciled"
                );
                commands::note_index::announce_walk(bus, &root, &outcome);
            }
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
    }
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

    // Before the logs, the database or anything else in the folder is
    // touched: a launch that finds another Writ on this folder hands its
    // paths over and ends here, so two processes never write the same notes.
    let instance_lock = instance::claim_or_hand_over(&writ_dir);

    let logs_dir = writ_dir.join("logs");
    // Installed first: setting the hook touches nothing, while opening the
    // log file can fail on the same unwritable directory.
    logging::panic_handler::install_panic_handler(&logs_dir);
    let mut log_guard = logging::init_logging(&logs_dir);

    info!("writ starting");
    let instance_lock = match instance_lock {
        Ok(lock) => Some(lock),
        Err(error) => {
            tracing::warn!(%error, "running without the data folder lock");
            None
        }
    };

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
            log_guard.flush();
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
    let pending_path = writ_storage::pending_clients::path(&app_state.writ_dir);
    let watcher_ignore = app_state.watcher_ignore.clone();

    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    let builder =
        tauri::Builder::default().plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            let args = argv.into_iter().skip(1).map(std::ffi::OsString::from);
            instance::deliver_forwarded(app, &instance::launch_paths(args));
        }));

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    let builder = tauri::Builder::default();

    let builder = builder
        // Refuse any navigation that is not the app origin, the preview scheme,
        // or an inert about: page, so a script cannot set top.location to an
        // external URL and carry note text out in the query string (TS-02).
        .plugin(
            tauri::plugin::Builder::<tauri::Wry>::new("writ-navguard")
                .on_navigation(|webview, url| {
                    navigation::is_allowed_navigation(
                        url.as_str(),
                        navigation::dev_server(webview.config()),
                    )
                })
                .build(),
        )
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        // Registered for the Rust-side `OpenerExt` only. `opener:allow-open-url`
        // stays out of capabilities/default.json so the frontend cannot reach
        // the plugin's own command and skip the link policy (ADR-025).
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .register_asynchronous_uri_scheme_protocol("writ-preview", preview::handler::serve)
        .manage(app_state)
        .manage(commands::ai::AiState::default())
        .manage(commands::ai_connect::ConnectState::default())
        .manage(commands::chat::ChatState::default())
        .manage(commands::materialise::MaterialiseState::default())
        .invoke_handler(tauri::generate_handler![
            commands::buffer::create_buffer,
            commands::buffer::get_buffer,
            commands::buffer::save_buffer_content,
            commands::buffer::restore_note_file,
            commands::buffer::save_note_to,
            commands::buffer::read_buffer_content,
            commands::buffer::note_disk_state,
            commands::buffer::resolve_external_change,
            commands::buffer::record_unsaved_notes,
            commands::buffer::discard_unsaved_text,
            commands::buffer::list_active_buffers,
            commands::buffer::close_buffer,
            commands::buffer::close_buffers,
            commands::buffer::delete_buffer,
            commands::buffer::rename_buffer,
            commands::buffer::set_buffer_language,
            commands::buffer::reopen_with_encoding,
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
            commands::notes::list_notes_dir,
            commands::first_run::first_run_state,
            commands::first_run::dismiss_first_run_hint,
            commands::first_run::finish_first_run,
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
            commands::note_index::note_folder_candidates,
            commands::note_index::note_paths_in_folder,
            commands::note_index::note_backlinks,
            commands::note_index::note_heading_line,
            commands::note_index::note_all_tags,
            commands::note_index::note_paths_for_tag,
            commands::note_index::note_graph,
            commands::note_history::note_versions,
            commands::note_history::note_version_content,
            commands::note_history::restore_note_version,
            commands::note_history::copy_note_version,
            commands::note_history::compare_note_version,
            commands::file::open_file,
            commands::file::open_file_confirmed,
            commands::file::pick_files_to_open,
            commands::file::pick_note_save_path,
            commands::materialise::materialise_note,
            commands::materialise::cancel_materialise_note,
            commands::history::list_history,
            commands::history::restore_buffer,
            commands::history::restore_buffer_confirmed,
            commands::history::clear_history,
            commands::history::search_buffers,
            commands::history::search_notes_by_name,
            commands::config::get_config,
            commands::config::update_config,
            commands::config::get_config_diagnostics,
            commands::command_usage::get_command_usage,
            commands::command_usage::record_command_uses,
            commands::command_usage::clear_command_usage,
            commands::command_usage::prune_command_usage,
            commands::window::confirm_quit_flush,
            commands::window::reveal_window,
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
            commands::preview::preview_inline_image,
            commands::preview::preview_url_prefix,
            commands::preview::preview_get_scripts,
            commands::preview::preview_set_scripts,
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
            hotkey::global_hotkey_status,
            hotkey::set_global_hotkey,
            commands::notices::open_third_party_notices,
            commands::ai::ai_rewrite,
            commands::ai::ai_cancel,
            commands::ai::ai_check_connection,
            commands::ai::ai_set_api_key,
            commands::ai::ai_clear_api_key,
            commands::ai::ai_has_api_key,
            commands::ai::ai_endpoint_state,
            commands::ai::ai_consent_host,
            commands::ai::ai_providers,
            commands::ai::ai_list_models,
            commands::ai::ai_set_provider,
            commands::ai::ai_probe_local,
            commands::ai_connect::ai_openrouter_connect,
            commands::ai_connect::ai_openrouter_cancel,
            commands::chat::chat_state,
            commands::chat::chat_attached_sizes,
            commands::chat::chat_list,
            commands::chat::chat_open,
            commands::chat::chat_new,
            commands::chat::chat_rename,
            commands::chat::chat_delete,
            commands::chat::chat_render_reply,
            commands::chat::chat_send,
            commands::chat::chat_stop,
            commands::chat::chat_apply_proposal,
            commands::chat::chat_discard_proposal,
            commands::activity::activity_recent,
            commands::activity::activity_clear,
            commands::activity::mcp_clients,
            commands::activity::mcp_set_client_permission,
            commands::activity::mcp_forget_client,
            commands::activity::mcp_deny_client,
            commands::activity::mcp_return_to_waiting,
            commands::activity::mcp_restore_client,
            commands::activity::mcp_server_command,
            commands::activity::mcp_tools,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();

            // Every subscriber the bus runs, and the rule they keep: none of
            // them takes the buffer store (`bus_subscribers`).
            bus_subscribers::attach_bus_subscribers(&handle);

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

            // The database is maintained once the window is up, so a launch
            // whose VACUUM takes seconds still shows its window first (TS-35).
            let maintenance_handle = handle.clone();
            std::thread::spawn(move || {
                let state = maintenance_handle.state::<AppState>();
                state.wait_for_reveal(FIRST_PAINT_FALLBACK * 2);
                state.run_startup_maintenance();
            });

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
            app_menu::install(&handle);

            // The Dock, an Apple Event quit and a logout all reach the app
            // through `terminate:`, which never raises an exit request.
            #[cfg(target_os = "macos")]
            quit_macos::install(&handle);

            if let Err(e) = hotkey::setup_global_hotkey(&handle) {
                tracing::warn!(error = %e, "failed to register global hotkey");
            }

            {
                let writ_dir = app.state::<AppState>().writ_dir.clone();
                match instance::start_open_request_watcher(handle.clone(), writ_dir) {
                    Ok(watcher) => {
                        app.manage(watcher);
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "failed to watch for a second launch's files");
                    }
                }
            }

            let watcher_bus = app.state::<AppState>().event_bus.clone();
            match watcher::handler::start_file_watcher(
                watcher_bus,
                config_path,
                pending_path,
                watcher_ignore,
            ) {
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
                    let kind = commands::workspace::workspace_kind(&state, &root);
                    match watcher::handler::start_workspace_watcher(
                        state.event_bus.clone(),
                        root,
                        kind,
                    ) {
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
                let root_handle = app.handle().clone();
                spawn_notes_reconcile(
                    state.notes_index.clone(),
                    state.notes_index_cancel.clone(),
                    state.notes_reconcile.clone(),
                    move || root_handle.state::<AppState>().notes_root(),
                    state.event_bus.clone(),
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

            let heartbeat_handle = handle.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(heartbeat::SNAPSHOT_HEARTBEAT);
                heartbeat::beat(&heartbeat_handle.state::<AppState>());
            });

            if let Err(e) = std::thread::Builder::new()
                .name("writ-update-check".into())
                .spawn(move || commands::update::run_silent_checks(handle))
            {
                tracing::warn!(error = %e, "could not start the update check thread");
            }

            info!("writ ready");
            Ok(())
        });

    // A missing or broken system web view fails here, before any window exists
    // to show it in.
    let app = startup_failure::build_window(|| builder.build(tauri::generate_context!()))
        .unwrap_or_else(|failure| {
            log_guard.flush();
            startup_failure::abort_with_report(&failure, Some(logs_dir.as_path()))
        });

    app.run(move |app_handle, event| {
        // Held by the loop for the life of the process: the event loop ends it
        // with `exit`, which is when the kernel lets the folder go.
        let _ = &instance_lock;
        if let tauri::RunEvent::ExitRequested { api, code, .. } = &event {
            // A restart is not a quit the handshake can hold: `prevent_exit`
            // is a documented no-op for it, so asking the frontend to flush
            // would leave the answer arriving after the runtime had gone.
            // The restart command runs the handshake before it asks
            // (`relaunch::shut_down_for_relaunch`), which leaves nothing to
            // claim here; a restart raised any other way writes what Rust
            // holds and goes through.
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
                    // on disk, one would take the process down mid-write.
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
            // The event loop ends the process with `exit`, which drops
            // nothing: the log lines still queued are written here or lost.
            log_guard.flush();
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
                        info!(
                            folder = %writ_storage::paths::file_name_only(dir_path),
                            "workspace folder opened from OS"
                        );
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
    fn the_quit_item_is_not_forwarded_to_the_frontend() {
        assert_eq!(
            menu::menu_action_for_id(app_menu::QUIT_MENU_ID),
            None,
            "quit must reach the exit path, not the frontend's menu handler"
        );
    }

    #[test]
    fn capabilities_grant_window_control_actions() {
        // The frontend drives every window control through getCurrentWindow()
        // (services/tauri.ts). Tauri v2 gates each call behind a capability
        // permission; a missing action permission makes the IPC reject and the
        // try/catch swallows it, so the button silently does nothing. The build
        // gates do not catch this, so pin the action permissions the chrome
        // depends on. core:window:default covers the read permissions
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

    /// Every permission the main webview holds is one a frontend call needs.
    ///
    /// The only files that reach Tauri from the frontend are
    /// `src/services/tauri.ts` and, for events, `src/services/events.ts`, so a
    /// permission none of their calls needs is one only an injected script
    /// would use. Each grant names the calls that need it; a grant this table
    /// does not know fails, and so does one whose calls are gone.
    #[test]
    fn every_granted_permission_has_a_caller() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let read = |path: std::path::PathBuf| -> String {
            std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
        };
        let caps: serde_json::Value =
            serde_json::from_str(&read(root.join("capabilities").join("default.json")))
                .expect("capabilities/default.json parses");
        let services = root.join("..").join("src").join("services");
        let tauri_ts = read(services.join("tauri.ts"));
        let events_ts = read(services.join("events.ts"));

        let callers: &[(&str, &str, &[&str])] = &[
            ("core:event:default", "events.ts", &["listen(", "emit("]),
            (
                "core:window:default",
                "tauri.ts",
                &[
                    ".isMaximized(",
                    ".isFullscreen(",
                    ".scaleFactor(",
                    ".innerSize(",
                ],
            ),
            ("core:window:allow-hide", "tauri.ts", &[".hide()"]),
            ("core:window:allow-destroy", "tauri.ts", &[".destroy()"]),
            ("core:window:allow-minimize", "tauri.ts", &[".minimize()"]),
            (
                "core:window:allow-unminimize",
                "tauri.ts",
                &[".unminimize()"],
            ),
            ("core:window:allow-maximize", "tauri.ts", &[".maximize()"]),
            (
                "core:window:allow-unmaximize",
                "tauri.ts",
                &[".unmaximize()"],
            ),
            (
                "core:window:allow-set-fullscreen",
                "tauri.ts",
                &[".setFullscreen("],
            ),
            ("core:window:allow-set-size", "tauri.ts", &[".setSize("]),
            (
                "core:window:allow-set-position",
                "tauri.ts",
                &[".setPosition("],
            ),
            ("core:window:allow-center", "tauri.ts", &[".center()"]),
            ("core:window:allow-show", "tauri.ts", &[".show()"]),
            ("core:window:allow-set-focus", "tauri.ts", &[".setFocus()"]),
            (
                "core:window:allow-start-dragging",
                "tauri.ts",
                &[".startDragging()"],
            ),
            (
                "clipboard-manager:allow-read-text",
                "tauri.ts",
                &["readText("],
            ),
            (
                "clipboard-manager:allow-write-text",
                "tauri.ts",
                &["writeText("],
            ),
        ];

        let granted: Vec<&str> = caps["permissions"]
            .as_array()
            .expect("permissions is a list")
            .iter()
            .map(|p| p.as_str().expect("a permission is a string"))
            .collect();
        assert!(!granted.is_empty());
        for permission in granted {
            let Some((_, file, calls)) = callers.iter().find(|(name, _, _)| *name == permission)
            else {
                panic!(
                    "{permission} is granted but no frontend call is recorded as needing it: \
                     drop it, or name the call in services/tauri.ts that needs it here"
                );
            };
            let source = match *file {
                "events.ts" => &events_ts,
                _ => &tauri_ts,
            };
            assert!(
                calls.iter().any(|call| source.contains(call)),
                "{permission} is granted but services/{file} makes none of {calls:?}"
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
    fn wry_resolves_to_the_nil_safe_fork() {
        // wry's collect_paths unwraps a nil pasteboard read that some drag and
        // trackpad gestures produce (#113), so the workspace patches in a fork.
        // Cargo drops a patch that no longer matches the wry version tauri asks
        // for with only a warning and a [[patch.unused]] entry, so this checks
        // what the lock resolved, not what the manifest asks for.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let read_toml = |name: &str| -> toml::Table {
            std::fs::read_to_string(root.join(name))
                .unwrap_or_else(|e| panic!("read workspace {name}: {e}"))
                .parse()
                .unwrap_or_else(|e| panic!("parse workspace {name}: {e}"))
        };

        let manifest = read_toml("Cargo.toml");
        let patch = manifest
            .get("patch")
            .and_then(|p| p.get("crates-io"))
            .and_then(|p| p.get("wry"))
            .expect("Cargo.toml must patch wry under [patch.crates-io] (#113)");
        let git = patch
            .get("git")
            .and_then(toml::Value::as_str)
            .expect("the wry patch must name a git url");
        let rev = patch
            .get("rev")
            .and_then(toml::Value::as_str)
            .expect("the wry patch must pin a rev");
        assert_eq!(git, "https://github.com/ibrahemid/wry.git");
        assert!(
            rev.len() == 40 && rev.bytes().all(|b| b.is_ascii_hexdigit()),
            "the wry patch must pin a full commit sha, got {rev:?}"
        );

        let lock = read_toml("Cargo.lock");
        let unused: Vec<&str> = lock
            .get("patch")
            .and_then(|p| p.get("unused"))
            .and_then(toml::Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|e| e.get("name").and_then(toml::Value::as_str))
                    .collect()
            })
            .unwrap_or_default();
        assert!(
            unused.is_empty(),
            "Cargo.lock lists [[patch.unused]] for {unused:?}: the patch no longer applies"
        );

        let wry: Vec<&toml::Value> = lock
            .get("package")
            .and_then(toml::Value::as_array)
            .expect("Cargo.lock has [[package]] entries")
            .iter()
            .filter(|p| p.get("name").and_then(toml::Value::as_str) == Some("wry"))
            .collect();
        assert_eq!(wry.len(), 1, "Cargo.lock must resolve exactly one wry");
        let source = wry[0].get("source").and_then(toml::Value::as_str);
        assert_eq!(
            source,
            Some(format!("git+{git}?rev={rev}#{rev}").as_str()),
            "Cargo.lock must resolve wry to the pinned fork rev, not crates.io or a local path (#113)"
        );
    }
}

#[cfg(test)]
mod reconcile_tests {
    use super::*;

    use std::sync::atomic::AtomicBool;
    use std::sync::Mutex;

    use writ_core::events::bus::EventBus;
    use writ_core::watcher::reconcile::ReconcileGate;
    use writ_storage::notes_index::NotesIndexStore;

    #[test]
    fn a_walk_owed_while_the_folder_moved_reads_the_folder_writ_holds_now() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let db = dir.path().join("writ.db");
        let conn = writ_storage::database::connection::open_database(&db).expect("open");
        writ_storage::database::migrations::run_migrations(&conn).expect("migrations");
        drop(conn);
        let index = NotesIndexStore::open(&db).expect("index");
        let old = dir.path().join("Old");
        let new = dir.path().join("New");
        std::fs::create_dir_all(&old).expect("old");
        std::fs::create_dir_all(&new).expect("new");
        std::fs::write(new.join("Moved.md"), "here now\n").expect("note");

        let gate = ReconcileGate::new();
        assert!(gate.request(), "the walk under way");
        assert!(!gate.request(), "the move asks for a walk while it runs");

        // The first walk starts against the old folder; the move lands before
        // the walk it is owed starts.
        let roots = Mutex::new(vec![new.clone(), old.clone()]);
        let root = || roots.lock().expect("roots").pop().expect("a root per walk");
        reconcile_until_settled(
            &index,
            &AtomicBool::new(false),
            &gate,
            &root,
            &EventBus::new(),
        );

        let paths = index.note_paths().expect("paths");
        assert!(
            paths.iter().any(|path| path.ends_with("Moved.md")),
            "the owed walk read the new folder: {paths:?}"
        );
        assert!(!gate.is_walking());
    }
}
