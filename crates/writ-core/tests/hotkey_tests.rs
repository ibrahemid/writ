use writ_core::hotkey::{parse_hotkey_chord, HotkeyKey, HotkeyMod, HotkeyParseError};

#[test]
fn parses_default_summon_chord() {
    let chord = parse_hotkey_chord("CmdOrCtrl+Shift+Space").expect("parse failed");
    assert_eq!(chord.key(), HotkeyKey::Space);
    assert!(chord.has_modifier(HotkeyMod::CmdOrCtrl));
    assert!(chord.has_modifier(HotkeyMod::Shift));
    assert!(!chord.has_modifier(HotkeyMod::Ctrl));
    assert!(!chord.has_modifier(HotkeyMod::Cmd));
    assert!(!chord.has_modifier(HotkeyMod::Alt));
}

#[test]
fn parses_literal_cmd_chord_for_mac_users() {
    let chord = parse_hotkey_chord("Cmd+Shift+Space").expect("parse failed");
    assert!(chord.has_modifier(HotkeyMod::Cmd));
    assert!(!chord.has_modifier(HotkeyMod::CmdOrCtrl));
    assert_eq!(chord.key(), HotkeyKey::Space);
}

#[test]
fn parses_literal_ctrl_chord_on_any_platform() {
    let chord = parse_hotkey_chord("Ctrl+Alt+W").expect("parse failed");
    assert!(chord.has_modifier(HotkeyMod::Ctrl));
    assert!(chord.has_modifier(HotkeyMod::Alt));
    assert!(!chord.has_modifier(HotkeyMod::CmdOrCtrl));
    assert_eq!(chord.key(), HotkeyKey::KeyW);
}

#[test]
fn parser_is_case_insensitive() {
    let chord = parse_hotkey_chord("cmdorctrl+SHIFT+space").expect("parse failed");
    assert!(chord.has_modifier(HotkeyMod::CmdOrCtrl));
    assert!(chord.has_modifier(HotkeyMod::Shift));
    assert_eq!(chord.key(), HotkeyKey::Space);
}

#[test]
fn parser_accepts_alternate_modifier_spellings() {
    let chord = parse_hotkey_chord("Control+Option+T").expect("parse failed");
    assert!(chord.has_modifier(HotkeyMod::Ctrl));
    assert!(chord.has_modifier(HotkeyMod::Alt));
    assert_eq!(chord.key(), HotkeyKey::KeyT);
}

#[test]
fn parser_dedupes_repeated_modifiers() {
    let chord = parse_hotkey_chord("Shift+Shift+Space").expect("parse failed");
    assert_eq!(chord.modifiers().len(), 1);
    assert_eq!(chord.modifiers()[0], HotkeyMod::Shift);
}

#[test]
fn parses_function_keys() {
    let chord = parse_hotkey_chord("CmdOrCtrl+F12").expect("parse failed");
    assert_eq!(chord.key(), HotkeyKey::F12);
}

#[test]
fn parses_digit_keys() {
    let chord = parse_hotkey_chord("CmdOrCtrl+1").expect("parse failed");
    assert_eq!(chord.key(), HotkeyKey::Digit1);
}

#[test]
fn parser_rejects_empty_input() {
    assert_eq!(parse_hotkey_chord(""), Err(HotkeyParseError::Empty));
    assert_eq!(parse_hotkey_chord("   "), Err(HotkeyParseError::Empty));
}

#[test]
fn parser_rejects_missing_key() {
    assert_eq!(
        parse_hotkey_chord("CmdOrCtrl+Shift"),
        Err(HotkeyParseError::MissingKey)
    );
}

#[test]
fn parser_rejects_multiple_keys() {
    assert_eq!(
        parse_hotkey_chord("CmdOrCtrl+A+B"),
        Err(HotkeyParseError::MultipleKeys)
    );
}

#[test]
fn parser_rejects_unknown_token() {
    match parse_hotkey_chord("CmdOrCtrl+Bogus+Space") {
        Err(HotkeyParseError::UnknownToken(t)) => assert_eq!(t, "Bogus"),
        other => panic!("expected UnknownToken, got {other:?}"),
    }
}

#[test]
fn parser_rejects_empty_token_between_pluses() {
    match parse_hotkey_chord("CmdOrCtrl++Space") {
        Err(HotkeyParseError::UnknownToken(_)) => {}
        other => panic!("expected UnknownToken, got {other:?}"),
    }
}

#[test]
fn equal_chords_normalize_modifier_set() {
    let a = parse_hotkey_chord("CmdOrCtrl+Shift+Space").expect("parse failed");
    let b = parse_hotkey_chord("Shift+CmdOrCtrl+Space").expect("parse failed");
    assert_eq!(a.modifiers().len(), b.modifiers().len());
    assert!(a.has_modifier(HotkeyMod::CmdOrCtrl) && a.has_modifier(HotkeyMod::Shift));
    assert!(b.has_modifier(HotkeyMod::CmdOrCtrl) && b.has_modifier(HotkeyMod::Shift));
    assert_eq!(a.key(), b.key());
}
