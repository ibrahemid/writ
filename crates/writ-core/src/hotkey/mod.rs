//! Platform-neutral hotkey chord representation and parser.
//!
//! Writ persists hotkey chords as user-editable strings such as
//! `"CmdOrCtrl+Shift+Space"`. This module defines a typed
//! representation and a parser that converts those strings into a
//! [`HotkeyChord`] of modifiers plus a single key.
//!
//! The chord type intentionally lives in `writ-core` (zero framework
//! deps). The translation into the `tauri_plugin_global_shortcut`
//! plugin types lives in `src-tauri`, where it can use
//! `#[cfg(target_os = "...")]` to map [`HotkeyMod::CmdOrCtrl`] to the
//! platform-correct underlying modifier.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Reason a chord string failed to parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyParseError {
    /// The input was empty or contained only separators.
    Empty,
    /// A token between `+` separators was not a recognized modifier
    /// or key.
    UnknownToken(String),
    /// The chord contained zero key tokens.
    MissingKey,
    /// The chord contained more than one key token (only one
    /// non-modifier key is permitted).
    MultipleKeys,
}

impl fmt::Display for HotkeyParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HotkeyParseError::Empty => write!(f, "hotkey chord is empty"),
            HotkeyParseError::UnknownToken(t) => write!(f, "unknown hotkey token: {t}"),
            HotkeyParseError::MissingKey => write!(f, "hotkey chord is missing a key"),
            HotkeyParseError::MultipleKeys => {
                write!(f, "hotkey chord must contain exactly one key")
            }
        }
    }
}

impl std::error::Error for HotkeyParseError {}

/// A platform-neutral modifier token used in chord strings.
///
/// [`HotkeyMod::CmdOrCtrl`] is the meta token that resolves to `Cmd`
/// on macOS and `Ctrl` on Windows/Linux. [`HotkeyMod::Cmd`] is the
/// macOS-specific Command key (and the platform Super on Windows/Linux
/// when the user wants the literal Super key — but Writ avoids it in
/// defaults because Linux and Windows desktop shells routinely
/// intercept it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HotkeyMod {
    /// Resolves to Cmd (META) on macOS, Ctrl (CONTROL) elsewhere.
    CmdOrCtrl,
    /// Literal Ctrl on every platform.
    Ctrl,
    /// macOS Command key (Win key on Windows / Super on Linux). Used
    /// only when the user explicitly wants the platform Super
    /// modifier.
    Cmd,
    /// Shift on every platform.
    Shift,
    /// Alt on Windows/Linux, Option on macOS.
    Alt,
}

/// A platform-neutral key token.
///
/// Mapped to the Tauri plugin's `Code` enum in `src-tauri`. Variants
/// here cover the keys Writ actually binds; extend the enum as new
/// defaults require new keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[allow(missing_docs)]
pub enum HotkeyKey {
    Space,
    Backquote,
    Comma,
    Period,
    Slash,
    Semicolon,
    Quote,
    LeftBracket,
    RightBracket,
    Backslash,
    Minus,
    Equal,
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    KeyA,
    KeyB,
    KeyC,
    KeyD,
    KeyE,
    KeyF,
    KeyG,
    KeyH,
    KeyI,
    KeyJ,
    KeyK,
    KeyL,
    KeyM,
    KeyN,
    KeyO,
    KeyP,
    KeyQ,
    KeyR,
    KeyS,
    KeyT,
    KeyU,
    KeyV,
    KeyW,
    KeyX,
    KeyY,
    KeyZ,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    Enter,
    Escape,
    Tab,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
}

/// A fully parsed hotkey chord: an ordered set of modifiers plus
/// exactly one key.
///
/// Equality is based on the set of modifiers (order-insensitive) and
/// the key, so `"Shift+CmdOrCtrl+Space"` and
/// `"CmdOrCtrl+Shift+Space"` parse to equal chords.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotkeyChord {
    mods: Vec<HotkeyMod>,
    key: HotkeyKey,
}

