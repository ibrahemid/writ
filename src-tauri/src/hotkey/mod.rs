use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use tracing::info;

use crate::window_state::{decide_toggle, ToggleAction};

pub fn setup_global_hotkey(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let shortcut = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::Space);

    app.global_shortcut()
        .on_shortcut(shortcut, move |app, _shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            let Some(window) = app.get_webview_window("main") else {
                return;
            };

            let is_minimized = window.is_minimized().unwrap_or(false);
            let is_visible = window.is_visible().unwrap_or(false);
            let is_focused = window.is_focused().unwrap_or(false);

            match decide_toggle(is_minimized, is_visible, is_focused) {
                ToggleAction::Unminimize => {
                    window.unminimize().ok();
                    window.show().ok();
                    window.set_focus().ok();
                    info!("window unminimized via hotkey");
                }
                ToggleAction::Show => {
                    window.show().ok();
                    window.set_focus().ok();
                    info!("window shown via hotkey");
                }
                ToggleAction::Focus => {
                    window.set_focus().ok();
                    info!("window focused via hotkey");
                }
                ToggleAction::Hide => {
                    window.hide().ok();
                    info!("window hidden via hotkey");
                }
            }
        })?;

    info!("global hotkey registered");
    Ok(())
}
