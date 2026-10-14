//! The one list of menu commands, shared with the frontend.
//!
//! `src/commands/menu-commands.json` is the source: the macOS menu bar
//! ([`crate::build_app_menu`]) builds its items from it, and
//! `src/components/TitleBar/AppMenu.tsx` builds the Windows and Linux menu
//! from the same file. Nothing here is behind a `cfg`: the list and its
//! routing are the contract both menus answer to, so they compile — and are
//! tested — on every target.

use serde::Deserialize;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::OnceLock;
use writ_core::config::AppId;

const MENU_COMMANDS_JSON: &str = include_str!("../../src/commands/menu-commands.json");
const NATIVE_MENU_CHORDS_JSON: &str = include_str!("../../src/commands/native-menu-chords.json");

/// The menu an item hangs under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MenuSection {
    App,
    File,
    Edit,
    View,
    Help,
}

/// Where an item is offered. The frontend's names, so one word means one
/// platform on both sides of the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MenuPlatform {
    Mac,
    Win,
    Linux,
}

/// The action a menu item raises.
///
/// One variant per item the menus offer. An item id with no variant reaches
/// nothing, which is what [`menu_action_for_id`] answers `None` for and what
/// the tests below hold the shared list to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    ThirdPartyNotices,
    OpenSettings,
    CheckUpdates,
    NewNote,
    TodaysNote,
    QuickOpenNote,
    OpenFile,
    OpenRecent,
    ShowNotesFolder,
    Save,
    SaveCopy,
    RenameNote,
    OpenNoteVersions,
    CloseTab,
    ReopenClosedTab,
    Find,
    FindNext,
    FindPrevious,
    Replace,
    ToggleSidebar,
    TogglePanel,
    ToggleChat,
    OpenFolderGraph,
    OpenProgramActivity,
    NextTab,
    PreviousTab,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    OpenPalette,
    CustomizeShortcuts,
    OpenFolder,
    SearchEverywhere,
    CopyAsPrompt,
    /// One of the text transforms of the Edit menu's Transform submenu, by
    /// its command id (an entry of [`TRANSFORM_MENU_IDS`]).
    Transform(&'static str),
    OpenHelp,
    OpenWhatsNew,
    ShowSetupAgain,
    ReportIssue,
}

/// The command ids of the Transform submenu, in the order it lists them. One
/// per built-in transform (`writ_plugin::transform::builtins`); the frontend
/// registers each as `transform.<transform id>` from the same registry.
pub const TRANSFORM_MENU_IDS: [&str; 9] = [
    "transform.trim_trailing_whitespace",
    "transform.trim_leading_whitespace",
    "transform.normalize_whitespace",
    "transform.tidy_whitespace",
    "transform.dedent",
    "transform.ensure_final_newline",
    "transform.smart_to_straight_quotes",
    "transform.fix_punctuation_spacing",
    "transform.prepare_prompt",
];

impl MenuAction {
    /// Every action the table routes to.
    pub const ALL: [MenuAction; 47] = [
        MenuAction::ThirdPartyNotices,
        MenuAction::OpenSettings,
        MenuAction::CheckUpdates,
        MenuAction::NewNote,
        MenuAction::TodaysNote,
        MenuAction::QuickOpenNote,
        MenuAction::OpenFile,
        MenuAction::OpenRecent,
        MenuAction::ShowNotesFolder,
        MenuAction::Save,
        MenuAction::SaveCopy,
        MenuAction::RenameNote,
        MenuAction::OpenNoteVersions,
        MenuAction::CloseTab,
        MenuAction::ReopenClosedTab,
        MenuAction::Find,
        MenuAction::FindNext,
        MenuAction::FindPrevious,
        MenuAction::Replace,
        MenuAction::ToggleSidebar,
        MenuAction::TogglePanel,
        MenuAction::ToggleChat,
        MenuAction::OpenFolderGraph,
        MenuAction::OpenProgramActivity,
        MenuAction::NextTab,
        MenuAction::PreviousTab,
        MenuAction::ZoomIn,
        MenuAction::ZoomOut,
        MenuAction::ZoomReset,
        MenuAction::OpenPalette,
        MenuAction::CustomizeShortcuts,
        MenuAction::OpenFolder,
        MenuAction::SearchEverywhere,
        MenuAction::CopyAsPrompt,
        MenuAction::Transform(TRANSFORM_MENU_IDS[0]),
        MenuAction::Transform(TRANSFORM_MENU_IDS[1]),
        MenuAction::Transform(TRANSFORM_MENU_IDS[2]),
        MenuAction::Transform(TRANSFORM_MENU_IDS[3]),
        MenuAction::Transform(TRANSFORM_MENU_IDS[4]),
        MenuAction::Transform(TRANSFORM_MENU_IDS[5]),
        MenuAction::Transform(TRANSFORM_MENU_IDS[6]),
        MenuAction::Transform(TRANSFORM_MENU_IDS[7]),
        MenuAction::Transform(TRANSFORM_MENU_IDS[8]),
        MenuAction::OpenHelp,
        MenuAction::OpenWhatsNew,
        MenuAction::ShowSetupAgain,
        MenuAction::ReportIssue,
    ];

