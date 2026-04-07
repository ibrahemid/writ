pub mod commands;
pub mod events;
pub mod hotkey;
pub mod logging;
pub mod state;
pub mod watcher;

use state::AppState;
use tracing::info;

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

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
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

            if let Err(e) = hotkey::setup_global_hotkey(&handle) {
                tracing::warn!(error = %e, "failed to register global hotkey");
            }

            if let Err(e) = watcher::handler::start_file_watcher(handle, config_path, buffers_dir, watcher_ignore) {
                tracing::warn!(error = %e, "failed to start file watcher");
            }

            info!("writ ready");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run writ");
}
