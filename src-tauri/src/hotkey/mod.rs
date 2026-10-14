use crate::state::AppState;
use serde::Serialize;
use std::sync::Mutex;
use std::time::Instant;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use tracing::{info, warn};
use writ_core::config::HotkeyConfig;
use writ_core::hotkey::{
    check_global_chord, is_wayland_session, parse_hotkey_chord, GlobalChordError, HotkeyChord,
    HotkeyKey, HotkeyMod, HotkeyParseError,
};

#[derive(Debug, thiserror::Error)]
pub enum HotkeyError {
    #[error("hotkey chord parse failed: {0}")]
    Parse(#[from] HotkeyParseError),
    #[error("{0}")]
    Global(#[from] GlobalChordError),
    #[error("hotkey chord contains unsupported key")]
    UnsupportedKey,
    #[error("{0} already uses this shortcut")]
    HeldByOther(&'static str),
}

/// Which of Writ's two chords that work from any app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalChord {
    /// Shows and hides the window.
    Window,
    /// Opens the quick capture note (ADR-055).
    Capture,
}

impl GlobalChord {
    fn slot(self) -> usize {
        match self {
            GlobalChord::Window => 0,
            GlobalChord::Capture => 1,
        }
    }

    fn other(self) -> Self {
        match self {
            GlobalChord::Window => GlobalChord::Capture,
            GlobalChord::Capture => GlobalChord::Window,
        }
    }

    /// The chord Writ ships for this role.
    pub fn shipped(self) -> String {
        let shipped = HotkeyConfig::default();
        match self {
            GlobalChord::Window => shipped.toggle,
            GlobalChord::Capture => shipped.capture,
        }
    }

    /// What the shortcut editor calls the row, for a refusal naming it.
    fn row_name(self) -> &'static str {
        match self {
            GlobalChord::Window => "Show and hide Writ",
            GlobalChord::Capture => "Quick capture",
        }
    }

    fn configured(self, hotkey: &HotkeyConfig) -> String {
        match self {
            GlobalChord::Window => hotkey.toggle.clone(),
            GlobalChord::Capture => hotkey.capture.clone(),
        }
    }

    fn status_event(self, status: GlobalHotkeyStatus) -> WritFrontendEvent {
        match self {
            GlobalChord::Window => WritFrontendEvent::HotkeyStatus(status),
            GlobalChord::Capture => WritFrontendEvent::CaptureHotkeyStatus(status),
        }
    }
}

/// The shortcut the OS is asked for, from a chord as config spells it. A
/// chord that would take a key from every app is refused here, so neither the
/// shortcut editor nor a hand edit of config.toml can register one.
pub fn chord_from_config(input: &str) -> Result<Shortcut, HotkeyError> {
    let chord = parse_hotkey_chord(input)?;
    check_global_chord(&chord)?;
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
        HotkeyKey::F13 => Code::F13,
        HotkeyKey::F14 => Code::F14,
        HotkeyKey::F15 => Code::F15,
        HotkeyKey::F16 => Code::F16,
        HotkeyKey::F17 => Code::F17,
        HotkeyKey::F18 => Code::F18,
        HotkeyKey::F19 => Code::F19,
        HotkeyKey::F20 => Code::F20,
        HotkeyKey::F21 | HotkeyKey::F22 | HotkeyKey::F23 | HotkeyKey::F24 => {
            return function_key_past_f20(key);
        }
    })
}

/// Carbon has no key codes past F20, so macOS cannot register these.
#[cfg(target_os = "macos")]
fn function_key_past_f20(_key: HotkeyKey) -> Option<Code> {
    None
}

#[cfg(not(target_os = "macos"))]
fn function_key_past_f20(key: HotkeyKey) -> Option<Code> {
    match key {
        HotkeyKey::F21 => Some(Code::F21),
        HotkeyKey::F22 => Some(Code::F22),
        HotkeyKey::F23 => Some(Code::F23),
        HotkeyKey::F24 => Some(Code::F24),
        _ => None,
    }
}