    /// The command id the frontend registry answers to. It is the wire format
    /// `WritEvent::MenuAction` carries, so it is spelled the registry's way.
    pub fn command_id(self) -> &'static str {
        match self {
            MenuAction::ThirdPartyNotices => "app.thirdPartyNotices",
            MenuAction::OpenSettings => "settings.open",
            MenuAction::CheckUpdates => "app.check_updates",
            MenuAction::NewNote => "note.new",
            MenuAction::TodaysNote => "note.today",
            MenuAction::QuickOpenNote => "notes.quickOpen",
            MenuAction::OpenFile => "file.open",
            MenuAction::OpenRecent => "history.openRecent",
            MenuAction::ShowNotesFolder => "notes.showFolder",
            MenuAction::Save => "buffer.save",
            MenuAction::SaveCopy => "note.saveCopy",
            MenuAction::RenameNote => "note.rename",
            MenuAction::OpenNoteVersions => "note.versions",
            MenuAction::CloseTab => "buffer.close",
            MenuAction::ReopenClosedTab => "history.restoreLast",
            MenuAction::Find => "editor.find",
            MenuAction::FindNext => "editor.findNext",
            MenuAction::FindPrevious => "editor.findPrevious",
            MenuAction::Replace => "editor.replace",
            MenuAction::ToggleSidebar => "sidebar.toggle",
            MenuAction::TogglePanel => "panel.toggle",
            MenuAction::ToggleChat => "chat.toggle",
            MenuAction::OpenFolderGraph => "folderGraph.open",
            MenuAction::OpenProgramActivity => "activity.open",
            MenuAction::NextTab => "buffer.nextTab",
            MenuAction::PreviousTab => "buffer.prevTab",
            MenuAction::ZoomIn => "editor.zoomIn",
            MenuAction::ZoomOut => "editor.zoomOut",
            MenuAction::ZoomReset => "editor.zoomReset",
            MenuAction::OpenPalette => "palette.open",
            MenuAction::CustomizeShortcuts => "shortcuts.customize",
            MenuAction::OpenFolder => "workspace.openFolder",
            MenuAction::SearchEverywhere => "search.openEverywhere",
            MenuAction::CopyAsPrompt => "prompt.copyAsPrompt",
            MenuAction::Transform(id) => id,
            MenuAction::OpenHelp => "help.docs",
            MenuAction::OpenWhatsNew => "help.whatsNew",
            MenuAction::ShowSetupAgain => "help.showSetup",
            MenuAction::ReportIssue => "help.reportIssue",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct MenuCommand {
    /// The command id the frontend registry answers to, and the menu item's
    /// own id. [`menu_action_for_id`] turns it back into a [`MenuAction`].
    pub id: String,
    /// The menu bar's wording, in the Title Case macOS menus use.
    pub label: String,
    /// Tauri accelerator, absent for an item the menu shows without one.
    #[serde(default)]
    pub accelerator: Option<String>,
    pub menu: MenuSection,
    /// Items of one group sit together; a separator is drawn between groups.
    pub group: u8,
    pub platforms: Vec<MenuPlatform>,
    /// The app the item belongs to, absent for an item every install has.
    /// An item whose app is off is left out of the menu (ADR-042 section 3).
    #[serde(default)]
    pub app: Option<AppId>,
    /// The submenu the item sits in inside its menu, absent for an item of
    /// the menu itself. Items naming one submenu are drawn together in it,
    /// where the first of them sits.
    #[serde(default)]
    pub submenu: Option<String>,
}

impl MenuCommand {
    pub fn is_offered_on(&self, platform: MenuPlatform) -> bool {
        self.platforms.contains(&platform)
    }

    /// Whether the item belongs in a menu drawn while `apps` are on.
    pub fn is_offered_with(&self, apps: &BTreeSet<AppId>) -> bool {
        self.app.is_none_or(|app| apps.contains(&app))
    }
}

/// The shared list, parsed once.
///
/// A malformed list is a build-time mistake that would leave the app with no
/// menu bar at all, so it panics here rather than starting without one.
pub fn menu_commands() -> &'static [MenuCommand] {
    static COMMANDS: OnceLock<Vec<MenuCommand>> = OnceLock::new();
    COMMANDS.get_or_init(|| {
        serde_json::from_str(MENU_COMMANDS_JSON)
            .expect("src/commands/menu-commands.json must parse")
    })
}

/// The commands one menu carries on one platform while `apps` are on, in
/// list order.
pub fn commands_in(
    section: MenuSection,
    platform: MenuPlatform,
    apps: &BTreeSet<AppId>,
) -> impl Iterator<Item = &'static MenuCommand> + '_ {
    menu_commands().iter().filter(move |command| {
        command.menu == section && command.is_offered_on(platform) && command.is_offered_with(apps)
    })
}

/// The action a menu item id forwards to the frontend, or `None` for an id
/// this table does not route.
pub fn menu_action_for_id(id: &str) -> Option<MenuAction> {
    Some(match id {
        "app.thirdPartyNotices" => MenuAction::ThirdPartyNotices,
        "settings.open" => MenuAction::OpenSettings,
        "app.check_updates" => MenuAction::CheckUpdates,
        "note.new" => MenuAction::NewNote,
        "note.today" => MenuAction::TodaysNote,
        "notes.quickOpen" => MenuAction::QuickOpenNote,
        "file.open" => MenuAction::OpenFile,
        "history.openRecent" => MenuAction::OpenRecent,
        "notes.showFolder" => MenuAction::ShowNotesFolder,
        "buffer.save" => MenuAction::Save,
        "note.saveCopy" => MenuAction::SaveCopy,
        "note.rename" => MenuAction::RenameNote,
        "note.versions" => MenuAction::OpenNoteVersions,
        "buffer.close" => MenuAction::CloseTab,
        "history.restoreLast" => MenuAction::ReopenClosedTab,
        "editor.find" => MenuAction::Find,
        "editor.findNext" => MenuAction::FindNext,
        "editor.findPrevious" => MenuAction::FindPrevious,
        "editor.replace" => MenuAction::Replace,
        "sidebar.toggle" => MenuAction::ToggleSidebar,
        "panel.toggle" => MenuAction::TogglePanel,
        "chat.toggle" => MenuAction::ToggleChat,
        "folderGraph.open" => MenuAction::OpenFolderGraph,
        "activity.open" => MenuAction::OpenProgramActivity,
        "buffer.nextTab" => MenuAction::NextTab,
        "buffer.prevTab" => MenuAction::PreviousTab,
        "editor.zoomIn" => MenuAction::ZoomIn,
        "editor.zoomOut" => MenuAction::ZoomOut,
        "editor.zoomReset" => MenuAction::ZoomReset,
        "palette.open" => MenuAction::OpenPalette,
        "shortcuts.customize" => MenuAction::CustomizeShortcuts,
        "workspace.openFolder" => MenuAction::OpenFolder,
        "search.openEverywhere" => MenuAction::SearchEverywhere,
        "prompt.copyAsPrompt" => MenuAction::CopyAsPrompt,
        "help.docs" => MenuAction::OpenHelp,
        "help.whatsNew" => MenuAction::OpenWhatsNew,
        "help.showSetup" => MenuAction::ShowSetupAgain,
        "help.reportIssue" => MenuAction::ReportIssue,
        _ => {
            return TRANSFORM_MENU_IDS
                .iter()
                .copied()
                .find(|transform| *transform == id)
                .map(MenuAction::Transform)
        }
    })
}

/// A chord the macOS bar's own items hold (Quit, Hide, the Window and Edit
/// items AppKit provides), in the frontend's spelling. The shortcut editor
/// reads the same file and refuses these chords on macOS.
#[derive(Debug, Clone, Deserialize)]
pub struct NativeMenuChord {
    pub chord: String,
    pub label: String,
}

/// The bar's own chords, parsed once. Malformed means a build-time mistake,
/// as for [`menu_commands`].
pub fn native_menu_chords() -> &'static [NativeMenuChord] {
    static CHORDS: OnceLock<Vec<NativeMenuChord>> = OnceLock::new();
    CHORDS.get_or_init(|| {
        serde_json::from_str(NATIVE_MENU_CHORDS_JSON)
            .expect("src/commands/native-menu-chords.json must parse")
    })
}

