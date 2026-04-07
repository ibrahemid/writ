use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use tracing::info;

pub fn setup_global_hotkey(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let shortcut = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::Space);

    app.global_shortcut()
        .on_shortcut(shortcut, move |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                if let Some(window) = app.get_webview_window("main") {
                    let is_visible: bool = window.is_visible().unwrap_or(false);
                    let is_focused: bool = window.is_focused().unwrap_or(false);

                    if is_visible && is_focused {
                        window.hide().ok();
                        info!("window hidden via hotkey");
                    } else if is_visible {
                        window.set_focus().ok();
                        info!("window focused via hotkey");
                    } else {
                        window.show().ok();
                        window.set_focus().ok();
                        info!("window shown via hotkey");
                    }
                }
            }
        })?;

    info!("global hotkey registered");
    Ok(())
}
