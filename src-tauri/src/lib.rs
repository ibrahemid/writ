pub mod commands;
pub mod events;
pub mod fts_scheduler;
pub mod hotkey;
pub mod logging;
pub mod poison;
pub mod preview;
pub mod security;
pub mod startup;
pub mod state;
pub mod watcher;
pub mod window_state;

use events::{bus_bridge, emit_event, WritFrontendEvent};
use poison::recover_poison;
use state::AppState;
use tauri::{Listener, Manager};
use tracing::info;

#[cfg(target_os = "macos")]
const MENU_ACTION_IDS: &[&str] = &[
    "app.check_updates",
    "file.open",
    "buffer.new",
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
/// expected to provide, and its `CmdOrCtrl+O/T/W` accelerators are correct
/// there. On Windows/Linux the window runs with `decorations: false`, so this
/// menu would be invisible chrome while its accelerators collide with the
/// platform translator. Every action it exposes (`app.check_updates`,
/// `file.open`, `buffer.new`, `buffer.close`) is also registered as a command
/// palette entry and keyboard shortcut in the frontend, so gating it off those
/// platforms removes dead chrome without removing any reachable action.
#[cfg(target_os = "macos")]
fn build_app_menu(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
    use writ_core::events::bus::WritEvent;

    let check_updates =
        MenuItemBuilder::with_id("app.check_updates", "Check for Updates…").build(app)?;

    let app_menu = SubmenuBuilder::new(app, "Writ")
        .items(&[
            &PredefinedMenuItem::about(app, Some("About Writ"), None)?,
            &PredefinedMenuItem::separator(app)?,
            &check_updates,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, Some("Quit Writ"))?,
        ])
        .build()?;

    let open_file = MenuItemBuilder::with_id("file.open", "Open File…")
        .accelerator("CmdOrCtrl+O")
        .build(app)?;
    let new_tab = MenuItemBuilder::with_id("buffer.new", "New Tab")
        .accelerator("CmdOrCtrl+T")
        .build(app)?;
    let close_tab = MenuItemBuilder::with_id("buffer.close", "Close Tab")
        .accelerator("CmdOrCtrl+W")
        .build(app)?;

    let file_menu = SubmenuBuilder::new(app, "File")
        .items(&[
            &open_file,
            &new_tab,
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
        if let Some(action) = menu_action_for_id(id) {
            let state = app_handle.state::<AppState>();
            state.event_bus.emit(WritEvent::MenuAction {
                action: action.to_string(),
            });
        }
    });

    Ok(())
}

/// Applies the saved window geometry to the (hidden) main window before it is
/// first shown, so the frontend never has to await IPC to resize on the cold
/// path. Sizing always applies; positioning runs the saved rect through
/// [`window_state::place_window`] against the live monitor layout so a window
/// saved on a now-disconnected display re-centers instead of opening
/// off-screen.
fn restore_main_window_geometry(
    window: &tauri::WebviewWindow,
    cfg: &writ_core::config::WindowConfig,
) {
    if cfg.width > 0 && cfg.height > 0 {
        let _ = window.set_size(tauri::LogicalSize::new(
            f64::from(cfg.width),
            f64::from(cfg.height),
        ));
    }

    if let (Some(x), Some(y)) = (cfg.x, cfg.y) {
        let monitors: Vec<window_state::Rect> = window
            .available_monitors()
            .unwrap_or_default()
            .iter()
            .map(|m| {
                let p = m.position();
                let s = m.size();
                window_state::logical_rect(p.x, p.y, s.width, s.height, m.scale_factor())
            })
            .collect();
        let saved = window_state::Rect {
            x,
            y,
            width: cfg.width,
            height: cfg.height,
        };
        match window_state::place_window(saved, &monitors) {
            window_state::WindowPlacement::At { x, y } => {
                let _ =
                    window.set_position(tauri::LogicalPosition::new(f64::from(x), f64::from(y)));
            }
            window_state::WindowPlacement::Center => {
                let _ = window.center();
            }
        }
    }
}