/// The shortcut to register, and the chord it actually stands for.
///
/// A chord that does not parse falls back to the default, and the caller is
/// given the fallback rather than what it asked for: reporting the asked-for
/// chord would show the settings surface a chord that is held by nothing.
fn resolve_chord(role: GlobalChord, configured: &str) -> (Shortcut, String) {
    match chord_from_config(configured) {
        Ok(s) => (s, configured.to_string()),
        Err(e) => {
            let fallback = role.shipped();
            warn!(
                error = %e,
                configured = configured,
                fallback = %fallback,
                "invalid hotkey config; falling back to default"
            );
            let shortcut = chord_from_config(&fallback).expect("default hotkey chord must parse");
            (shortcut, fallback)
        }
    }
}

use crate::events::{emit_event, WritFrontendEvent};
use crate::window_state::{decide_toggle, ToggleAction};

/// What became of the chord that shows and hides the window.
///
/// `registered: false` is the case the settings surface has to show: the OS
/// handed the chord to another app first, so the key does nothing here and
/// saying nothing would read as Writ being broken. `unsupported` is the
/// session itself: a Wayland desktop gives an X11 key grab only the keys typed
/// into other X11 windows, so the chord does nothing from a native app
/// whatever the registration said.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GlobalHotkeyStatus {
    pub chord: String,
    pub registered: bool,
    pub unsupported: bool,
}

/// What a request to change the chord came to.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HotkeyRebind {
    /// The chord that shows and hides the window now.
    pub status: GlobalHotkeyStatus,
    /// The OS gave the chord asked for to another app first. `status` is then
    /// the chord that was there before, still held.
    pub refused: bool,
}

/// The toggle as the OS last answered for it.
#[derive(Debug, Clone, PartialEq)]
struct Held {
    status: GlobalHotkeyStatus,
    /// The shortcut the OS holds for Writ, while it holds one.
    shortcut: Option<Shortcut>,
}

/// The last answer the OS gave for each [`GlobalChord`]. Module state rather
/// than a field on [`AppState`]: it is written by the one function that asks
/// the OS, and read by the commands that report it.
static HELD: [Mutex<Option<Held>>; 2] = [Mutex::new(None), Mutex::new(None)];

fn lock_held(role: GlobalChord) -> std::sync::MutexGuard<'static, Option<Held>> {
    match HELD[role.slot()].lock() {
        Ok(held) => held,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn held_status(role: GlobalChord) -> Option<GlobalHotkeyStatus> {
    lock_held(role).as_ref().map(|held| held.status.clone())
}

/// Whether the OS holds the capture chord for Writ now.
pub fn capture_chord_is_held() -> bool {
    held_status(GlobalChord::Capture).is_some_and(|status| status.registered)
}

/// What changing the toggle asks of the OS. A trait so the order of the asks
/// can be tested without a desktop to ask.
trait ShortcutRegistrar {
    fn register(&self, shortcut: Shortcut) -> Result<(), tauri_plugin_global_shortcut::Error>;
    fn unregister(&self, shortcut: Shortcut) -> Result<(), tauri_plugin_global_shortcut::Error>;
}

struct OsRegistrar<'a>(&'a AppHandle, GlobalChord);

impl ShortcutRegistrar for OsRegistrar<'_> {
    fn register(&self, shortcut: Shortcut) -> Result<(), tauri_plugin_global_shortcut::Error> {
        let role = self.1;
        self.0
            .global_shortcut()
            .on_shortcut(shortcut, move |app, _shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    match role {
                        GlobalChord::Window => toggle_window(app),
                        GlobalChord::Capture => crate::capture::on_chord(app),
                    }
                }
            })
    }

    fn unregister(&self, shortcut: Shortcut) -> Result<(), tauri_plugin_global_shortcut::Error> {
        self.0.global_shortcut().unregister(shortcut)
    }
}

