use writ_core::config::{SidebarPosition, WritConfig};

#[test]
fn default_config_has_expected_values() {
    let config = WritConfig::default();

    assert_eq!(config.hotkey.toggle, "CmdOrCtrl+Shift+Space");

    assert_eq!(config.sidebar.toggle, "CmdOrCtrl+S");
    assert!(!config.sidebar.default_visible);
    assert!(!config.sidebar.open);
    assert_eq!(config.sidebar.position, SidebarPosition::Left);

    assert_eq!(config.editor.font_family, "monospace");
    assert_eq!(config.editor.font_size, 14);
    assert!(config.editor.word_wrap);
    assert_eq!(config.editor.tab_size, 2);
    assert_eq!(config.editor.autosave_debounce_ms, 300);

    assert_eq!(config.window.width, 1100);
    assert_eq!(config.window.height, 720);

    assert_eq!(
        config.keybindings.get("buffer.new").map(String::as_str),
        Some("CmdOrCtrl+N")
    );
    assert_eq!(
        config.keybindings.get("buffer.close").map(String::as_str),
        Some("CmdOrCtrl+W")
    );
    assert_eq!(
        config
            .keybindings
            .get("history.restoreLast")
            .map(String::as_str),
        Some("CmdOrCtrl+Shift+T")
    );
    assert_eq!(
        config.keybindings.get("sidebar.toggle").map(String::as_str),
        Some("CmdOrCtrl+S")
    );
    assert!(!config.keybindings.contains_key("sidebar.cycleMode"));
    assert_eq!(
        config.keybindings.get("palette.open").map(String::as_str),
        Some("CmdOrCtrl+Shift+P")
    );

    assert_eq!(config.history.max_entries, 500);

    assert_eq!(config.storage.path, "~/.writ");
}

#[test]
fn config_serializes_to_toml() {
    let config = WritConfig::default();
    let toml_str = toml::to_string(&config).expect("serialization failed");

    assert!(toml_str.contains("[hotkey]"));
    assert!(toml_str.contains("[sidebar]"));
    assert!(toml_str.contains("[editor]"));
    assert!(toml_str.contains("[window]"));
    assert!(toml_str.contains("[keybindings]"));
    assert!(toml_str.contains("[history]"));
    assert!(toml_str.contains("[storage]"));
}

#[test]
fn config_roundtrips_through_toml() {
    let original = WritConfig::default();
    let toml_str = toml::to_string(&original).expect("serialization failed");
    let restored: WritConfig = toml::from_str(&toml_str).expect("deserialization failed");
    assert_eq!(original, restored);
}

#[test]
fn sidebar_open_defaults_when_missing_from_toml() {
    let toml_str = "[sidebar]\ntoggle = \"CmdOrCtrl+S\"\n";
    let parsed: WritConfig = toml::from_str(toml_str).expect("deserialization failed");
    assert!(!parsed.sidebar.open);
}

#[test]
fn sidebar_open_serde_roundtrip() {
    let mut config = WritConfig::default();
    config.sidebar.open = true;
    let toml_str = toml::to_string(&config).expect("serialization failed");
    let restored: WritConfig = toml::from_str(&toml_str).expect("deserialization failed");
    assert!(restored.sidebar.open);
}

#[test]
fn legacy_sidebar_mode_field_is_silently_ignored() {
    let toml_str = "[sidebar]\nmode = \"floating\"\nopen = true\n";
    let parsed: WritConfig = toml::from_str(toml_str).expect("deserialization failed");
    assert!(parsed.sidebar.open);
}

#[test]
fn theme_defaults_to_warp_dark() {
    let config = WritConfig::default();
    assert_eq!(config.theme.preset, "warp-dark");
    assert!(config.theme.overrides.is_empty());
}

#[test]
fn theme_section_round_trips() {
    let mut config = WritConfig::default();
    config.theme.preset = "dracula".to_string();
    config
        .theme
        .overrides
        .insert("accent.default".to_string(), "#ff7b00".to_string());
    let toml_str = toml::to_string(&config).expect("serialization failed");
    let restored: WritConfig = toml::from_str(&toml_str).expect("deserialization failed");
    assert_eq!(restored.theme.preset, "dracula");
    assert_eq!(
        restored.theme.overrides.get("accent.default"),
        Some(&"#ff7b00".to_string()),
    );
}

#[test]
fn missing_theme_section_uses_defaults() {
    let toml_str = "[sidebar]\ntoggle = \"CmdOrCtrl+S\"\n";
    let parsed: WritConfig = toml::from_str(toml_str).expect("deserialization failed");
    assert_eq!(parsed.theme.preset, "warp-dark");
}

#[test]
fn config_serialization_emits_theme_section() {
    let toml_str = toml::to_string(&WritConfig::default()).expect("serialization failed");
    assert!(toml_str.contains("[theme]"));
}
