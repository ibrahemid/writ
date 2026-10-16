use serde::Serialize;
use tauri::{AppHandle, Manager};
use writ_core::title_bar::{title_bar_effect, DoubleClickPreference, TitleBarEffect, WindowState};

use crate::snap_overlay::{self, CaptionButtonMetrics};
use crate::state::AppState;
use crate::window_state::{logical_rect, place_window, Rect, WindowPlacement};

/// A window top-left position in logical pixels, returned to the frontend.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct LogicalWindowPosition {
    pub x: i32,
    pub y: i32,
}

/// Reports that the frontend has written everything it was holding, so the
/// shutdown path can stop waiting and write what is left.
///
/// Safe to call twice, or after the wait has already given up: both leave the
/// exit taking the same course.
#[tauri::command]
pub fn confirm_quit_flush(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.quit.confirm_flush();
    Ok(())
}

/// Brings the window up after the frontend's first paint.
///
/// The frontend does not show the window itself: the timer in setup shows the
/// same window on a launch where this signal is late or never comes, and one
/// claim in [`AppState`] has to settle which of them does it
/// ([`crate::reveal_main_window`]).
#[tauri::command]
pub fn reveal_window(app: AppHandle) {
    crate::reveal_main_window(&app, "frontend first paint");
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

    let saved = Rect {
        x,
        y,
        width,
        height,
    };
    Ok(match place_window(saved, &rects) {
        WindowPlacement::At { x, y } => Some(LogicalWindowPosition { x, y }),
        WindowPlacement::Center => None,
    })
}

/// Reports where the frontend drew the caption maximize button, in logical
/// pixels, so Windows can be given a rect that hit-tests as the maximize button
/// and offers its snap-layout flyout there. A no-op on other platforms.
#[tauri::command]
pub fn set_caption_button_metrics(
    app: AppHandle,
    metrics: CaptionButtonMetrics,
) -> Result<(), String> {
    let metrics = metrics.validate().map_err(|e| e.to_string())?;
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };
    snap_overlay::install(&window, metrics)
}

/// The code an IPC failure of the title-bar command carries.
pub const ERR_TITLE_BAR: &str = "ERR_TITLE_BAR";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TitleBarError {
    #[error("{ERR_TITLE_BAR}: the window state could not be read ({0})")]
    State(String),
    #[error("{ERR_TITLE_BAR}: the window did not change ({0})")]
    Change(String),
}

/// What a title-bar double-click asks of the window. A trait so the command
/// can be tested without one.
pub trait TitleBarWindow {
    fn is_maximized(&self) -> Result<bool, TitleBarError>;
    fn is_fullscreen(&self) -> Result<bool, TitleBarError>;
    fn maximize(&self) -> Result<(), TitleBarError>;
    fn unmaximize(&self) -> Result<(), TitleBarError>;
    fn minimize(&self) -> Result<(), TitleBarError>;
}

impl<R: tauri::Runtime> TitleBarWindow for tauri::WebviewWindow<R> {
    fn is_maximized(&self) -> Result<bool, TitleBarError> {
        tauri::WebviewWindow::is_maximized(self).map_err(|e| TitleBarError::State(e.to_string()))
    }

    fn is_fullscreen(&self) -> Result<bool, TitleBarError> {
        tauri::WebviewWindow::is_fullscreen(self).map_err(|e| TitleBarError::State(e.to_string()))
    }

    fn maximize(&self) -> Result<(), TitleBarError> {
        tauri::WebviewWindow::maximize(self).map_err(|e| TitleBarError::Change(e.to_string()))
    }

    fn unmaximize(&self) -> Result<(), TitleBarError> {
        tauri::WebviewWindow::unmaximize(self).map_err(|e| TitleBarError::Change(e.to_string()))
    }

    fn minimize(&self) -> Result<(), TitleBarError> {
        tauri::WebviewWindow::minimize(self).map_err(|e| TitleBarError::Change(e.to_string()))
    }
}

/// Makes the change `preference` asks of a title-bar double-click on
/// `window`, and answers which change that was.
///
/// No check of `is_maximizable` comes first. On macOS that reads the window's
/// zoom button, which an undecorated window does not have, so it is always
/// false there; Tauri's own drag-region double-click checks it and never
/// maximizes Writ's window.
pub fn title_bar_double_click_inner(
    window: &impl TitleBarWindow,
    preference: &DoubleClickPreference,
) -> Result<TitleBarEffect, TitleBarError> {
    let state = WindowState {
        maximized: window.is_maximized()?,
        fullscreen: window.is_fullscreen()?,
    };
    let effect = title_bar_effect(preference.resolve(), state);
    match effect {
        TitleBarEffect::Maximize => window.maximize()?,
        TitleBarEffect::Unmaximize => window.unmaximize()?,
        TitleBarEffect::Minimize => window.minimize()?,
        TitleBarEffect::Unchanged => {}
    }
    Ok(effect)
}

/// A double-click on an empty part of the window's top chrome, answered the
/// way the system's own title bar answers it (macOS: System Settings,
/// "Double-click a window's title bar to"; Windows and Linux: maximize and
/// restore). Async so the window calls run off the main thread.
#[tauri::command]
pub async fn title_bar_double_click(window: tauri::WebviewWindow) -> Result<(), String> {
    let preference = crate::title_bar::read_double_click_preference();
    title_bar_double_click_inner(&window, &preference)
        .map(|_| ())
        .map_err(|e| e.to_string())
}