const NAMED_KEYS: [&str; 14] = [
    "Space",
    "Enter",
    "Tab",
    "Backspace",
    "Delete",
    "Escape",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    "ArrowUp",
    "ArrowDown",
    "ArrowLeft",
    "ArrowRight",
];

fn is_function_key(key: &str) -> bool {
    key.strip_prefix('F')
        .filter(|number| number.bytes().all(|b| b.is_ascii_digit()) && !number.starts_with('0'))
        .and_then(|number| number.parse::<u8>().ok())
        .is_some_and(|number| (1..=24).contains(&number))
}

fn is_accelerator_key(key: &str) -> bool {
    let mut chars = key.chars();
    if let (Some(only), None) = (chars.next(), chars.next()) {
        return only.is_ascii_uppercase() || only.is_ascii_digit() || "`\\[],=-.';/".contains(only);
    }
    NAMED_KEYS.contains(&key) || is_function_key(key)
}

/// The menu accelerator for a chord the shortcut editor saved, or `None` for
/// one a menu item cannot carry: the double tap of a modifier, a key the
/// accelerator parser does not know, or a bare key that would take typing.
///
/// The two spellings agree token for token (`CmdOrCtrl`, `Ctrl`, `Shift`,
/// `Alt`, then the key), so a chord that passes is used as it is.
pub fn accelerator_for_chord(chord: &str) -> Option<String> {
    let chord = chord.trim();
    let mut parts: Vec<&str> = chord.split('+').collect();
    let key = parts.pop()?;
    if !is_accelerator_key(key) {
        return None;
    }
    let mut seen = HashSet::new();
    for modifier in &parts {
        let known = matches!(*modifier, "CmdOrCtrl" | "Ctrl" | "Shift" | "Alt");
        if !known || !seen.insert(*modifier) {
            return None;
        }
    }
    if parts.is_empty() && !is_function_key(key) {
        return None;
    }
    Some(chord.to_string())
}

