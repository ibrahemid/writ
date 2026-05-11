use tauri::{AppHandle, Manager};

use crate::window_state::{decide_toggle, ToggleAction};

#[tauri::command]
pub fn toggle_window(app: AppHandle) -> Result<(), String> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };

    let is_minimized = window.is_minimized().unwrap_or(false);
    let is_visible = window.is_visible().unwrap_or(false);
    let is_focused = window.is_focused().unwrap_or(false);

    match decide_toggle(is_minimized, is_visible, is_focused) {
        ToggleAction::Unminimize => {
            window.unminimize().map_err(|e| e.to_string())?;
            window.show().map_err(|e| e.to_string())?;
            window.set_focus().map_err(|e| e.to_string())?;
        }
        ToggleAction::Show => {
            window.show().map_err(|e| e.to_string())?;
            window.set_focus().map_err(|e| e.to_string())?;
        }
        ToggleAction::Focus => {
            window.set_focus().map_err(|e| e.to_string())?;
        }
        ToggleAction::Hide => {
            window.hide().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