/// Asks the OS for `shortcut` and settles what the toggle is afterwards.
///
/// The new chord is registered before the old one is released, so a chord
/// another app holds costs nothing: the old one keeps working and the answer
/// says the new one was refused. A chord Writ already holds is not asked for
/// again, since the OS would refuse it as a duplicate. With nothing held
/// before, which is startup, a refusal is the chord being taken.
fn rebind(
    os: &impl ShortcutRegistrar,
    previous: Option<&Held>,
    chord: String,
    shortcut: Shortcut,
    unsupported: bool,
) -> Rebound {
    let held_now = |chord: String| Held {
        status: GlobalHotkeyStatus {
            chord,
            registered: true,
            unsupported,
        },
        shortcut: Some(shortcut),
    };

    if previous.is_some_and(|held| held.shortcut == Some(shortcut)) {
        return Rebound {
            held: held_now(chord),
            refused: false,
        };
    }

    match os.register(shortcut) {
        Ok(()) => {
            info!(chord = %chord, "global hotkey registered");
            if let Some(old) = previous.and_then(|held| held.shortcut) {
                if let Err(e) = os.unregister(old) {
                    warn!(error = %e, "could not release the previous global hotkey");
                }
            }
            Rebound {
                held: held_now(chord),
                refused: false,
            }
        }
        Err(e) => {
            warn!(error = %e, chord = %chord, "global hotkey is taken by another app");
            match previous {
                Some(held) => Rebound {
                    held: held.clone(),
                    refused: true,
                },
                None => Rebound {
                    held: Held {
                        status: GlobalHotkeyStatus {
                            chord,
                            registered: false,
                            unsupported,
                        },
                        shortcut: None,
                    },
                    refused: false,
                },
            }
        }
    }
}

/// [`rebind`]'s answer before it is recorded.
#[derive(Debug, Clone, PartialEq)]
struct Rebound {
    held: Held,
    refused: bool,
}

/// Whether this session can deliver a chord from other apps at all.
fn session_lacks_global_shortcuts() -> bool {
    cfg!(target_os = "linux")
        && is_wayland_session(
            std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
            std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
        )
}

/// Whether `shortcut` is the one the other role holds. The OS would turn the
/// second registration down as a duplicate, and the editor would then say
/// another app holds it.
fn held_by_other(role: GlobalChord, shortcut: Shortcut) -> bool {
    lock_held(role.other())
        .as_ref()
        .is_some_and(|held| held.shortcut == Some(shortcut))
}

/// What turning a role off leaves: nothing held, and the empty chord.
fn released(os: &impl ShortcutRegistrar, previous: Option<&Held>, unsupported: bool) -> Held {
    if let Some(old) = previous.and_then(|held| held.shortcut) {
        if let Err(e) = os.unregister(old) {
            warn!(error = %e, "could not release a global hotkey");
        }
    }
    Held {
        status: GlobalHotkeyStatus {
            chord: String::new(),
            registered: false,
            unsupported,
        },
        shortcut: None,
    }
}

/// What asking for `chord` as `role` comes to, given what `role` holds now
/// and what the other role holds. Only the capture chord can be empty, which
/// releases it.
///
/// A chord the other role holds is turned down here rather than asked for:
/// the OS would answer it as a duplicate, and the editor would then say
/// another app holds it.
fn settle(
    os: &impl ShortcutRegistrar,
    role: GlobalChord,
    previous: Option<&Held>,
    other: Option<&Held>,
    chord: &str,
    unsupported: bool,
) -> Rebound {
    if role == GlobalChord::Capture && chord.trim().is_empty() {
        return Rebound {
            held: released(os, previous, unsupported),
            refused: false,
        };
    }
    let (shortcut, chord) = resolve_chord(role, chord);
    if other.is_some_and(|held| held.shortcut == Some(shortcut)) {
        warn!(chord = %chord, "both global chords name the same shortcut");
        return Rebound {
            held: previous.cloned().unwrap_or(Held {
                status: GlobalHotkeyStatus {
                    chord,
                    registered: false,
                    unsupported,
                },
                shortcut: None,
            }),
            refused: true,
        };
    }
    rebind(os, previous, chord, shortcut, unsupported)
}

