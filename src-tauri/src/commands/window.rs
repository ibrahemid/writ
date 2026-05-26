use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::window_state::{decide_toggle, logical_rect, place_window, Rect, ToggleAction, WindowPlacement};

/// A window top-left position in logical pixels, returned to the frontend.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct LogicalWindowPosition {
    pub x: i32,
    pub y: i32,
}

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

/// Resolve a saved window position against the live display layout.
///
/// Returns the (possibly clamped) logical top-left to restore, or `None` when
/// the saved position is unusable on the current monitors and the window should
/// fall back to centering. Monitor enumeration is the only mechanism here; the
/// placement decision lives in [`place_window`].
#[tauri::command]
pub fn compute_window_placement(
    app: AppHandle,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<Option<LogicalWindowPosition>, String> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(None);
    };
    let monitors = window.available_monitors().map_err(|e| e.to_string())?;
    let rects: Vec<Rect> = monitors
        .iter()
        .map(|m| {
            let p = m.position();
            let s = m.size();
            logical_rect(p.x, p.y, s.width, s.height, m.scale_factor())
        })
        .collect();

    let saved = Rect { x, y, width, height };
    Ok(match place_window(saved, &rects) {
        WindowPlacement::At { x, y } => Some(LogicalWindowPosition { x, y }),
        WindowPlacement::Center => None,
    })
}