pub fn run() {
    let writ_dir = dirs::home_dir()
        .expect("could not find home directory")
        .join(".writ");
    let logs_dir = writ_dir.join("logs");
    logging::init_logging(&logs_dir);
    logging::panic_handler::install_panic_handler(&logs_dir);

    info!("writ starting");

    let app_state = AppState::initialize().expect("failed to initialize app state");
    let config_path = app_state.writ_dir.join("config.toml");
    let buffers_dir = app_state.buffers_dir.clone();
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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .register_uri_scheme_protocol("writ-preview", preview::handler::serve)
        .manage(app_state)
        .manage(commands::ai::AiState::default())
        .invoke_handler(tauri::generate_handler![
            commands::buffer::create_buffer,
            commands::buffer::get_buffer,
            commands::buffer::save_buffer_content,
            commands::buffer::read_buffer_content,
            commands::buffer::list_active_buffers,
            commands::buffer::close_buffer,
            commands::buffer::close_buffers,
            commands::buffer::delete_buffer,
            commands::buffer::update_tab_order,
            commands::buffer::rename_buffer,
            commands::file::open_file,
            commands::file::open_file_confirmed,
            commands::file::pick_files_to_open,
            commands::file::save_to_source,
            commands::history::list_history,
            commands::history::restore_buffer,
            commands::history::clear_history,
            commands::history::search_buffers,
            commands::config::get_config,
            commands::config::update_config,
            commands::window::toggle_window,
            commands::window::compute_window_placement,
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
            commands::cli::cli_status,
            commands::cli::install_cli,
            commands::default_app::list_default_app_types,
            commands::default_app::get_default_app_status,
            commands::default_app::set_default_app,
            commands::inbox::pick_inbox_folder,
            commands::inbox::clear_inbox,
            commands::inbox::get_inbox_path,
            commands::inbox::list_inbox_files,
            commands::storage::get_storage_info,
            commands::storage::reveal_storage_path,
            commands::ai::ai_rewrite,
            commands::ai::ai_cancel,
            commands::ai::ai_check_connection,
            commands::ai::ai_set_api_key,
            commands::ai::ai_clear_api_key,
            commands::ai::ai_has_api_key,
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
                    std::thread::sleep(std::time::Duration::from_secs(3));
                })
                .await
                .ok();
                if let Some(window) = fallback_handle.get_webview_window("main") {
                    if !window.is_visible().unwrap_or(true) {
                        tracing::warn!(
                            "frontend did not signal first paint; showing window via fallback"
                        );
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
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

            if let Err(e) = hotkey::setup_global_hotkey(&handle) {
                tracing::warn!(error = %e, "failed to register global hotkey");
            }

            let watcher_bus = app.state::<AppState>().event_bus.clone();
            match watcher::handler::start_file_watcher(
                watcher_bus,
                config_path,
                buffers_dir,
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
                let restored_inbox = state
                    .inbox_root
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                if let Some(root) = restored_inbox {
                    match watcher::handler::start_inbox_watcher(state.event_bus.clone(), root) {
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
                std::thread::sleep(std::time::Duration::from_secs(30));
                let snapshot_error = {
                    let state = snapshot_handle.state::<AppState>();
                    state.store.lock().ok().map(|store| {
                        store
                            .collect_buffer_contents()
                            .and_then(|contents| store.write_session_snapshot(&contents, false))
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

    app.run(|app_handle, event| {
        if let tauri::RunEvent::ExitRequested { .. } = &event {
            // Flush deferred FTS reindexes before exit: a reindex still inside
            // its debounce window would otherwise be lost, leaving search stale
            // (the startup consistency check only removes orphan rows, it never
            // adds missing content). ADR-020.
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

            let snapshot_result = {
                let state = app_handle.state::<AppState>();
                state.store.lock().ok().map(|store| {
                    store
                        .collect_buffer_contents()
                        .and_then(|contents| store.write_session_snapshot(&contents, true))
                })
            };
            match snapshot_result {
                Some(Ok(())) => info!("clean-shutdown snapshot written"),
                Some(Err(e)) => tracing::warn!(error = %e, "clean-shutdown snapshot failed"),
                None => {}
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
