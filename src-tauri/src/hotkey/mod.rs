use std::time::Instant;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use tracing::info;

use crate::events::{emit_event, WritFrontendEvent};
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

            let started = Instant::now();
            let is_minimized = window.is_minimized().unwrap_or(false);
            let is_visible = window.is_visible().unwrap_or(false);
            let is_focused = window.is_focused().unwrap_or(false);
            let action = decide_toggle(is_minimized, is_visible, is_focused);

            match action {
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

            let rust_elapsed_us = started.elapsed().as_micros();
            info!(
                action = ?action,
                rust_elapsed_us = rust_elapsed_us as u64,
                "hotkey handler complete"
            );

            if matches!(
                action,
                ToggleAction::Show | ToggleAction::Unminimize | ToggleAction::Focus
            ) {
                let _ = emit_event(
                    app,
                    WritFrontendEvent::WindowShown {
                        rust_elapsed_us: rust_elapsed_us as u64,
                    },
                );
            }
        })?;

    info!("global hotkey registered");
    Ok(())
}
