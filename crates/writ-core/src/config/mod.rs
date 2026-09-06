//! Typed user configuration for the Writ editor.
//!
//! Writ's configuration is TOML-encoded and deserialized into
//! [`WritConfig`]. Every field has a `#[serde(default)]` so partial
//! configs remain valid and new fields can be introduced without
//! breaking existing user files.

/// Opt-in rewrite configuration (`[ai]`).
pub mod ai;
/// Keybinding conflict reporting types.
pub mod keybinding;
/// Notes-folder configuration (`[notes]`).
pub mod notes;
/// Preview surface configuration (`[preview]`).
pub mod preview;
/// Spell-check configuration (`[spelling]`).
pub mod spelling;

pub use ai::AiConfig;
pub use notes::NotesConfig;
pub use preview::{DefaultLayout, PreviewConfig};
pub use spelling::SpellingConfig;

use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

fn default_hotkey_toggle() -> String {
    "CmdOrCtrl+Shift+Space".to_string()
}

fn default_sidebar_toggle() -> String {
    "CmdOrCtrl+\\".to_string()
}

fn default_sidebar_default_visible() -> bool {
    false
}

fn default_sidebar_position() -> SidebarPosition {
    SidebarPosition::Left
}

fn default_sidebar_open() -> bool {
    true
}

fn default_sidebar_width() -> u16 {
    240
}

fn default_panel_open() -> bool {
    false
}

fn default_panel_width() -> u16 {
    240
}

fn default_font_family() -> String {
    "monospace".to_string()
}

fn default_font_size() -> u32 {
    16
}

fn default_word_wrap() -> bool {
    true
}

fn default_tab_size() -> u32 {
    2
}

fn default_autosave_debounce_ms() -> u32 {
    1000
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
    "writ-light".to_string()
}

fn default_status_bar() -> bool {
    false
}

fn default_polarity() -> Polarity {
    Polarity::System
}

fn default_accent() -> Accent {
    Accent::Pine
}

fn default_prose_face() -> ProseFace {
    ProseFace::System
}

fn default_keybindings() -> HashMap<String, String> {
    HashMap::new()
}

/// Whether the app follows the OS light/dark setting or pins one polarity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Polarity {
    /// Take the OS setting and follow it while the app runs.
    System,
    /// Always light.
    Light,
    /// Always dark.
    Dark,
}

/// The accent hue, spent on links, the caret, focus and one primary button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Accent {
    /// The default.
    Pine,
    /// The hue the app shipped with before ADR-030.
    WritBlue,
    /// Warm red-orange.
    Terracotta,
    /// Desaturated blue.
    Slate,
    /// Muted purple.
    Plum,
    /// Dark yellow.
    Gold,
}

/// The face the note body is set in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProseFace {
    /// The platform's own UI face.
    System,
    /// The bundled iA Writer Quattro S.
    Quattro,
}

/// Appearance configuration (`[appearance]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppearanceConfig {
    /// Follow the OS setting, or pin light or dark.
    #[serde(default = "default_polarity")]
    pub polarity: Polarity,
    /// Which of the six accents is spent on interaction.
    #[serde(default = "default_accent")]
    pub accent: Accent,
    /// Which face the note body is set in.
    #[serde(default = "default_prose_face")]
    pub prose_face: ProseFace,
    /// Interface text size in px, within
    /// [`INTERFACE_TEXT_SIZE_MIN`]..=[`INTERFACE_TEXT_SIZE_MAX`]. `None` leaves
    /// the platform's own size in place, which is what the stylesheet already
    /// resolves, so an unset config is not a value this has to name.
    #[serde(
        default,
        deserialize_with = "deserialize_interface_text_size",
        skip_serializing_if = "Option::is_none"
    )]
    pub interface_text_size: Option<u8>,
}

/// Reads the interface text size, clamping a hand-edited file into range so an
/// unreadable 4px or a 200px chrome can never reach the window.
fn deserialize_interface_text_size<'de, D>(deserializer: D) -> Result<Option<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<u8>::deserialize(deserializer)?.map(clamp_interface_text_size))
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            polarity: default_polarity(),
            accent: default_accent(),
            prose_face: default_prose_face(),
            interface_text_size: None,
        }
    }
}

/// Smallest interface text size the settings row offers.
pub const INTERFACE_TEXT_SIZE_MIN: u8 = 12;
/// Largest interface text size the settings row offers.
pub const INTERFACE_TEXT_SIZE_MAX: u8 = 22;

/// Clamps an interface text size into the supported range.
pub fn clamp_interface_text_size(size: u8) -> u8 {
    size.clamp(INTERFACE_TEXT_SIZE_MIN, INTERFACE_TEXT_SIZE_MAX)
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
    /// Sidebar width in CSS pixels, restored across launches. ADR-030 gives
    /// the sidebar 240px, resizable between 200 and 320.
    #[serde(default = "default_sidebar_width")]
    pub width: u16,
}

impl Default for SidebarConfig {
    fn default() -> Self {
        Self {
            toggle: default_sidebar_toggle(),
            default_visible: default_sidebar_default_visible(),
            position: default_sidebar_position(),
            open: default_sidebar_open(),
            width: default_sidebar_width(),
        }
    }
}

