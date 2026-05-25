use crate::state::AppState;
use std::time::Instant;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use tracing::{info, warn};
use writ_core::config::HotkeyConfig;
use writ_core::hotkey::{parse_hotkey_chord, HotkeyChord, HotkeyKey, HotkeyMod, HotkeyParseError};

#[derive(Debug, thiserror::Error)]
pub enum HotkeyError {
    #[error("hotkey chord parse failed: {0}")]
    Parse(#[from] HotkeyParseError),
    #[error("hotkey chord contains unsupported key")]
    UnsupportedKey,
}

pub fn chord_from_config(input: &str) -> Result<Shortcut, HotkeyError> {
    let chord = parse_hotkey_chord(input)?;
    chord_to_shortcut(&chord)
}

fn chord_to_shortcut(chord: &HotkeyChord) -> Result<Shortcut, HotkeyError> {
    let mut mods = Modifiers::empty();
    for m in chord.modifiers() {
        mods |= modifier_to_plugin(*m);
    }
    let code = key_to_code(chord.key()).ok_or(HotkeyError::UnsupportedKey)?;
    Ok(Shortcut::new(Some(mods), code))
}

fn modifier_to_plugin(m: HotkeyMod) -> Modifiers {
    match m {
        HotkeyMod::CmdOrCtrl => {
            #[cfg(target_os = "macos")]
            {
                Modifiers::META
            }
            #[cfg(not(target_os = "macos"))]
            {
                Modifiers::CONTROL
            }
        }
        HotkeyMod::Ctrl => Modifiers::CONTROL,
        HotkeyMod::Cmd => Modifiers::META,
        HotkeyMod::Shift => Modifiers::SHIFT,
        HotkeyMod::Alt => Modifiers::ALT,
    }
}

fn key_to_code(key: HotkeyKey) -> Option<Code> {
    Some(match key {
        HotkeyKey::Space => Code::Space,
        HotkeyKey::Enter => Code::Enter,
        HotkeyKey::Escape => Code::Escape,
        HotkeyKey::Tab => Code::Tab,
        HotkeyKey::ArrowUp => Code::ArrowUp,
        HotkeyKey::ArrowDown => Code::ArrowDown,
        HotkeyKey::ArrowLeft => Code::ArrowLeft,
        HotkeyKey::ArrowRight => Code::ArrowRight,
        HotkeyKey::Backquote => Code::Backquote,
        HotkeyKey::Comma => Code::Comma,
        HotkeyKey::Period => Code::Period,
        HotkeyKey::Slash => Code::Slash,
        HotkeyKey::Semicolon => Code::Semicolon,
        HotkeyKey::Quote => Code::Quote,
        HotkeyKey::LeftBracket => Code::BracketLeft,
        HotkeyKey::RightBracket => Code::BracketRight,
        HotkeyKey::Backslash => Code::Backslash,
        HotkeyKey::Minus => Code::Minus,
        HotkeyKey::Equal => Code::Equal,
        HotkeyKey::Digit0 => Code::Digit0,
        HotkeyKey::Digit1 => Code::Digit1,
        HotkeyKey::Digit2 => Code::Digit2,
        HotkeyKey::Digit3 => Code::Digit3,
        HotkeyKey::Digit4 => Code::Digit4,
        HotkeyKey::Digit5 => Code::Digit5,
        HotkeyKey::Digit6 => Code::Digit6,
        HotkeyKey::Digit7 => Code::Digit7,
        HotkeyKey::Digit8 => Code::Digit8,
        HotkeyKey::Digit9 => Code::Digit9,
        HotkeyKey::KeyA => Code::KeyA,
        HotkeyKey::KeyB => Code::KeyB,
        HotkeyKey::KeyC => Code::KeyC,
        HotkeyKey::KeyD => Code::KeyD,
        HotkeyKey::KeyE => Code::KeyE,
        HotkeyKey::KeyF => Code::KeyF,
        HotkeyKey::KeyG => Code::KeyG,
        HotkeyKey::KeyH => Code::KeyH,
        HotkeyKey::KeyI => Code::KeyI,
        HotkeyKey::KeyJ => Code::KeyJ,
        HotkeyKey::KeyK => Code::KeyK,
        HotkeyKey::KeyL => Code::KeyL,
        HotkeyKey::KeyM => Code::KeyM,
        HotkeyKey::KeyN => Code::KeyN,
        HotkeyKey::KeyO => Code::KeyO,
        HotkeyKey::KeyP => Code::KeyP,
        HotkeyKey::KeyQ => Code::KeyQ,
        HotkeyKey::KeyR => Code::KeyR,
        HotkeyKey::KeyS => Code::KeyS,
        HotkeyKey::KeyT => Code::KeyT,
        HotkeyKey::KeyU => Code::KeyU,
        HotkeyKey::KeyV => Code::KeyV,
        HotkeyKey::KeyW => Code::KeyW,
        HotkeyKey::KeyX => Code::KeyX,
        HotkeyKey::KeyY => Code::KeyY,
        HotkeyKey::KeyZ => Code::KeyZ,
        HotkeyKey::F1 => Code::F1,
        HotkeyKey::F2 => Code::F2,
        HotkeyKey::F3 => Code::F3,
        HotkeyKey::F4 => Code::F4,
        HotkeyKey::F5 => Code::F5,
        HotkeyKey::F6 => Code::F6,
        HotkeyKey::F7 => Code::F7,
        HotkeyKey::F8 => Code::F8,
        HotkeyKey::F9 => Code::F9,
        HotkeyKey::F10 => Code::F10,
        HotkeyKey::F11 => Code::F11,
        HotkeyKey::F12 => Code::F12,
    })
}

fn resolve_shortcut(configured: &str) -> Shortcut {
    match chord_from_config(configured) {
        Ok(s) => s,
        Err(e) => {
            let fallback = HotkeyConfig::default().toggle;
            warn!(
                error = %e,
                configured = configured,
                fallback = %fallback,
                "invalid hotkey config; falling back to default"
            );
            chord_from_config(&fallback).expect("default hotkey chord must parse")
        }
    }
}

use crate::events::{emit_event, WritFrontendEvent};
use crate::window_state::{decide_toggle, ToggleAction};

pub fn setup_global_hotkey(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let configured = {
        let state = app.state::<AppState>();
        let cfg = state.config.lock().map_err(|e| e.to_string())?;
        cfg.hotkey.toggle.clone()
    };

    let shortcut = resolve_shortcut(&configured);

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

    info!(chord = %configured, "global hotkey registered");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri_plugin_global_shortcut::{Code, Modifiers};

    #[test]
    fn default_config_chord_parses_successfully() {
        let cfg = HotkeyConfig::default();
        let shortcut = chord_from_config(&cfg.toggle).expect("default chord must parse");
        assert_eq!(shortcut.key, Code::Space);
        assert!(shortcut.mods.contains(Modifiers::SHIFT));

        #[cfg(target_os = "macos")]
        {
            assert!(shortcut.mods.contains(Modifiers::SUPER));
            assert!(!shortcut.mods.contains(Modifiers::CONTROL));
        }
        #[cfg(not(target_os = "macos"))]
        {
            assert!(shortcut.mods.contains(Modifiers::CONTROL));
            assert!(!shortcut.mods.contains(Modifiers::SUPER));
        }
    }

    #[test]
    fn alternate_chord_parses_with_alt_modifier() {
        let shortcut = chord_from_config("CmdOrCtrl+Alt+W").expect("chord must parse");
        assert_eq!(shortcut.key, Code::KeyW);
        assert!(shortcut.mods.contains(Modifiers::ALT));

        #[cfg(target_os = "macos")]
        assert!(shortcut.mods.contains(Modifiers::SUPER));
        #[cfg(not(target_os = "macos"))]
        assert!(shortcut.mods.contains(Modifiers::CONTROL));
    }

    #[test]
    fn literal_ctrl_chord_uses_control_on_every_platform() {
        let shortcut = chord_from_config("Ctrl+Shift+Space").expect("chord must parse");
        assert!(shortcut.mods.contains(Modifiers::CONTROL));
        assert!(!shortcut.mods.contains(Modifiers::SUPER));
        assert!(shortcut.mods.contains(Modifiers::SHIFT));
        assert_eq!(shortcut.key, Code::Space);
    }

    #[test]
    fn literal_cmd_chord_maps_to_super_on_every_platform() {
        let shortcut = chord_from_config("Cmd+Shift+Space").expect("chord must parse");
        assert!(shortcut.mods.contains(Modifiers::SUPER));
        assert!(!shortcut.mods.contains(Modifiers::CONTROL));
        assert_eq!(shortcut.key, Code::Space);
    }

    #[test]
    fn invalid_chord_returns_parse_error() {
        let err = chord_from_config("CmdOrCtrl+Shift").expect_err("must fail");
        assert!(matches!(err, HotkeyError::Parse(_)));
    }

    #[test]
    fn resolve_shortcut_falls_back_to_default_on_parse_error() {
        let shortcut = resolve_shortcut("garbage++");
        assert_eq!(shortcut.key, Code::Space);
        assert!(shortcut.mods.contains(Modifiers::SHIFT));
    }
}
