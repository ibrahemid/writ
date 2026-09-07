//! The one list of menu commands, shared with the frontend.
//!
//! `src/commands/menu-commands.json` is the source: the macOS menu bar
//! ([`crate::build_app_menu`]) builds its items from it, and
//! `src/components/TitleBar/AppMenu.tsx` builds the Windows and Linux menu
//! from the same file. Nothing here is behind a `cfg`: the list and its
//! routing are the contract both menus answer to, so they compile — and are
//! tested — on every target.

use serde::Deserialize;
use std::sync::OnceLock;

const MENU_COMMANDS_JSON: &str = include_str!("../../src/commands/menu-commands.json");

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
    CloseTab,
    Find,
    FindNext,
    FindPrevious,
    Replace,
    ToggleSidebar,
    TogglePanel,
    OpenPalette,
    CustomizeShortcuts,
}

impl MenuAction {
    /// Every action the table routes to.
    pub const ALL: [MenuAction; 21] = [
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
        MenuAction::CloseTab,
        MenuAction::Find,
        MenuAction::FindNext,
        MenuAction::FindPrevious,
        MenuAction::Replace,
        MenuAction::ToggleSidebar,
        MenuAction::TogglePanel,
        MenuAction::OpenPalette,
        MenuAction::CustomizeShortcuts,
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
            MenuAction::CloseTab => "buffer.close",
            MenuAction::Find => "editor.find",
            MenuAction::FindNext => "editor.findNext",
            MenuAction::FindPrevious => "editor.findPrevious",
            MenuAction::Replace => "editor.replace",
            MenuAction::ToggleSidebar => "sidebar.toggle",
            MenuAction::TogglePanel => "panel.toggle",
            MenuAction::OpenPalette => "palette.open",
            MenuAction::CustomizeShortcuts => "shortcuts.customize",
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
}

impl MenuCommand {
    pub fn is_offered_on(&self, platform: MenuPlatform) -> bool {
        self.platforms.contains(&platform)
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

/// The commands one menu carries on one platform, in list order.
pub fn commands_in(
    section: MenuSection,
    platform: MenuPlatform,
) -> impl Iterator<Item = &'static MenuCommand> {
    menu_commands()
        .iter()
        .filter(move |command| command.menu == section && command.is_offered_on(platform))
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
        "buffer.close" => MenuAction::CloseTab,
        "editor.find" => MenuAction::Find,
        "editor.findNext" => MenuAction::FindNext,
        "editor.findPrevious" => MenuAction::FindPrevious,
        "editor.replace" => MenuAction::Replace,
        "sidebar.toggle" => MenuAction::ToggleSidebar,
        "panel.toggle" => MenuAction::TogglePanel,
        "palette.open" => MenuAction::OpenPalette,
        "shortcuts.customize" => MenuAction::CustomizeShortcuts,
        _ => return None,
    })
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
                commands_in(section, MenuPlatform::Mac).next().is_some(),
                "{section:?} is empty"
            );
        }
    }
}