/// The one chord the shortcut editor records that no accelerator can carry:
/// two taps of Shift. An item answering it says so in its label instead.
const DOUBLE_SHIFT: &str = "Shift+Shift";

/// One item as the bar draws it: its entry in the shared list with the chord
/// the saved overrides give it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedItem {
    pub id: String,
    pub label: String,
    pub section: MenuSection,
    pub group: u8,
    pub accelerator: Option<String>,
    pub submenu: Option<String>,
}

/// Everything the shared list puts in the bar for one config: the items of
/// the apps that are on, each with its effective chord. The bar is drawn
/// again only when this changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuPlan {
    items: Vec<PlannedItem>,
}

impl MenuPlan {
    /// The plan for `platform` while `apps` are on, with `keybindings` (the
    /// config's overrides, by command id) in place of the list's chords. An
    /// empty override unbinds the item. A chord a native item or an earlier
    /// item already holds is left off, so one key never names two items.
    pub fn new(
        platform: MenuPlatform,
        apps: &BTreeSet<AppId>,
        keybindings: &HashMap<String, String>,
    ) -> Self {
        let mut held: HashSet<String> = native_menu_chords()
            .iter()
            .map(|native| native.chord.clone())
            .collect();
        let items = menu_commands()
            .iter()
            .filter(|command| command.is_offered_on(platform) && command.is_offered_with(apps))
            .map(|command| {
                let chord = keybindings
                    .get(&command.id)
                    .or(command.accelerator.as_ref());
                let accelerator = chord
                    .and_then(|chord| accelerator_for_chord(chord))
                    .filter(|chord| held.insert(chord.clone()));
                let label = if chord.is_some_and(|chord| chord.trim() == DOUBLE_SHIFT) {
                    format!("{} ⇧⇧", command.label)
                } else {
                    command.label.clone()
                };
                PlannedItem {
                    id: command.id.clone(),
                    label,
                    section: command.menu,
                    group: command.group,
                    accelerator,
                    submenu: command.submenu.clone(),
                }
            })
            .collect();
        Self { items }
    }