impl HotkeyChord {
    /// Construct a chord directly from typed parts. Duplicate
    /// modifiers are removed, preserving first-seen order.
    pub fn new(mods: Vec<HotkeyMod>, key: HotkeyKey) -> Self {
        let mut deduped: Vec<HotkeyMod> = Vec::with_capacity(mods.len());
        for m in mods {
            if !deduped.contains(&m) {
                deduped.push(m);
            }
        }
        Self {
            mods: deduped,
            key,
        }
    }

    /// Borrow the modifier list in the order originally parsed.
    pub fn modifiers(&self) -> &[HotkeyMod] {
        &self.mods
    }

    /// The single key component of the chord.
    pub fn key(&self) -> HotkeyKey {
        self.key
    }

    /// Whether the chord contains the given modifier.
    pub fn has_modifier(&self, m: HotkeyMod) -> bool {
        self.mods.contains(&m)
    }
}

/// Parse a chord string like `"CmdOrCtrl+Shift+Space"` into a
/// [`HotkeyChord`].
///
/// Tokens are split on `+`, ASCII-case-insensitive, surrounding
/// whitespace is trimmed. Recognized modifier tokens: `CmdOrCtrl`,
/// `Ctrl`, `Control`, `Cmd`, `Command`, `Meta`, `Super`, `Shift`,
/// `Alt`, `Option`.
///
/// Recognized key tokens: `Space`, `A`-`Z`, `0`-`9`, `F1`-`F12`,
/// arrow keys, and a small set of punctuation. Exactly one key token
/// must be present.
pub fn parse_hotkey_chord(input: &str) -> Result<HotkeyChord, HotkeyParseError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(HotkeyParseError::Empty);
    }

    let mut mods: Vec<HotkeyMod> = Vec::new();
    let mut key: Option<HotkeyKey> = None;

    for raw in trimmed.split('+') {
        let token = raw.trim();
        if token.is_empty() {
            return Err(HotkeyParseError::UnknownToken(raw.to_string()));
        }

        if let Some(m) = parse_modifier_token(token) {
            if !mods.contains(&m) {
                mods.push(m);
            }
            continue;
        }

        if let Some(k) = parse_key_token(token) {
            if key.is_some() {
                return Err(HotkeyParseError::MultipleKeys);
            }
            key = Some(k);
            continue;
        }

        return Err(HotkeyParseError::UnknownToken(token.to_string()));
    }

    match key {
        Some(k) => Ok(HotkeyChord { mods, key: k }),
        None => Err(HotkeyParseError::MissingKey),
    }
}

fn parse_modifier_token(token: &str) -> Option<HotkeyMod> {
    match token.to_ascii_lowercase().as_str() {
        "cmdorctrl" | "ctrlorcmd" | "mod" => Some(HotkeyMod::CmdOrCtrl),
        "ctrl" | "control" => Some(HotkeyMod::Ctrl),
        "cmd" | "command" | "meta" | "super" => Some(HotkeyMod::Cmd),
        "shift" => Some(HotkeyMod::Shift),
        "alt" | "option" | "opt" => Some(HotkeyMod::Alt),
        _ => None,
    }
}

