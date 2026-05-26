//! Typed user configuration for the Writ editor.
//!
//! Writ's configuration is TOML-encoded and deserialized into
//! [`WritConfig`]. Every field has a `#[serde(default)]` so partial
//! configs remain valid and new fields can be introduced without
//! breaking existing user files.

/// Keybinding conflict reporting types.
pub mod keybinding;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

fn default_hotkey_toggle() -> String {
    "CmdOrCtrl+Shift+Space".to_string()
}

fn default_sidebar_toggle() -> String {
    "CmdOrCtrl+S".to_string()
}

fn default_sidebar_default_visible() -> bool {
    false
}

fn default_sidebar_position() -> SidebarPosition {
    SidebarPosition::Left
}

fn default_sidebar_open() -> bool {
    false
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
    1100
}

fn default_window_height() -> u32 {
    720
}

fn default_max_entries() -> u32 {
    500
}

fn default_storage_path() -> String {
    "~/.writ".to_string()
}

fn default_theme_preset() -> String {
    "warp-dark".to_string()
}

fn default_keybindings() -> HashMap<String, String> {
    HashMap::new()
}

/// Which side of the window the sidebar is rendered on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SidebarPosition {
    /// Sidebar is docked to the left of the editor.
    Left,
    /// Sidebar is docked to the right of the editor.
    Right,
}

/// Global hotkey configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HotkeyConfig {
    /// Hotkey used to toggle the Writ window from any application.
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

/// Sidebar visibility and placement configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SidebarConfig {
    /// Shortcut that toggles sidebar visibility.
    #[serde(default = "default_sidebar_toggle")]
    pub toggle: String,
    /// Whether the sidebar is visible at startup. Retained for backwards
    /// compatibility with existing configs; new state lives in `open`.
    #[serde(default = "default_sidebar_default_visible")]
    pub default_visible: bool,
    /// Which side the sidebar is rendered on.
    #[serde(default = "default_sidebar_position")]
    pub position: SidebarPosition,
    /// Whether the sidebar was open at last save; restored across launches.
    #[serde(default = "default_sidebar_open")]
    pub open: bool,
}

impl Default for SidebarConfig {
    fn default() -> Self {
        Self {
            toggle: default_sidebar_toggle(),
            default_visible: default_sidebar_default_visible(),
            position: default_sidebar_position(),
            open: default_sidebar_open(),
        }
    }
}

/// Editor surface configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditorConfig {
    /// CSS font stack used by the editor.
    #[serde(default = "default_font_family")]
    pub font_family: String,
    /// Editor font size in pixels.
    #[serde(default = "default_font_size")]
    pub font_size: u32,
    /// Whether long lines soft-wrap.
    #[serde(default = "default_word_wrap")]
    pub word_wrap: bool,
    /// Tab width in spaces.
    #[serde(default = "default_tab_size")]
    pub tab_size: u32,
    /// Debounce delay, in milliseconds, before autosave fires after the
    /// last edit.
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

/// Persisted window geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowConfig {
    /// Window width in logical pixels.
    #[serde(default = "default_window_width")]
    pub width: u32,
    /// Window height in logical pixels.
    #[serde(default = "default_window_height")]
    pub height: u32,
    /// Last saved window x position in logical pixels. Signed for monitors
    /// left of the primary display; `None` until the window has been placed,
    /// so a fresh install centers on the OS default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<i32>,
    /// Last saved window y position in logical pixels. `None` until placed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<i32>,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: default_window_width(),
            height: default_window_height(),
            x: None,
            y: None,
        }
    }
}

/// History retention configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryConfig {
    /// Maximum number of closed buffers retained for reopen.
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

/// UI theme configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThemeConfig {
    /// Identifier of the active preset theme.
    #[serde(default = "default_theme_preset")]
    pub preset: String,
    /// Per-token color overrides applied on top of the preset.
    ///
    /// Keys are dot-separated token paths such as `accent.default` or
    /// `surface.background`. Values are CSS color strings.
    #[serde(default)]
    pub overrides: HashMap<String, String>,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            preset: default_theme_preset(),
            overrides: HashMap::new(),
        }
    }
}

/// Per-command usage statistics used to rank command palette results.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CommandUsage {
    /// Total number of times the command has been executed.
    #[serde(default)]
    pub count: u32,
    /// Unix epoch milliseconds at which the command was last executed.
    /// `0` means the command has never been executed.
    #[serde(default)]
    pub last_used_ms: u64,
}

/// Command palette ranking configuration.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CommandsConfig {
    /// Usage stats keyed by Writ command id.
    #[serde(default)]
    pub usage: HashMap<String, CommandUsage>,
}

/// On-disk storage location configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Filesystem path for Writ's data directory.
    ///
    /// Tildes are expanded by the host before use.
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

/// Top-level Writ configuration.
///
/// This is the root type deserialized from the user's `config.toml`.
/// Every nested section has its own `Default` implementation, so a new
/// install with no config file behaves identically to an explicit
/// "use defaults everywhere" config.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WritConfig {
    /// Global hotkey configuration.
    #[serde(default)]
    pub hotkey: HotkeyConfig,
    /// Sidebar configuration.
    #[serde(default)]
    pub sidebar: SidebarConfig,
    /// Editor surface configuration.
    #[serde(default)]
    pub editor: EditorConfig,
    /// Initial window geometry.
    #[serde(default)]
    pub window: WindowConfig,
    /// User-defined keybindings, keyed by Writ command id.
    #[serde(default = "default_keybindings")]
    pub keybindings: HashMap<String, String>,
    /// History retention configuration.
    #[serde(default)]
    pub history: HistoryConfig,
    /// On-disk storage location configuration.
    #[serde(default)]
    pub storage: StorageConfig,
    /// UI theme configuration.
    #[serde(default)]
    pub theme: ThemeConfig,
    /// Command palette ranking state.
    #[serde(default)]
    pub commands: CommandsConfig,
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
            theme: ThemeConfig::default(),
            commands: CommandsConfig::default(),
        }
    }
}
