use crate::state::AppState;
use serde::Serialize;
use std::sync::Mutex;
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

/// What became of the chord that shows and hides the window.
///
/// `registered: false` is the case the settings surface has to show: the OS
/// handed the chord to another app first, so the key does nothing here and
/// saying nothing would read as Writ being broken.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GlobalHotkeyStatus {
    pub chord: String,
    pub registered: bool,
}

/// The last answer the OS gave. Module state rather than a field on
/// [`AppState`]: it is written by the one function that asks the OS, and read
/// by the one command that reports it.
static STATUS: Mutex<Option<GlobalHotkeyStatus>> = Mutex::new(None);

fn record_status(status: GlobalHotkeyStatus) {
    match STATUS.lock() {
        Ok(mut held) => *held = Some(status),
        Err(poisoned) => *poisoned.into_inner() = Some(status),
    }
}

fn held_status() -> Option<GlobalHotkeyStatus> {
    match STATUS.lock() {
        Ok(held) => held.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

/// Registers `chord` as the toggle, replacing whatever was registered before,
/// and records what the OS said.
///
/// A refusal is not an error to the caller: the app runs perfectly well with
/// the chord taken, and the one thing that must happen is that the surface
/// offering a rebind is told.
fn register_toggle(app: &AppHandle, chord: &str) -> GlobalHotkeyStatus {
    let shortcut = resolve_shortcut(chord);

    // Writ registers this one chord, so clearing them all is clearing the
    // previous toggle, and it must happen before the new one is asked for:
    // rebinding to the chord already held would otherwise be refused as a
    // duplicate.
    if let Err(e) = app.global_shortcut().unregister_all() {
        warn!(error = %e, "could not release the previous global hotkey");
    }

    let outcome = app
        .global_shortcut()
        .on_shortcut(shortcut, move |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                toggle_window(app);
            }
        });

    let registered = match outcome {
        Ok(()) => {
            info!(chord = %chord, "global hotkey registered");
            true
        }
        Err(e) => {
            warn!(error = %e, chord = %chord, "global hotkey is taken by another app");
            false
        }
    };

    let status = GlobalHotkeyStatus {
        chord: chord.to_string(),
        registered,
    };
    record_status(status.clone());
    let _ = emit_event(app, WritFrontendEvent::HotkeyStatus(status.clone()));
    status
}

/// The toggle itself, lifted out of the registration so re-registering builds
/// a fresh handler without a second copy of the behaviour.
fn toggle_window(app: &AppHandle) {
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
            crate::note_window_dismissed(app);
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
}

/// IPC: what became of the toggle chord.
///
/// Nothing recorded means startup has not asked the OS yet, and an unasked
/// chord is not a taken one, so the configured chord is reported as held.
#[tauri::command]
pub fn global_hotkey_status(
    state: tauri::State<'_, AppState>,
) -> Result<GlobalHotkeyStatus, String> {
    if let Some(status) = held_status() {
        return Ok(status);
    }
    let chord = state
        .config
        .lock()
        .map_err(|e| e.to_string())?
        .hotkey
        .toggle
        .clone();
    Ok(GlobalHotkeyStatus {
        chord,
        registered: true,
    })
}

/// IPC: takes the chord the shortcut editor recorded and asks the OS for it,
/// answering whether it was given.
#[tauri::command]
pub fn set_global_hotkey(app: AppHandle, chord: String) -> Result<GlobalHotkeyStatus, String> {
    chord_from_config(&chord).map_err(|e| e.to_string())?;
    Ok(register_toggle(&app, &chord))
}

pub fn setup_global_hotkey(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let configured = {
        let state = app.state::<AppState>();
        let cfg = state.config.lock().map_err(|e| e.to_string())?;
        cfg.hotkey.toggle.clone()
    };

    register_toggle(app, &configured);
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
    fn a_taken_chord_is_recorded_as_taken_and_read_back() {
        record_status(GlobalHotkeyStatus {
            chord: "CmdOrCtrl+Shift+Space".to_string(),
            registered: false,
        });

        let held = held_status().expect("a recorded status must read back");
        assert_eq!(held.chord, "CmdOrCtrl+Shift+Space");
        assert!(!held.registered, "the chord another app holds is not ours");

        record_status(GlobalHotkeyStatus {
            chord: "CmdOrCtrl+Shift+Space".to_string(),
            registered: true,
        });
        assert!(held_status().expect("still recorded").registered);
    }

    #[test]
    fn the_status_reaches_the_frontend_under_its_own_event_name() {
        let event = crate::events::WritFrontendEvent::HotkeyStatus(GlobalHotkeyStatus {
            chord: "CmdOrCtrl+Shift+Space".to_string(),
            registered: false,
        });
        let json = serde_json::to_value(&event).expect("the event must serialize");
        assert_eq!(json["kind"], "hotkey:status");
        assert_eq!(json["payload"]["chord"], "CmdOrCtrl+Shift+Space");
        assert_eq!(json["payload"]["registered"], false);
    }

    #[test]
    fn a_chord_the_editor_could_record_is_accepted_and_a_broken_one_is_not() {
        assert!(chord_from_config("CmdOrCtrl+Alt+Space").is_ok());
        assert!(chord_from_config("CmdOrCtrl+Shift").is_err());
    }

    #[test]
    fn resolve_shortcut_falls_back_to_default_on_parse_error() {
        let shortcut = resolve_shortcut("garbage++");
        assert_eq!(shortcut.key, Code::Space);
        assert!(shortcut.mods.contains(Modifiers::SHIFT));
    }
}