/// The panel beside the note: what links to it, its outline, its properties.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PanelConfig {
    /// Whether the panel was open at last save; restored across launches. A
    /// first launch shows a cursor and nothing else, so this starts false.
    #[serde(default = "default_panel_open")]
    pub open: bool,
    /// Panel width in CSS pixels, restored across launches. It takes the
    /// sidebar's 240px and its 200-320 range so the two edges match.
    #[serde(default = "default_panel_width")]
    pub width: u16,
}

impl Default for PanelConfig {
    fn default() -> Self {
        Self {
            open: default_panel_open(),
            width: default_panel_width(),
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
    /// Whether the editor shows the status bar. Off by default: the word count
    /// sits at the top right of the canvas instead (ADR-030 decision 5).
    #[serde(default = "default_status_bar")]
    pub status_bar: bool,
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
            status_bar: default_status_bar(),
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
    /// Whether the window was maximized at last save; restored across launches.
    /// The width/height above stay the pre-maximize floating size, so
    /// unmaximizing lands back at the size the user left. The x/y follow the
    /// maximized frame instead, so the next launch reopens on the same monitor.
    #[serde(default)]
    pub maximized: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: default_window_width(),
            height: default_window_height(),
            x: None,
            y: None,
            maximized: false,
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
///
/// The workspace is the folder the user opened temporarily, not a second
/// home: notes live in the notes folder (ADR-028), and closing a workspace
/// takes nothing with it.
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
    /// The panel beside the note.
    #[serde(default)]
    pub panel: PanelConfig,
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
    /// Polarity, accent and prose face.
    #[serde(default)]
    pub appearance: AppearanceConfig,
    /// Command palette ranking state.
    #[serde(default)]
    pub commands: CommandsConfig,
    /// Preview surface configuration.
    #[serde(default)]
    pub preview: PreviewConfig,
    /// Workspace folder configuration.
    #[serde(default)]
    pub workspace: WorkspaceConfig,
    /// Notes-folder configuration.
    #[serde(default)]
    pub notes: NotesConfig,
    /// Watch-inbox configuration.
    #[serde(default)]
    pub inbox: InboxConfig,
    /// Auto-update configuration.
    #[serde(default)]
    pub updater: UpdaterConfig,
    /// Opt-in rewrite configuration.
    #[serde(default)]
    pub ai: AiConfig,
    /// Spell-check configuration.
    #[serde(default)]
    pub spelling: SpellingConfig,
}

impl Default for WritConfig {
    fn default() -> Self {
        Self {
            hotkey: HotkeyConfig::default(),
            sidebar: SidebarConfig::default(),
            panel: PanelConfig::default(),
            editor: EditorConfig::default(),
            window: WindowConfig::default(),
            keybindings: default_keybindings(),
            history: HistoryConfig::default(),
            storage: StorageConfig::default(),
            theme: ThemeConfig::default(),
            appearance: AppearanceConfig::default(),
            commands: CommandsConfig::default(),
            preview: PreviewConfig::default(),
            workspace: WorkspaceConfig::default(),
            notes: NotesConfig::default(),
            inbox: InboxConfig::default(),
            updater: UpdaterConfig::default(),
            ai: AiConfig::default(),
            spelling: SpellingConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_defaults_to_system_pine_system_face() {
        let appearance = AppearanceConfig::default();
        assert_eq!(appearance.polarity, Polarity::System);
        assert_eq!(appearance.accent, Accent::Pine);
        assert_eq!(appearance.prose_face, ProseFace::System);
    }

    #[test]
    fn editor_status_bar_defaults_off() {
        let config: WritConfig = toml::from_str("").unwrap();
        assert!(!config.editor.status_bar);
    }

    #[test]
    fn missing_appearance_section_deserializes_to_defaults() {
        let config: WritConfig = toml::from_str("[editor]\nfont_size = 18\n").unwrap();
        assert_eq!(config.appearance, AppearanceConfig::default());
    }

    #[test]
    fn polarity_round_trips_through_toml() {
        for (written, expected) in [
            ("system", Polarity::System),
            ("light", Polarity::Light),
            ("dark", Polarity::Dark),
        ] {
            let source = format!("[appearance]\npolarity = \"{written}\"\n");
            let config: WritConfig = toml::from_str(&source).unwrap();
            assert_eq!(config.appearance.polarity, expected);
            let parsed: WritConfig = toml::from_str(&toml::to_string(&config).unwrap()).unwrap();
            assert_eq!(parsed.appearance.polarity, expected);
        }
    }

    #[test]
    fn accent_round_trips_through_toml() {
        let config: WritConfig = toml::from_str("[appearance]\naccent = \"writ-blue\"\n").unwrap();
        assert_eq!(config.appearance.accent, Accent::WritBlue);
        let serialized = toml::to_string(&config).unwrap();
        assert!(
            serialized.contains("accent = \"writ-blue\""),
            "{serialized}"
        );
        let parsed: WritConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(parsed.appearance.accent, Accent::WritBlue);
    }

    #[test]
    fn default_theme_preset_is_writ_light() {
        let config: WritConfig = toml::from_str("").unwrap();
        assert_eq!(config.theme.preset, "writ-light");
    }

    #[test]
    fn interface_text_size_is_unset_by_default() {
        let config: WritConfig = toml::from_str("").unwrap();
        assert_eq!(config.appearance.interface_text_size, None);
        let serialized = toml::to_string(&config).unwrap();
        assert!(!serialized.contains("interface_text_size"), "{serialized}");
    }

    #[test]
    fn interface_text_size_round_trips_through_toml() {
        let config: WritConfig =
            toml::from_str("[appearance]\ninterface_text_size = 18\n").unwrap();
        assert_eq!(config.appearance.interface_text_size, Some(18));
        let parsed: WritConfig = toml::from_str(&toml::to_string(&config).unwrap()).unwrap();
        assert_eq!(parsed.appearance.interface_text_size, Some(18));
    }

    #[test]
    fn interface_text_size_clamps_to_the_supported_range() {
        assert_eq!(clamp_interface_text_size(4), INTERFACE_TEXT_SIZE_MIN);
        assert_eq!(clamp_interface_text_size(99), INTERFACE_TEXT_SIZE_MAX);
        assert_eq!(clamp_interface_text_size(16), 16);
    }

    #[test]
    fn an_out_of_range_interface_text_size_is_clamped_on_read() {
        let small: WritConfig = toml::from_str("[appearance]\ninterface_text_size = 4\n").unwrap();
        assert_eq!(
            small.appearance.interface_text_size,
            Some(INTERFACE_TEXT_SIZE_MIN)
        );
        let large: WritConfig =
            toml::from_str("[appearance]\ninterface_text_size = 200\n").unwrap();
        assert_eq!(
            large.appearance.interface_text_size,
            Some(INTERFACE_TEXT_SIZE_MAX)
        );
    }

    #[test]
    fn prose_face_round_trips_through_toml() {
        let config: WritConfig =
            toml::from_str("[appearance]\nprose_face = \"quattro\"\n").unwrap();
        assert_eq!(config.appearance.prose_face, ProseFace::Quattro);
        let parsed: WritConfig = toml::from_str(&toml::to_string(&config).unwrap()).unwrap();
        assert_eq!(parsed.appearance.prose_face, ProseFace::Quattro);
    }

    #[test]
    fn an_existing_config_without_the_new_fields_still_loads() {
        // Every field added by ADR-030 carries a serde default, so a config
        // written by 0.3.5 deserializes without a migration.
        let source =
            "[editor]\nfont_size = 14\nword_wrap = false\n\n[theme]\npreset = \"warp-dark\"\n";
        let config: WritConfig = toml::from_str(source).unwrap();
        assert_eq!(config.editor.font_size, 14);
        assert!(!config.editor.word_wrap);
        assert!(!config.editor.status_bar);
        assert_eq!(config.theme.preset, "warp-dark");
        assert_eq!(config.appearance, AppearanceConfig::default());
    }

    #[test]
    fn missing_inbox_section_defaults_to_no_path_and_focus() {
        let config: WritConfig = toml::from_str("").unwrap();
        assert_eq!(config.inbox.path, None);
        assert!(config.inbox.focus);
    }

    #[test]
    fn missing_window_section_defaults_to_unmaximized() {
        let config: WritConfig = toml::from_str("").unwrap();
        assert!(!config.window.maximized);
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
    fn missing_ai_section_defaults_to_off() {
        let config: WritConfig = toml::from_str("").unwrap();
        assert!(!config.ai.enabled);
        assert_eq!(config.ai.preset, "ollama");
        assert!(config.ai.consented_hosts.is_empty());
    }

    #[test]
    fn ai_section_round_trips_through_toml() {
        let mut config = WritConfig::default();
        config.ai.enabled = true;
        config.ai.preset = "deepseek".to_string();
        config.ai.base_url = "https://api.deepseek.com/v1".to_string();
        config.ai.model = "deepseek-chat".to_string();
        config.ai.consented_hosts = vec!["api.deepseek.com".to_string()];

        let serialized = toml::to_string(&config).unwrap();
        let parsed: WritConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(parsed.ai, config.ai);
    }

    #[test]
    fn missing_spelling_section_defaults_to_off() {
        let config: WritConfig = toml::from_str("").unwrap();
        assert!(!config.spelling.enabled);
        assert_eq!(config.spelling.dialect, "american");
        assert!(config.spelling.ignored_words.is_empty());
    }

    #[test]
    fn spelling_section_round_trips_through_toml() {
        let mut config = WritConfig::default();
        config.spelling.enabled = true;
        config.spelling.dialect = "british".to_string();
        config.spelling.ignored_words = vec!["tauri".to_string(), "writ".to_string()];

        let serialized = toml::to_string(&config).unwrap();
        let parsed: WritConfig = toml::from_str(&serialized).unwrap();
        assert!(parsed.spelling.enabled);
        assert_eq!(parsed.spelling.dialect, "british");
        assert_eq!(
            parsed.spelling.ignored_words,
            vec!["tauri".to_string(), "writ".to_string()]
        );
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
