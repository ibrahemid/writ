//! What the quick capture note adds to Today's File (ADR-055).

use writ_core::capture::{append_entry, capture_entry};
use writ_core::config::{HotkeyConfig, WritConfig};

#[test]
fn a_blank_note_adds_nothing() {
    assert_eq!(capture_entry(""), None);
    assert_eq!(capture_entry("   \n\t\r\n "), None);
}

#[test]
fn an_entry_loses_the_blank_lines_before_it_and_the_space_after_it() {
    assert_eq!(
        capture_entry("\n\nbuy milk  \n\n").as_deref(),
        Some("buy milk")
    );
}

#[test]
fn the_first_line_keeps_its_indent() {
    assert_eq!(
        capture_entry("\n  - step one\n  - step two\n").as_deref(),
        Some("  - step one\n  - step two")
    );
}

#[test]
fn pasted_line_breaks_are_folded() {
    assert_eq!(
        capture_entry("one\r\ntwo\rthree").as_deref(),
        Some("one\ntwo\nthree")
    );
}

#[test]
fn an_entry_into_an_empty_file_is_its_only_line() {
    assert_eq!(append_entry("", "buy milk"), "buy milk\n");
}

#[test]
fn an_entry_follows_the_last_line() {
    assert_eq!(append_entry("first\n", "second"), "first\nsecond\n");
}

#[test]
fn a_file_without_a_final_break_gets_one_before_the_entry() {
    assert_eq!(append_entry("first", "second"), "first\nsecond\n");
}

#[test]
fn a_crlf_file_stays_crlf() {
    assert_eq!(append_entry("a\r\nb\r\n", "c\nd"), "a\r\nb\r\nc\r\nd\r\n");
    assert_eq!(append_entry("a\r\nb", "c"), "a\r\nb\r\nc\r\n");
}

#[test]
fn what_the_file_held_is_kept_byte_for_byte() {
    let existing = "one\n\n\ntwo  \n";
    let appended = append_entry(existing, "three");
    assert!(appended.starts_with(existing));
    assert_eq!(appended, "one\n\n\ntwo  \nthree\n");
}

#[test]
fn the_capture_chord_ships_beside_the_window_chord() {
    let hotkey = HotkeyConfig::default();
    assert_eq!(hotkey.capture, "CmdOrCtrl+Shift+Alt+Space");
    assert_ne!(hotkey.capture, hotkey.toggle);
}

#[test]
fn a_config_from_before_quick_capture_reads_the_shipped_capture_chord() {
    let config = WritConfig::parse("[hotkey]\ntoggle = \"Ctrl+Alt+W\"\n").expect("parses");
    assert_eq!(config.hotkey.toggle, "Ctrl+Alt+W");
    assert_eq!(config.hotkey.capture, "CmdOrCtrl+Shift+Alt+Space");
}

#[test]
fn an_empty_capture_chord_is_kept_as_off() {
    let config = WritConfig::parse("[hotkey]\ncapture = \"\"\n").expect("parses");
    assert_eq!(config.hotkey.capture, "");
    let written = toml::to_string(&config).expect("serialises");
    let read_back = WritConfig::parse(&written).expect("parses again");
    assert_eq!(read_back.hotkey.capture, "");
}