    /// One menu's items, in list order.
    pub fn section(&self, section: MenuSection) -> impl Iterator<Item = &PlannedItem> + '_ {
        self.items
            .iter()
            .filter(move |item| item.section == section)
    }

    /// The chord an item is drawn with, `None` for an item with none or one
    /// the plan does not hold.
    pub fn accelerator_of(&self, id: &str) -> Option<&str> {
        self.items
            .iter()
            .find(|item| item.id == id)
            .and_then(|item| item.accelerator.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn the_shared_list_parses_and_is_not_empty() {
        assert!(menu_commands().len() > 10);
    }

    #[test]
    fn every_id_routes_and_every_route_is_an_id() {
        for command in menu_commands() {
            let action = menu_action_for_id(&command.id)
                .unwrap_or_else(|| panic!("{} is in the menu with no route", command.id));
            assert_eq!(
                action.command_id(),
                command.id,
                "{} routes to an action that names a different command",
                command.id
            );
        }

        let ids: HashSet<&str> = menu_commands().iter().map(|c| c.id.as_str()).collect();
        for action in MenuAction::ALL {
            assert!(
                ids.contains(action.command_id()),
                "{action:?} routes to a command no menu item raises"
            );
        }
    }

    #[test]
    fn unknown_ids_route_nowhere() {
        assert_eq!(menu_action_for_id(""), None);
        assert_eq!(menu_action_for_id("unknown.command"), None);
        assert_eq!(menu_action_for_id("file.open "), None);
        assert_eq!(menu_action_for_id("app.quit"), None);
    }

    #[test]
    fn no_id_appears_twice() {
        let mut seen = HashSet::new();
        for command in menu_commands() {
            assert!(
                seen.insert(command.id.as_str()),
                "{} listed twice",
                command.id
            );
        }
    }

    #[test]
    fn no_accelerator_is_claimed_twice() {
        let mut seen: HashSet<&str> = HashSet::new();
        for command in menu_commands() {
            let Some(accelerator) = command.accelerator.as_deref() else {
                continue;
            };
            assert!(
                seen.insert(accelerator),
                "{accelerator} is on more than one menu item"
            );
        }
    }

    #[test]
    fn no_command_is_reachable_from_the_macos_menu_alone() {
        for command in menu_commands() {
            assert!(
                command.is_offered_on(MenuPlatform::Win)
                    && command.is_offered_on(MenuPlatform::Linux),
                "{} would be unreachable off macOS",
                command.id
            );
        }
    }

    #[test]
    fn every_menu_the_builder_draws_has_items() {
        for section in [
            MenuSection::App,
            MenuSection::File,
            MenuSection::Edit,
            MenuSection::View,
            MenuSection::Help,
        ] {
            assert!(
                commands_in(section, MenuPlatform::Mac, &BTreeSet::new())
                    .next()
                    .is_some(),
                "{section:?} is empty"
            );
        }
    }

    fn view_ids(apps: &BTreeSet<AppId>) -> Vec<&'static str> {
        commands_in(MenuSection::View, MenuPlatform::Mac, apps)
            .map(|command| command.id.as_str())
            .collect()
    }

    /// The View items every install has, after the ones that belong to apps.
    const VIEW_TAIL: [&str; 5] = [
        "buffer.nextTab",
        "buffer.prevTab",
        "editor.zoomIn",
        "editor.zoomOut",
        "editor.zoomReset",
    ];

    fn with_tail(head: &[&'static str]) -> Vec<&'static str> {
        head.iter().copied().chain(VIEW_TAIL).collect()
    }

    #[test]
    fn an_app_that_is_off_has_no_menu_item() {
        assert_eq!(view_ids(&BTreeSet::new()), with_tail(&["sidebar.toggle"]));
    }

    #[test]
    fn an_app_that_is_on_brings_its_item_back() {
        let chat: BTreeSet<AppId> = [AppId::Chat].into();
        assert_eq!(
            view_ids(&chat),
            with_tail(&["sidebar.toggle", "chat.toggle"])
        );
        let all: BTreeSet<AppId> = AppId::ALL.into();
        assert_eq!(
            view_ids(&all),
            with_tail(&[
                "sidebar.toggle",
                "panel.toggle",
                "chat.toggle",
                "folderGraph.open",
                "activity.open"
            ])
        );
    }

    #[test]
    fn program_activity_is_in_the_view_menu_while_connected_programs_is_on() {
        let programs: BTreeSet<AppId> = [AppId::Programs].into();
        assert_eq!(
            view_ids(&programs),
            with_tail(&["sidebar.toggle", "activity.open"])
        );
        let item = menu_commands()
            .iter()
            .find(|command| command.id == "activity.open")
            .expect("the item");
        assert_eq!(item.label, "Program Activity");
        assert_eq!(
            menu_action_for_id("activity.open"),
            Some(MenuAction::OpenProgramActivity)
        );
    }

    #[test]
    fn items_that_belong_to_an_app_name_the_right_one() {
        let apps: Vec<(&str, AppId)> = menu_commands()
            .iter()
            .filter_map(|command| command.app.map(|app| (command.id.as_str(), app)))
            .collect();
        assert_eq!(
            apps,
            [
                ("panel.toggle", AppId::Connections),
                ("chat.toggle", AppId::Chat),
                ("folderGraph.open", AppId::Graph),
                ("activity.open", AppId::Programs),
            ]
        );
    }

    fn plan_with(overrides: &[(&str, &str)]) -> MenuPlan {
        let keybindings: HashMap<String, String> = overrides
            .iter()
            .map(|(id, chord)| (id.to_string(), chord.to_string()))
            .collect();
        MenuPlan::new(MenuPlatform::Mac, &AppId::ALL.into(), &keybindings)
    }

    #[test]
    fn with_no_override_every_item_carries_the_list_chord() {
        let plan = plan_with(&[]);
        for command in menu_commands() {
            let expected = command
                .accelerator
                .as_deref()
                .filter(|chord| *chord != DOUBLE_SHIFT);
            assert_eq!(plan.accelerator_of(&command.id), expected, "{}", command.id);
        }
    }

    fn labels_in(plan: &MenuPlan, section: MenuSection) -> Vec<(&str, &str)> {
        plan.section(section)
            .map(|item| (item.id.as_str(), item.label.as_str()))
            .collect()
    }

    #[test]
    fn the_help_menu_holds_the_palette_and_the_five_help_items() {
        assert_eq!(
            labels_in(&plan_with(&[]), MenuSection::Help),
            [
                ("palette.open", "Command Palette… ⇧⇧"),
                ("help.docs", "Writ Help"),
                ("shortcuts.customize", "Keyboard Shortcuts"),
                ("help.whatsNew", "What's New"),
                ("help.showSetup", "Show Setup Again"),
                ("help.reportIssue", "Report an Issue"),
            ]
        );
    }

    #[test]
    fn the_palette_item_names_double_shift_only_while_it_answers_it() {
        let plan = plan_with(&[]);
        assert_eq!(plan.accelerator_of("palette.open"), None);

        let rebound = plan_with(&[("palette.open", "CmdOrCtrl+Shift+P")]);
        assert_eq!(
            rebound.accelerator_of("palette.open"),
            Some("CmdOrCtrl+Shift+P")
        );
        assert_eq!(
            labels_in(&rebound, MenuSection::Help)[0],
            ("palette.open", "Command Palette…")
        );

        let unbound = plan_with(&[("palette.open", "")]);
        assert_eq!(
            labels_in(&unbound, MenuSection::Help)[0],
            ("palette.open", "Command Palette…")
        );
    }

    #[test]
    fn search_everywhere_and_open_folder_have_items_and_chords() {
        let plan = plan_with(&[]);
        assert!(plan
            .section(MenuSection::Edit)
            .any(|item| item.id == "search.openEverywhere" && item.label == "Search Everywhere…"));
        assert_eq!(
            plan.accelerator_of("search.openEverywhere"),
            Some("CmdOrCtrl+Shift+F")
        );
        let file: Vec<&str> = plan
            .section(MenuSection::File)
            .map(|item| item.id.as_str())
            .collect();
        let open = file
            .iter()
            .position(|id| *id == "file.open")
            .expect("Open File");
        assert_eq!(file.get(open + 1), Some(&"workspace.openFolder"));
        assert_eq!(
            plan.accelerator_of("workspace.openFolder"),
            Some("CmdOrCtrl+Alt+O")
        );
        for platform in [MenuPlatform::Win, MenuPlatform::Linux] {
            let other = MenuPlan::new(platform, &BTreeSet::new(), &HashMap::new());
            assert_eq!(
                other.accelerator_of("workspace.openFolder"),
                Some("CmdOrCtrl+Alt+O")
            );
        }
    }

    #[test]
    fn the_transform_submenu_lists_every_built_in_transform() {
        let plan = plan_with(&[]);
        let transforms: Vec<&PlannedItem> = plan
            .section(MenuSection::Edit)
            .filter(|item| item.submenu.as_deref() == Some("Transform"))
            .collect();
        let ids: Vec<&str> = transforms.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(ids, TRANSFORM_MENU_IDS);

        let mut registry = writ_plugin::transform::TransformRegistry::new();
        writ_plugin::transform::builtins::register_builtins(&mut registry).expect("builtins");
        let built_in: BTreeSet<String> = registry
            .list()
            .iter()
            .map(|descriptor| format!("transform.{}", descriptor.id))
            .collect();
        let listed: BTreeSet<String> = ids.iter().map(|id| id.to_string()).collect();
        assert_eq!(listed, built_in, "the submenu and the built-ins differ");

        // The bar's Title Case says the words the palette says.
        for descriptor in registry.list() {
            let item = transforms
                .iter()
                .find(|item| item.id == format!("transform.{}", descriptor.id))
                .expect("listed");
            assert_eq!(
                item.label.to_lowercase(),
                descriptor.metadata.label.to_lowercase(),
                "{}",
                item.id
            );
        }
    }

    #[test]
    fn copy_as_prompt_follows_the_transform_submenu_in_the_edit_menu() {
        let plan = plan_with(&[]);
        let edit: Vec<&str> = plan
            .section(MenuSection::Edit)
            .map(|item| item.id.as_str())
            .collect();
        let last_transform = edit
            .iter()
            .rposition(|id| id.starts_with("transform."))
            .expect("transforms");
        assert_eq!(edit.get(last_transform + 1), Some(&"prompt.copyAsPrompt"));
    }

    #[test]
    fn the_items_of_one_submenu_sit_together() {
        let mut closed: HashSet<&str> = HashSet::new();
        let mut open: Option<&str> = None;
        for command in menu_commands() {
            let submenu = command.submenu.as_deref();
            if submenu != open {
                if let Some(left) = open {
                    closed.insert(left);
                }
                if let Some(name) = submenu {
                    assert!(!closed.contains(name), "{name} is split by {}", command.id);
                }
                open = submenu;
            }
        }
    }

    #[test]
    fn an_override_changes_the_native_accelerator() {
        let plan = plan_with(&[("sidebar.toggle", "CmdOrCtrl+Shift+B")]);
        assert_eq!(
            plan.accelerator_of("sidebar.toggle"),
            Some("CmdOrCtrl+Shift+B")
        );
        assert_ne!(plan, plan_with(&[]), "a saved chord must redraw the bar");
    }

    #[test]
    fn an_override_moves_a_chord_between_items() {
        let plan = plan_with(&[
            ("note.new", "CmdOrCtrl+Shift+N"),
            ("sidebar.toggle", "CmdOrCtrl+N"),
        ]);
        assert_eq!(plan.accelerator_of("note.new"), Some("CmdOrCtrl+Shift+N"));
        assert_eq!(plan.accelerator_of("sidebar.toggle"), Some("CmdOrCtrl+N"));
    }

    #[test]
    fn a_macos_control_chord_stays_a_control_chord() {
        let plan = plan_with(&[("buffer.save", "Ctrl+S")]);
        assert_eq!(plan.accelerator_of("buffer.save"), Some("Ctrl+S"));
    }

    #[test]
    fn an_empty_override_unbinds_the_item() {
        let plan = plan_with(&[("buffer.save", "")]);
        assert_eq!(plan.accelerator_of("buffer.save"), None);
        assert!(plan
            .section(MenuSection::File)
            .any(|item| item.id == "buffer.save"));
    }

    #[test]
    fn a_chord_the_bar_holds_for_its_own_items_is_left_off() {
        let plan = plan_with(&[("sidebar.toggle", "CmdOrCtrl+Q")]);
        assert_eq!(plan.accelerator_of("sidebar.toggle"), None);
    }

    #[test]
    fn a_chord_two_items_claim_goes_to_the_first() {
        let plan = plan_with(&[("sidebar.toggle", "CmdOrCtrl+N")]);
        assert_eq!(plan.accelerator_of("note.new"), Some("CmdOrCtrl+N"));
        assert_eq!(plan.accelerator_of("sidebar.toggle"), None);
    }

    #[test]
    fn an_override_no_menu_can_carry_leaves_the_item_without_one() {
        for chord in [
            "Shift+Shift",
            "CmdOrCtrl+Shift++",
            "CmdOrCtrl+ى",
            "Hyper+N",
            "N",
            "CmdOrCtrl+CmdOrCtrl+N",
            "F0",
            "F01",
            "F25",
            "",
        ] {
            assert_eq!(accelerator_for_chord(chord), None, "{chord:?}");
        }
    }

    #[test]
    fn the_chords_the_shortcut_editor_records_are_accelerators() {
        for chord in [
            "CmdOrCtrl+N",
            "Ctrl+N",
            "CmdOrCtrl+Ctrl+N",
            "CmdOrCtrl+Shift+\\",
            "CmdOrCtrl+,",
            "CmdOrCtrl+Space",
            "Shift+Alt+ArrowUp",
            "CmdOrCtrl+Shift+0",
            "F2",
            "F24",
        ] {
            assert_eq!(accelerator_for_chord(chord).as_deref(), Some(chord));
        }
        for command in menu_commands() {
            if let Some(accelerator) = &command.accelerator {
                if accelerator == DOUBLE_SHIFT {
                    continue;
                }
                assert_eq!(
                    accelerator_for_chord(accelerator).as_ref(),
                    Some(accelerator),
                    "{}",
                    command.id
                );
            }
        }
    }

    #[test]
    fn the_bar_s_own_chords_are_listed_and_free_in_the_shared_list() {
        let native: HashSet<&str> = native_menu_chords()
            .iter()
            .map(|native| native.chord.as_str())
            .collect();
        assert!(native.contains("CmdOrCtrl+Q"));
        assert!(native.contains("CmdOrCtrl+H"));
        for command in menu_commands() {
            if let Some(accelerator) = command.accelerator.as_deref() {
                assert!(
                    !native.contains(accelerator),
                    "{} takes {accelerator}, which the bar holds",
                    command.id
                );
            }
        }
    }

    #[test]
    fn the_menus_teach_the_tab_zoom_and_rename_chords() {
        let plan = plan_with(&[]);
        for (id, chord) in [
            ("note.rename", "F2"),
            ("history.restoreLast", "CmdOrCtrl+Shift+T"),
            ("buffer.nextTab", "CmdOrCtrl+]"),
            ("buffer.prevTab", "CmdOrCtrl+["),
            ("editor.zoomIn", "CmdOrCtrl+="),
            ("editor.zoomOut", "CmdOrCtrl+-"),
            ("editor.zoomReset", "CmdOrCtrl+0"),
        ] {
            assert_eq!(plan.accelerator_of(id), Some(chord), "{id}");
        }
    }
}