fn parse_key_token(token: &str) -> Option<HotkeyKey> {
    let upper = token.to_ascii_uppercase();
    match upper.as_str() {
        "SPACE" => Some(HotkeyKey::Space),
        "ENTER" | "RETURN" => Some(HotkeyKey::Enter),
        "ESC" | "ESCAPE" => Some(HotkeyKey::Escape),
        "TAB" => Some(HotkeyKey::Tab),
        "UP" | "ARROWUP" => Some(HotkeyKey::ArrowUp),
        "DOWN" | "ARROWDOWN" => Some(HotkeyKey::ArrowDown),
        "LEFT" | "ARROWLEFT" => Some(HotkeyKey::ArrowLeft),
        "RIGHT" | "ARROWRIGHT" => Some(HotkeyKey::ArrowRight),
        "BACKQUOTE" | "`" => Some(HotkeyKey::Backquote),
        "COMMA" | "," => Some(HotkeyKey::Comma),
        "PERIOD" | "." => Some(HotkeyKey::Period),
        "SLASH" | "/" => Some(HotkeyKey::Slash),
        "SEMICOLON" | ";" => Some(HotkeyKey::Semicolon),
        "QUOTE" | "'" => Some(HotkeyKey::Quote),
        "LEFTBRACKET" | "[" => Some(HotkeyKey::LeftBracket),
        "RIGHTBRACKET" | "]" => Some(HotkeyKey::RightBracket),
        "BACKSLASH" | "\\" => Some(HotkeyKey::Backslash),
        "MINUS" | "-" => Some(HotkeyKey::Minus),
        "EQUAL" | "=" => Some(HotkeyKey::Equal),
        _ => {
            if upper.len() == 1 {
                let c = upper.chars().next().unwrap();
                if c.is_ascii_alphabetic() {
                    return Some(letter_to_key(c));
                }
                if c.is_ascii_digit() {
                    return Some(digit_to_key(c));
                }
            }
            if let Some(rest) = upper.strip_prefix('F') {
                if let Ok(n) = rest.parse::<u8>() {
                    return function_key(n);
                }
            }
            None
        }
    }
}

fn letter_to_key(c: char) -> HotkeyKey {
    match c {
        'A' => HotkeyKey::KeyA,
        'B' => HotkeyKey::KeyB,
        'C' => HotkeyKey::KeyC,
        'D' => HotkeyKey::KeyD,
        'E' => HotkeyKey::KeyE,
        'F' => HotkeyKey::KeyF,
        'G' => HotkeyKey::KeyG,
        'H' => HotkeyKey::KeyH,
        'I' => HotkeyKey::KeyI,
        'J' => HotkeyKey::KeyJ,
        'K' => HotkeyKey::KeyK,
        'L' => HotkeyKey::KeyL,
        'M' => HotkeyKey::KeyM,
        'N' => HotkeyKey::KeyN,
        'O' => HotkeyKey::KeyO,
        'P' => HotkeyKey::KeyP,
        'Q' => HotkeyKey::KeyQ,
        'R' => HotkeyKey::KeyR,
        'S' => HotkeyKey::KeyS,
        'T' => HotkeyKey::KeyT,
        'U' => HotkeyKey::KeyU,
        'V' => HotkeyKey::KeyV,
        'W' => HotkeyKey::KeyW,
        'X' => HotkeyKey::KeyX,
        'Y' => HotkeyKey::KeyY,
        'Z' => HotkeyKey::KeyZ,
        _ => unreachable!("letter_to_key called with non-letter {c}"),
    }
}

fn digit_to_key(c: char) -> HotkeyKey {
    match c {
        '0' => HotkeyKey::Digit0,
        '1' => HotkeyKey::Digit1,
        '2' => HotkeyKey::Digit2,
        '3' => HotkeyKey::Digit3,
        '4' => HotkeyKey::Digit4,
        '5' => HotkeyKey::Digit5,
        '6' => HotkeyKey::Digit6,
        '7' => HotkeyKey::Digit7,
        '8' => HotkeyKey::Digit8,
        '9' => HotkeyKey::Digit9,
        _ => unreachable!("digit_to_key called with non-digit {c}"),
    }
}

fn function_key(n: u8) -> Option<HotkeyKey> {
    match n {
        1 => Some(HotkeyKey::F1),
        2 => Some(HotkeyKey::F2),
        3 => Some(HotkeyKey::F3),
        4 => Some(HotkeyKey::F4),
        5 => Some(HotkeyKey::F5),
        6 => Some(HotkeyKey::F6),
        7 => Some(HotkeyKey::F7),
        8 => Some(HotkeyKey::F8),
        9 => Some(HotkeyKey::F9),
        10 => Some(HotkeyKey::F10),
        11 => Some(HotkeyKey::F11),
        12 => Some(HotkeyKey::F12),
        _ => None,
    }
}
