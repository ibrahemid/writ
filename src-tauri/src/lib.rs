pub mod commands;
pub mod events;
pub mod hotkey;
pub mod logging;
pub mod state;
pub mod watcher;

use events::{emit_event, WritFrontendEvent};
use state::AppState;
#[cfg(any(target_os = "macos", target_os = "ios"))]
use tauri::Manager;
use tracing::info;

fn build_app_menu(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};

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
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, Some("Quit Writ"))?,
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
        .items(&[&file_menu, &edit_menu, &window_menu])
        .build()?;

    app.set_menu(menu)?;

    app.on_menu_event(move |app_handle, event| {
        let id = event.id().0.as_str();
        match id {
            "file.open" => {
                let _ = emit_event(
                    app_handle,
                    WritFrontendEvent::MenuAction {
                        action: "file.open".to_string(),
                    },
                );
            }
            "buffer.new" => {
                let _ = emit_event(
                    app_handle,
                    WritFrontendEvent::MenuAction {
                        action: "buffer.new".to_string(),
                    },
                );
            }
            "buffer.close" => {
                let _ = emit_event(
                    app_handle,
                    WritFrontendEvent::MenuAction {
                        action: "buffer.close".to_string(),
                    },
                );
            }
            _ => {}
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

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            commands::buffer::create_buffer,
            commands::buffer::get_buffer,
            commands::buffer::save_buffer_content,
            commands::buffer::read_buffer_content,
            commands::buffer::list_active_buffers,
            commands::buffer::close_buffer,
            commands::buffer::delete_buffer,
            commands::buffer::update_tab_order,
            commands::buffer::rename_buffer,
            commands::file::open_file,
            commands::file::consume_pending_opens,
            commands::file::save_to_source,
            commands::history::list_history,
            commands::history::restore_buffer,
            commands::history::clear_history,
            commands::history::search_buffers,
            commands::config::get_config,
            commands::config::update_config,
            commands::window::toggle_window,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();

            if let Err(e) = build_app_menu(app) {
                tracing::warn!(error = %e, "failed to build application menu");
            }

            if let Err(e) = hotkey::setup_global_hotkey(&handle) {
                tracing::warn!(error = %e, "failed to register global hotkey");
            }

            if let Err(e) = watcher::handler::start_file_watcher(
                handle,
                config_path,
                buffers_dir,
                watcher_ignore,
            ) {
                tracing::warn!(error = %e, "failed to start file watcher");
            }

            info!("writ ready");
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build writ");

    app.run(|app_handle, event| {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        if let tauri::RunEvent::Opened { urls } = &event {
            let paths: Vec<String> = urls
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

            if !paths.is_empty() {
                info!(count = paths.len(), "files opened from OS");

                let state = app_handle.state::<AppState>();
                if let Ok(mut pending) = state.pending_opens.lock() {
                    pending.extend(paths);
                }

                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        }

        let _ = (&app_handle, &event);
    });
}