/// Registers `chord` for `role` through [`settle`], records what the OS said,
/// and tells the frontend.
///
/// A refusal is not an error to the caller: the app runs perfectly well with
/// the chord taken, and the one thing that must happen is that the surface
/// offering a rebind is told.
fn register_global(app: &AppHandle, role: GlobalChord, chord: &str) -> HotkeyRebind {
    // Copied out before this role's own lock is taken, so the two locks are
    // never held together.
    let other = lock_held(role.other()).clone();
    let mut held = lock_held(role);
    let answer = settle(
        &OsRegistrar(app, role),
        role,
        held.as_ref(),
        other.as_ref(),
        chord,
        session_lacks_global_shortcuts(),
    );
    *held = Some(answer.held.clone());
    drop(held);

    let status = answer.held.status;
    let _ = emit_event(app, role.status_event(status.clone()));
    HotkeyRebind {
        status,
        refused: answer.refused,
    }
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
    status_of(&state, GlobalChord::Window)
}

/// IPC: what became of the capture chord, as [`global_hotkey_status`] answers
/// for the window's.
#[tauri::command]
pub fn capture_hotkey_status(
    state: tauri::State<'_, AppState>,
) -> Result<GlobalHotkeyStatus, String> {
    status_of(&state, GlobalChord::Capture)
}

fn status_of(state: &AppState, role: GlobalChord) -> Result<GlobalHotkeyStatus, String> {
    if let Some(status) = held_status(role) {
        return Ok(status);
    }
    let chord = role.configured(&state.config.lock().map_err(|e| e.to_string())?.hotkey);
    Ok(GlobalHotkeyStatus {
        registered: !chord.is_empty(),
        chord,
        unsupported: session_lacks_global_shortcuts(),
    })
}

/// IPC: takes the chord the shortcut editor recorded and asks the OS for it,
/// answering whether it was given. A chord that cannot be held for every app
/// is refused before the OS is asked, with the rule as the error.
#[tauri::command]
pub fn set_global_hotkey(app: AppHandle, chord: String) -> Result<HotkeyRebind, String> {
    set_global(&app, GlobalChord::Window, &chord)
}

/// IPC: [`set_global_hotkey`] for the capture chord. An empty chord turns
/// capture off.
#[tauri::command]
pub fn set_capture_hotkey(app: AppHandle, chord: String) -> Result<HotkeyRebind, String> {
    set_global(&app, GlobalChord::Capture, &chord)
}

fn set_global(app: &AppHandle, role: GlobalChord, chord: &str) -> Result<HotkeyRebind, String> {
    if !(role == GlobalChord::Capture && chord.trim().is_empty()) {
        let shortcut = chord_from_config(chord).map_err(|e| e.to_string())?;
        if held_by_other(role, shortcut) {
            return Err(HotkeyError::HeldByOther(role.other().row_name()).to_string());
        }
    }
    Ok(register_global(app, role, chord))
}

