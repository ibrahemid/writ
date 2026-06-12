pub mod commands;
pub mod events;
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
use writ_core::events::bus::WritEvent;

const MENU_ACTION_IDS: &[&str] = &[
    "app.check_updates",
    "file.open",
    "buffer.new",
    "buffer.close",
];

fn menu_action_for_id(id: &str) -> Option<&'static str> {
    MENU_ACTION_IDS.iter().copied().find(|&allowed| allowed == id)
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

fn build_app_menu(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};

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
    let builder = tauri::Builder::default().plugin(tauri_plugin_single_instance::init(
        |app, argv, _cwd| {
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
                    info!(count = paths.len(), "files forwarded from secondary instance");
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
        },
    ));

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    let builder = tauri::Builder::default();

    let app = builder
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .register_uri_scheme_protocol("writ-preview", preview::handler::serve)
        .manage(app_state)
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
                    info!(count = drained.len(), "frontend-ready: draining pending opens");
                    if !drained.is_empty() {
                        let _ = emit_event(
                            &ready_handle,
                            WritFrontendEvent::PendingOpens { paths: drained },
                        );
                    }
                });
            }

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
                    let mut slot = recover_poison(
                        state.watcher.lock(),
                        "lib::setup:watcher_handle_stash",
                    );
                    *slot = Some(handle);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "failed to start file watcher");
                }
            }

            tauri::async_runtime::spawn(async move {
                tauri::async_runtime::spawn_blocking(|| {
                    std::thread::sleep(std::time::Duration::from_secs(5));
                })
                .await
                .ok();
                commands::update::run_update_check(handle, false).await;
            });

            info!("writ ready");
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build writ");

    app.run(|app_handle, event| {
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
                    info!(count = paths.len(), "files opened from OS");

                    let ready = state
                        .frontend_ready
                        .load(std::sync::atomic::Ordering::SeqCst);

                    if ready {
                        let _ =
                            emit_event(app_handle, WritFrontendEvent::PendingOpens { paths });
                    } else {
                        let mut pending = recover_poison(
                            state.pending_opens.lock(),
                            "lib::run_event:opened_files",
                        );
                        pending.extend(paths);
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

    #[test]
    fn menu_action_for_id_returns_each_whitelisted_id() {
        for id in MENU_ACTION_IDS {
            assert_eq!(menu_action_for_id(id), Some(*id));
        }
    }

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
