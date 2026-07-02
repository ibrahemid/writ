use writ_core::config::{CommandUsage, SidebarPosition, WritConfig};

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

    assert!(config.keybindings.is_empty());

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

#[test]
fn commands_usage_default_is_empty() {
    let config = WritConfig::default();
    assert!(config.commands.usage.is_empty());
}

#[test]
fn commands_usage_round_trips_through_toml() {
    let mut config = WritConfig::default();
    config.commands.usage.insert(
        "palette.open".to_string(),
        CommandUsage {
            count: 17,
            last_used_ms: 1_715_500_000_000,
        },
    );
    let toml_str = toml::to_string(&config).expect("serialization failed");
    let restored: WritConfig = toml::from_str(&toml_str).expect("deserialization failed");
    let entry = restored
        .commands
        .usage
        .get("palette.open")
        .expect("palette.open usage missing");
    assert_eq!(entry.count, 17);
    assert_eq!(entry.last_used_ms, 1_715_500_000_000);
}

#[test]
fn partial_config_without_commands_section_deserializes() {
    let toml_str = "[sidebar]\ntoggle = \"CmdOrCtrl+S\"\n";
    let parsed: WritConfig = toml::from_str(toml_str).expect("deserialization failed");
    assert!(parsed.commands.usage.is_empty());
}

#[test]
fn window_position_defaults_to_none() {
    let config = WritConfig::default();
    assert_eq!(config.window.x, None);
    assert_eq!(config.window.y, None);
}

#[test]
fn window_position_missing_from_toml_defaults_to_none() {
    let toml_str = "[window]\nwidth = 1100\nheight = 720\n";
    let parsed: WritConfig = toml::from_str(toml_str).expect("deserialization failed");
    assert_eq!(parsed.window.x, None);
    assert_eq!(parsed.window.y, None);
}

#[test]
fn window_position_round_trips_through_toml() {
    let mut config = WritConfig::default();
    config.window.x = Some(240);
    config.window.y = Some(-120);
    let toml_str = toml::to_string(&config).expect("serialization failed");
    let restored: WritConfig = toml::from_str(&toml_str).expect("deserialization failed");
    assert_eq!(restored.window.x, Some(240));
    assert_eq!(restored.window.y, Some(-120));
}

#[test]
fn window_position_omitted_from_toml_when_unset() {
    let toml_str = toml::to_string(&WritConfig::default()).expect("serialization failed");
    let window_section = toml_str
        .split("[window]")
        .nth(1)
        .and_then(|rest| rest.split('[').next())
        .unwrap_or("");
    assert!(!window_section.contains("x ="));
    assert!(!window_section.contains("y ="));
}

#[test]
fn markdown_typography_defaults_to_true() {
    let config = WritConfig::default();
    assert!(config.editor.markdown_typography);
}

#[test]
fn markdown_typography_false_roundtrips_through_toml() {
    let toml_str = "[editor]\nmarkdown_typography = false\n";
    let config: WritConfig = toml::from_str(toml_str).expect("deserialization failed");
    assert!(!config.editor.markdown_typography);
    let serialized = toml::to_string(&config).expect("serialization failed");
    let restored: WritConfig = toml::from_str(&serialized).expect("deserialization failed");
    assert!(!restored.editor.markdown_typography);
}

#[test]
fn markdown_typography_missing_from_toml_uses_default() {
    let toml_str = "[editor]\nfont_size = 16\n";
    let config: WritConfig = toml::from_str(toml_str).expect("deserialization failed");
    assert!(config.editor.markdown_typography);
}

#[test]
fn markdown_editing_defaults_to_true() {
    let config = WritConfig::default();
    assert!(config.editor.markdown_editing);
}

#[test]
fn markdown_editing_false_roundtrips_through_toml() {
    let toml_str = "[editor]\nmarkdown_editing = false\n";
    let config: WritConfig = toml::from_str(toml_str).expect("deserialization failed");
    assert!(!config.editor.markdown_editing);
    let serialized = toml::to_string(&config).expect("serialization failed");
    let restored: WritConfig = toml::from_str(&serialized).expect("deserialization failed");
    assert!(!restored.editor.markdown_editing);
}

#[test]
fn markdown_editing_missing_from_toml_uses_default() {
    let toml_str = "[editor]\nfont_size = 16\n";
    let config: WritConfig = toml::from_str(toml_str).expect("deserialization failed");
    assert!(config.editor.markdown_editing);
}
