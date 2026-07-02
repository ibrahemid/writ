//! Typed user configuration for the Writ editor.
//!
//! Writ's configuration is TOML-encoded and deserialized into
//! [`WritConfig`]. Every field has a `#[serde(default)]` so partial
//! configs remain valid and new fields can be introduced without
//! breaking existing user files.

/// Keybinding conflict reporting types.
pub mod keybinding;
/// Preview surface configuration (`[preview]`).
pub mod preview;

pub use preview::{DefaultLayout, PreviewConfig};

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

fn default_markdown_typography() -> bool {
    true
}

fn default_markdown_editing() -> bool {
    true
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
    /// When true, markdown buffers render scaled headings, styled inline
    /// elements, and dim/hidden syntax markers in the editor itself.
    #[serde(default = "default_markdown_typography")]
    pub markdown_typography: bool,
    /// When true, markdown buffers get formatting shortcuts (bold, italic,
    /// strikethrough, inline code, link) and marker wrap-on-type over a
    /// selection.
    #[serde(default = "default_markdown_editing")]
    pub markdown_editing: bool,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            font_family: default_font_family(),
            font_size: default_font_size(),
            word_wrap: default_word_wrap(),
            tab_size: default_tab_size(),
            autosave_debounce_ms: default_autosave_debounce_ms(),
            markdown_typography: default_markdown_typography(),
            markdown_editing: default_markdown_editing(),
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

fn default_workspace_root() -> Option<String> {
    None
}

/// Workspace folder configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    /// Absolute path to the open workspace root, or `None` if no workspace
    /// is open.
    #[serde(default = "default_workspace_root")]
    pub root: Option<String>,
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            root: default_workspace_root(),
        }
    }
}

fn default_inbox_path() -> Option<String> {
    None
}

fn default_inbox_focus() -> bool {
    true
}

/// Watch-inbox configuration (ADR-018).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InboxConfig {
    /// Absolute path to the watched inbox folder, or `None` when no inbox
    /// is watched.
    #[serde(default = "default_inbox_path")]
    pub path: Option<String>,
    /// Whether Writ brings its window forward when an inbox file
    /// auto-opens.
    #[serde(default = "default_inbox_focus")]
    pub focus: bool,
}

impl Default for InboxConfig {
    fn default() -> Self {
        Self {
            path: default_inbox_path(),
            focus: default_inbox_focus(),
        }
    }
}

fn default_updater_auto_check() -> bool {
    true
}

/// Auto-update configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdaterConfig {
    /// Whether Writ silently checks for an update shortly after launch. When
    /// `false`, updates are only checked when the user picks "Check for
    /// Updates…" from the menu. The last silent-check time is tracked outside
    /// `config.toml` so checking at most once per interval never rewrites the
    /// user's editable config and never races the frontend's config writes.
    #[serde(default = "default_updater_auto_check")]
    pub auto_check: bool,
}

impl Default for UpdaterConfig {
    fn default() -> Self {
        Self {
            auto_check: default_updater_auto_check(),
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
    /// Preview surface configuration.
    #[serde(default)]
    pub preview: PreviewConfig,
    /// Workspace folder configuration.
    #[serde(default)]
    pub workspace: WorkspaceConfig,
    /// Watch-inbox configuration.
    #[serde(default)]
    pub inbox: InboxConfig,
    /// Auto-update configuration.
    #[serde(default)]
    pub updater: UpdaterConfig,
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
            preview: PreviewConfig::default(),
            workspace: WorkspaceConfig::default(),
            inbox: InboxConfig::default(),
            updater: UpdaterConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_inbox_section_defaults_to_no_path_and_focus() {
        let config: WritConfig = toml::from_str("").unwrap();
        assert_eq!(config.inbox.path, None);
        assert!(config.inbox.focus);
    }

    #[test]
    fn missing_updater_section_defaults_to_auto_check_on() {
        let config: WritConfig = toml::from_str("").unwrap();
        assert!(config.updater.auto_check);
    }

    #[test]
    fn updater_auto_check_can_be_disabled_and_round_trips() {
        let config: WritConfig = toml::from_str("[updater]\nauto_check = false\n").unwrap();
        assert!(!config.updater.auto_check);
        let serialized = toml::to_string(&config).unwrap();
        let parsed: WritConfig = toml::from_str(&serialized).unwrap();
        assert!(!parsed.updater.auto_check);
    }

    #[test]
    fn partial_inbox_section_keeps_focus_default() {
        let config: WritConfig = toml::from_str("[inbox]\npath = \"/tmp/reports\"\n").unwrap();
        assert_eq!(config.inbox.path.as_deref(), Some("/tmp/reports"));
        assert!(config.inbox.focus);
    }

    #[test]
    fn inbox_section_round_trips_through_toml() {
        let mut config = WritConfig::default();
        config.inbox.path = Some("/tmp/inbox".to_string());
        config.inbox.focus = false;

        let serialized = toml::to_string(&config).unwrap();
        let parsed: WritConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(parsed.inbox.path.as_deref(), Some("/tmp/inbox"));
        assert!(!parsed.inbox.focus);
    }
}