/// Registers both chords at startup, the window's first. Each is asked for on
/// its own, so one another app holds leaves the other working.
pub fn setup_global_hotkey(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let hotkey = {
        let state = app.state::<AppState>();
        let cfg = state.config.lock().map_err(|e| e.to_string())?;
        cfg.hotkey.clone()
    };

    for role in [GlobalChord::Window, GlobalChord::Capture] {
        register_global(app, role, &role.configured(&hotkey));
    }
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

    /// The chord editor's window row and a hand edit of config.toml both end
    /// here. Before the rule, "A" registered and took the letter from every
    /// app.
    #[test]
    fn a_bare_key_chord_is_refused_with_the_rule() {
        for chord in ["A", "Space", "Shift+A", "F2"] {
            let err = chord_from_config(chord).expect_err("a bare key must be refused");
            assert!(matches!(
                err,
                HotkeyError::Global(GlobalChordError::NeedsModifier)
            ));
            assert!(err.to_string().contains("Cmd, Ctrl or Alt"), "{err}");
        }
    }

    #[test]
    fn f13_to_f20_register_on_their_own() {
        let shortcut = chord_from_config("F13").expect("F13 types nothing");
        assert_eq!(shortcut.key, Code::F13);
        assert_eq!(chord_from_config("F20").expect("F20").key, Code::F20);
    }

    #[test]
    fn f21_to_f24_reach_the_os_only_where_it_has_them() {
        let answer = chord_from_config("F21");
        #[cfg(target_os = "macos")]
        assert!(matches!(answer, Err(HotkeyError::UnsupportedKey)));
        #[cfg(not(target_os = "macos"))]
        assert_eq!(answer.expect("F21").key, Code::F21);
    }

    #[test]
    fn a_hand_edited_bare_key_falls_back_to_the_shipped_chord() {
        let (shortcut, chord) = resolve_chord(GlobalChord::Window, "A");
        assert_eq!(shortcut.key, Code::Space);
        assert_eq!(chord, HotkeyConfig::default().toggle);
    }

    #[test]
    fn the_status_reaches_the_frontend_under_its_own_event_name() {
        let event = crate::events::WritFrontendEvent::HotkeyStatus(GlobalHotkeyStatus {
            chord: "CmdOrCtrl+Shift+Space".to_string(),
            registered: false,
            unsupported: true,
        });
        let json = serde_json::to_value(&event).expect("the event must serialize");
        assert_eq!(json["kind"], "hotkey:status");
        assert_eq!(json["payload"]["chord"], "CmdOrCtrl+Shift+Space");
        assert_eq!(json["payload"]["registered"], false);
        assert_eq!(json["payload"]["unsupported"], true);
    }

    #[test]
    fn a_chord_the_editor_could_record_is_accepted_and_a_broken_one_is_not() {
        assert!(chord_from_config("CmdOrCtrl+Alt+Space").is_ok());
        assert!(chord_from_config("CmdOrCtrl+Shift").is_err());
    }

    #[test]
    fn keys_the_recorder_can_capture_but_the_parser_cannot_read_are_refused() {
        // `ShortcutRecorder` sends `event.key` through verbatim, so these do
        // reach `set_global_hotkey`. It rejects them before anything is
        // registered, and the shortcut editor's window row says why.
        for chord in [
            "CmdOrCtrl+Shift+Backspace",
            "CmdOrCtrl+F25",
            "CmdOrCtrl+Shift+Home",
            "CmdOrCtrl+Shift+Delete",
        ] {
            assert!(
                chord_from_config(chord).is_err(),
                "{chord} must not reach the OS"
            );
        }
    }

    /// Records each ask, and refuses the shortcuts it was told another app
    /// holds.
    #[derive(Default)]
    struct FakeOs {
        taken: Vec<Shortcut>,
        asks: std::cell::RefCell<Vec<String>>,
    }

    impl ShortcutRegistrar for FakeOs {
        fn register(&self, shortcut: Shortcut) -> Result<(), tauri_plugin_global_shortcut::Error> {
            self.asks
                .borrow_mut()
                .push(format!("register {}", shortcut.into_string()));
            if self.taken.contains(&shortcut) {
                // The variant Carbon's refusal arrives as.
                return Err(tauri_plugin_global_shortcut::Error::GlobalHotkey(
                    "HotKey already registered".into(),
                ));
            }
            Ok(())
        }

        fn unregister(
            &self,
            shortcut: Shortcut,
        ) -> Result<(), tauri_plugin_global_shortcut::Error> {
            self.asks
                .borrow_mut()
                .push(format!("unregister {}", shortcut.into_string()));
            Ok(())
        }
    }

    fn shortcut(chord: &str) -> Shortcut {
        chord_from_config(chord).expect("a chord the tests can register")
    }

    fn held(chord: &str) -> Held {
        Held {
            status: GlobalHotkeyStatus {
                chord: chord.to_string(),
                registered: true,
                unsupported: false,
            },
            shortcut: Some(shortcut(chord)),
        }
    }

    #[test]
    fn startup_records_a_chord_another_app_holds_as_taken() {
        let os = FakeOs {
            taken: vec![shortcut("CmdOrCtrl+Shift+Space")],
            ..FakeOs::default()
        };
        let answer = rebind(
            &os,
            None,
            "CmdOrCtrl+Shift+Space".to_string(),
            shortcut("CmdOrCtrl+Shift+Space"),
            false,
        );
        assert!(!answer.held.status.registered);
        assert_eq!(answer.held.status.chord, "CmdOrCtrl+Shift+Space");
        assert_eq!(answer.held.shortcut, None);
        assert!(!answer.refused, "startup has no chord to keep");
    }

    #[test]
    fn a_rebind_registers_the_new_chord_before_releasing_the_old() {
        let os = FakeOs::default();
        let before = held("CmdOrCtrl+Shift+Space");
        let answer = rebind(
            &os,
            Some(&before),
            "Ctrl+Alt+Space".to_string(),
            shortcut("Ctrl+Alt+Space"),
            false,
        );
        let asks = os.asks.borrow();
        assert_eq!(asks.len(), 2);
        assert!(asks[0].starts_with("register "), "{asks:?}");
        assert!(asks[1].starts_with("unregister "), "{asks:?}");
        assert_eq!(answer.held.status.chord, "Ctrl+Alt+Space");
        assert!(answer.held.status.registered);
        assert!(!answer.refused);
    }

    /// Before, the old chord was released first, so a refused rebind left
    /// Writ with no chord at all until the next launch.
    #[test]
    fn a_failed_rebind_keeps_the_old_chord() {
        let os = FakeOs {
            taken: vec![shortcut("Ctrl+Alt+Space")],
            ..FakeOs::default()
        };
        let before = held("CmdOrCtrl+Shift+Space");
        let answer = rebind(
            &os,
            Some(&before),
            "Ctrl+Alt+Space".to_string(),
            shortcut("Ctrl+Alt+Space"),
            false,
        );
        assert!(answer.refused);
        assert_eq!(answer.held, before, "the chord that worked still does");
        assert!(
            os.asks
                .borrow()
                .iter()
                .all(|ask| !ask.starts_with("unregister")),
            "nothing is released when the new chord is refused"
        );
    }

    #[test]
    fn a_chord_already_held_is_not_asked_for_again() {
        let os = FakeOs::default();
        let before = held("CmdOrCtrl+Shift+Space");
        let answer = rebind(
            &os,
            Some(&before),
            "CmdOrCtrl+Shift+Space".to_string(),
            shortcut("CmdOrCtrl+Shift+Space"),
            false,
        );
        assert!(os.asks.borrow().is_empty());
        assert!(answer.held.status.registered);
        assert!(!answer.refused);
    }

    #[test]
    fn a_taken_chord_is_asked_for_again() {
        let os = FakeOs::default();
        let before = Held {
            status: GlobalHotkeyStatus {
                chord: "CmdOrCtrl+Shift+Space".to_string(),
                registered: false,
                unsupported: false,
            },
            shortcut: None,
        };
        let answer = rebind(
            &os,
            Some(&before),
            "CmdOrCtrl+Shift+Space".to_string(),
            shortcut("CmdOrCtrl+Shift+Space"),
            false,
        );
        assert_eq!(
            os.asks.borrow().len(),
            1,
            "registered, and nothing to release"
        );
        assert!(answer.held.status.registered);
    }

    #[test]
    fn a_wayland_session_is_reported_whatever_the_registration_said() {
        let answer = rebind(
            &FakeOs::default(),
            None,
            "CmdOrCtrl+Shift+Space".to_string(),
            shortcut("CmdOrCtrl+Shift+Space"),
            true,
        );
        assert!(answer.held.status.unsupported);
    }

    #[test]
    fn a_hand_edited_capture_chord_falls_back_to_the_shipped_capture_chord() {
        let (shortcut, chord) = resolve_chord(GlobalChord::Capture, "A");
        assert_eq!(chord, HotkeyConfig::default().capture);
        assert_eq!(shortcut, self::shortcut("CmdOrCtrl+Shift+Alt+Space"));
    }

    #[test]
    fn the_capture_chord_registers_while_another_app_holds_the_window_chord() {
        let os = FakeOs {
            taken: vec![shortcut("CmdOrCtrl+Shift+Space")],
            ..FakeOs::default()
        };
        let window = settle(
            &os,
            GlobalChord::Window,
            None,
            None,
            "CmdOrCtrl+Shift+Space",
            false,
        );
        assert!(!window.held.status.registered);
        let capture = settle(
            &os,
            GlobalChord::Capture,
            None,
            Some(&window.held),
            "CmdOrCtrl+Shift+Alt+Space",
            false,
        );
        assert!(capture.held.status.registered);
        assert_eq!(
            capture.held.shortcut,
            Some(shortcut("CmdOrCtrl+Shift+Alt+Space"))
        );
    }

    #[test]
    fn a_taken_capture_chord_leaves_the_window_chord_alone() {
        let os = FakeOs {
            taken: vec![shortcut("CmdOrCtrl+Shift+Alt+Space")],
            ..FakeOs::default()
        };
        let window = held("CmdOrCtrl+Shift+Space");
        let capture = settle(
            &os,
            GlobalChord::Capture,
            None,
            Some(&window),
            "CmdOrCtrl+Shift+Alt+Space",
            false,
        );
        assert!(!capture.held.status.registered);
        assert!(
            os.asks
                .borrow()
                .iter()
                .all(|ask| !ask.starts_with("unregister")),
            "nothing of the window chord is released"
        );
    }

    #[test]
    fn one_chord_cannot_hold_both_roles() {
        let os = FakeOs::default();
        let window = held("CmdOrCtrl+Shift+Space");
        let before = held("CmdOrCtrl+Shift+Alt+Space");
        let capture = settle(
            &os,
            GlobalChord::Capture,
            Some(&before),
            Some(&window),
            "CmdOrCtrl+Shift+Space",
            false,
        );
        assert!(capture.refused);
        assert_eq!(capture.held, before, "the capture chord there was is kept");
        assert!(os.asks.borrow().is_empty(), "the OS is not asked");
    }

    #[test]
    fn an_empty_capture_chord_releases_only_the_capture_chord() {
        let os = FakeOs::default();
        let before = held("CmdOrCtrl+Shift+Alt+Space");
        let capture = settle(
            &os,
            GlobalChord::Capture,
            Some(&before),
            Some(&held("CmdOrCtrl+Shift+Space")),
            "",
            false,
        );
        assert_eq!(capture.held.status.chord, "");
        assert!(!capture.held.status.registered);
        assert_eq!(capture.held.shortcut, None);
        let asks = os.asks.borrow();
        assert_eq!(asks.len(), 1);
        assert!(asks[0].starts_with("unregister "), "{asks:?}");
    }

    #[test]
    fn the_capture_status_reaches_the_frontend_under_its_own_event_name() {
        let event = GlobalChord::Capture.status_event(GlobalHotkeyStatus {
            chord: "CmdOrCtrl+Shift+Alt+Space".to_string(),
            registered: true,
            unsupported: false,
        });
        let json = serde_json::to_value(&event).expect("the event must serialize");
        assert_eq!(json["kind"], "hotkey:capture-status");
        assert_eq!(json["payload"]["chord"], "CmdOrCtrl+Shift+Alt+Space");
    }

    #[test]
    fn resolve_shortcut_falls_back_to_default_on_parse_error() {
        let (shortcut, chord) = resolve_chord(GlobalChord::Window, "garbage++");
        assert_eq!(shortcut.key, Code::Space);
        assert!(shortcut.mods.contains(Modifiers::SHIFT));
        assert_eq!(
            chord,
            HotkeyConfig::default().toggle,
            "a chord that does not parse is reported as the one that was registered instead"
        );
    }
}
