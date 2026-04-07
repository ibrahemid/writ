pub mod keybinding;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

fn default_hotkey_toggle() -> String {
    "CmdOrCtrl+Shift+Space".to_string()
}

fn default_sidebar_toggle() -> String {
    "CmdOrCtrl+B".to_string()
}

fn default_sidebar_default_visible() -> bool {
    false
}

fn default_sidebar_position() -> SidebarPosition {
    SidebarPosition::Left
}

fn default_font_family() -> String {
    "monospace".to_string()
}

fn default_font_size() -> u32 {
    14
}

fn default_word_wrap() -> bool {
    true
}

fn default_tab_size() -> u32 {
    2
}

fn default_autosave_debounce_ms() -> u32 {
    300
}

fn default_window_width() -> u32 {
    800
}

fn default_window_height() -> u32 {
    600
}

fn default_max_entries() -> u32 {
    500
}

fn default_storage_path() -> String {
    "~/.writ".to_string()
}

fn default_keybindings() -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert("buffer.new".to_string(), "CmdOrCtrl+N".to_string());
    map.insert("buffer.close".to_string(), "CmdOrCtrl+W".to_string());
    map.insert(
        "history.restoreLast".to_string(),
        "CmdOrCtrl+Shift+T".to_string(),
    );
    map.insert("sidebar.toggle".to_string(), "CmdOrCtrl+B".to_string());
    map.insert("palette.open".to_string(), "CmdOrCtrl+Shift+P".to_string());
    map
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SidebarPosition {
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HotkeyConfig {
    #[serde(default = "default_hotkey_toggle")]
    pub toggle: String,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            toggle: default_hotkey_toggle(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SidebarConfig {
    #[serde(default = "default_sidebar_toggle")]
    pub toggle: String,
    #[serde(default = "default_sidebar_default_visible")]
    pub default_visible: bool,
    #[serde(default = "default_sidebar_position")]
    pub position: SidebarPosition,
}

impl Default for SidebarConfig {
    fn default() -> Self {
        Self {
            toggle: default_sidebar_toggle(),
            default_visible: default_sidebar_default_visible(),
            position: default_sidebar_position(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditorConfig {
    #[serde(default = "default_font_family")]
    pub font_family: String,
    #[serde(default = "default_font_size")]
    pub font_size: u32,
    #[serde(default = "default_word_wrap")]
    pub word_wrap: bool,
    #[serde(default = "default_tab_size")]
    pub tab_size: u32,
    #[serde(default = "default_autosave_debounce_ms")]
    pub autosave_debounce_ms: u32,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            font_family: default_font_family(),
            font_size: default_font_size(),
            word_wrap: default_word_wrap(),
            tab_size: default_tab_size(),
            autosave_debounce_ms: default_autosave_debounce_ms(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowConfig {
    #[serde(default = "default_window_width")]
    pub width: u32,
    #[serde(default = "default_window_height")]
    pub height: u32,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: default_window_width(),
            height: default_window_height(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryConfig {
    #[serde(default = "default_max_entries")]
    pub max_entries: u32,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            max_entries: default_max_entries(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StorageConfig {
    #[serde(default = "default_storage_path")]
    pub path: String,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            path: default_storage_path(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WritConfig {
    #[serde(default)]
    pub hotkey: HotkeyConfig,
    #[serde(default)]
    pub sidebar: SidebarConfig,
    #[serde(default)]
    pub editor: EditorConfig,
    #[serde(default)]
    pub window: WindowConfig,
    #[serde(default = "default_keybindings")]
    pub keybindings: HashMap<String, String>,
    #[serde(default)]
    pub history: HistoryConfig,
    #[serde(default)]
    pub storage: StorageConfig,
}

impl Default for WritConfig {
    fn default() -> Self {
        Self {
            hotkey: HotkeyConfig::default(),
            sidebar: SidebarConfig::default(),
            editor: EditorConfig::default(),
            window: WindowConfig::default(),
            keybindings: default_keybindings(),
            history: HistoryConfig::default(),
            storage: StorageConfig::default(),
        }
    }
}
