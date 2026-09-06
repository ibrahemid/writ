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
    Window,
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

#[derive(Debug, Clone, Deserialize)]
pub struct MenuCommand {
    /// The command id the frontend registry answers to. It is also the menu
    /// item's id, which is what makes [`menu_action_for_id`] a lookup rather
    /// than a translation table.
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

/// The action a menu item id forwards to the frontend, or `None` for an id the
/// shared list does not carry.
pub fn menu_action_for_id(id: &str) -> Option<&'static str> {
    menu_commands()
        .iter()
        .find(|command| command.id == id)
        .map(|command| command.id.as_str())
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
            assert_eq!(
                menu_action_for_id(&command.id),
                Some(command.id.as_str()),
                "{} is in the menu with no route",
                command.id
            );
        }
        let ids: HashSet<&str> = menu_commands().iter().map(|c| c.id.as_str()).collect();
        for command in menu_commands() {
            assert!(
                ids.contains(menu_action_for_id(&command.id).unwrap()),
                "{} routes to an action no item raises",
                command.id
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
