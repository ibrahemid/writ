use writ_core::config::{SidebarPosition, WritConfig};

#[test]
fn default_config_has_expected_values() {
    let config = WritConfig::default();

    assert_eq!(config.hotkey.toggle, "CmdOrCtrl+Shift+Space");

    assert_eq!(config.sidebar.toggle, "CmdOrCtrl+B");
    assert!(!config.sidebar.default_visible);
    assert_eq!(config.sidebar.position, SidebarPosition::Left);

    assert_eq!(config.editor.font_family, "monospace");
    assert_eq!(config.editor.font_size, 14);
    assert!(config.editor.word_wrap);
    assert_eq!(config.editor.tab_size, 2);
    assert_eq!(config.editor.autosave_debounce_ms, 300);

    assert_eq!(config.window.width, 800);
    assert_eq!(config.window.height, 600);

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
        Some("CmdOrCtrl+B")
    );
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
