//! The native macOS menu bar, built from the shared command list
//! (`src/commands/menu-commands.json`, read through [`crate::menu`]).
//!
//! macOS-only by design: it hosts the system menu bar that macOS apps are
//! expected to provide, and its `CmdOrCtrl` accelerators are correct there. On
//! Windows/Linux the window runs with `decorations: false`, so this menu would
//! be invisible chrome while its accelerators collide with the platform
//! translator. Every command in the list is offered on all three platforms —
//! `AppMenu.tsx` carries the same list into the titlebar menu button — so
//! gating this off those platforms removes chrome, never an action.
//!
//! The bar holds only the items of the apps that are on, each with the chord
//! the shortcut editor saved for it, and is built again whenever either
//! changes (ADR-042 section 3). [`sync`] is what every path that changes the
//! config calls; it is a no-op off macOS and when the plan is the one already
//! drawn.

use tauri::AppHandle;

#[cfg(target_os = "macos")]
pub(crate) use imp::QUIT_MENU_ID;

/// Registers the click handler and draws the bar for the config Writ started
/// with. The handler is registered here once: a rebuild replaces the menu, not
/// the handler, so a click never raises its action twice.
#[cfg(target_os = "macos")]
pub fn install(app: &AppHandle) {
    use crate::state::AppState;
    use tauri::Manager;
    use writ_core::events::bus::WritEvent;

    app.on_menu_event(move |app_handle, event| {
        let id = event.id().0.as_str();
        if id == QUIT_MENU_ID {
            app_handle.exit(0);
            return;
        }
        if let Some(action) = crate::menu::menu_action_for_id(id) {
            let state = app_handle.state::<AppState>();
            state.event_bus.emit(WritEvent::MenuAction {
                action: action.command_id().to_string(),
            });
        }
    });
    imp::redraw(app);
}

/// Draws the bar again if the apps that are on, or the chords saved for its
/// items, differ from the ones it shows.
///
/// The build runs on the main thread, where AppKit wants its menus touched;
/// the watcher's reload calls this from a thread of its own.
pub fn sync(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let handle = app.clone();
        if let Err(error) = app.run_on_main_thread(move || imp::redraw(&handle)) {
            tracing::warn!(%error, "the menu bar could not be scheduled for a redraw");
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

#[cfg(target_os = "macos")]
mod imp {
    use crate::menu::{MenuPlan, MenuPlatform, MenuSection, PlannedItem};
    use crate::poison::recover_poison;
    use crate::state::AppState;
    use tauri::menu::{
        Menu, MenuBuilder, MenuItem, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder,
    };
    use tauri::{AppHandle, Manager, Wry};

    /// The Quit item's id. Not in the shared command list: quitting is the
    /// exit path's own business, not an action forwarded to the frontend.
    pub(crate) const QUIT_MENU_ID: &str = "app.quit";

    type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

    /// Builds and sets the bar when its plan has changed since the last one
    /// was set.
    pub(super) fn redraw(app: &AppHandle) {
        let state = app.state::<AppState>();
        let plan = {
            let config = recover_poison(state.config.lock(), "app_menu::redraw");
            MenuPlan::new(MenuPlatform::Mac, &config.apps_on(), &config.keybindings)
        };
        let mut drawn = recover_poison(state.menu_apps.lock(), "app_menu::redraw");
        if drawn.as_ref() == Some(&plan) {
            return;
        }
        match build(app, &plan).and_then(|bar| app.set_menu(bar).map_err(Into::into)) {
            Ok(_) => {
                tracing::info!("menu bar drawn");
                *drawn = Some(plan);
            }
            Err(error) => tracing::warn!(%error, "failed to build application menu"),
        }
    }

    fn menu_item(app: &AppHandle, planned: &PlannedItem) -> Result<MenuItem<Wry>> {
        let mut item = MenuItemBuilder::with_id(planned.id.clone(), &planned.label);
        if let Some(accelerator) = &planned.accelerator {
            item = item.accelerator(accelerator);
        }
        Ok(item.build(app)?)
    }

    /// Adds one menu's commands from the shared list, with a separator
    /// between groups. A run of items naming one submenu is drawn as that
    /// submenu, where its first item sits.
    fn fill_section<'m>(
        app: &'m AppHandle,
        builder: SubmenuBuilder<'m, Wry, AppHandle>,
        section: MenuSection,
        plan: &MenuPlan,
    ) -> Result<SubmenuBuilder<'m, Wry, AppHandle>> {
        let mut builder = builder;
        let mut last_group: Option<u8> = None;
        let items: Vec<&PlannedItem> = plan.section(section).collect();
        let mut at = 0;
        while let Some(planned) = items.get(at).copied() {
            if last_group.is_some_and(|group| group != planned.group) {
                builder = builder.separator();
            }
            last_group = Some(planned.group);

            let Some(name) = &planned.submenu else {
                builder = builder.item(&menu_item(app, planned)?);
                at += 1;
                continue;
            };
            let mut submenu = SubmenuBuilder::new(app, name);
            while let Some(inner) = items
                .get(at)
                .copied()
                .filter(|inner| inner.submenu.as_ref() == Some(name))
            {
                submenu = submenu.item(&menu_item(app, inner)?);
                at += 1;
            }
            builder = builder.item(&submenu.build()?);
        }
        Ok(builder)
    }

    fn build(app: &AppHandle, plan: &MenuPlan) -> Result<Menu<Wry>> {
        // Not `PredefinedMenuItem::quit`: muda maps that to AppKit's
        // `terminate:`, which tears the process down without ever reaching
        // the event loop, so no exit request is raised and nothing gets a
        // chance to write. A plain item that calls `AppHandle::exit` goes the
        // long way round, through `RunEvent::ExitRequested` and the flush
        // handshake.
        let quit_item = MenuItemBuilder::with_id(QUIT_MENU_ID, "Quit Writ")
            .accelerator("CmdOrCtrl+Q")
            .build(app)?;

        // About sits above the list's app section, so the licences the list
        // opens read as part of what About says about this build.
        let app_menu = SubmenuBuilder::new(app, "Writ").item(&PredefinedMenuItem::about(
            app,
            Some("About Writ"),
            None,
        )?);
        let app_menu = fill_section(app, app_menu, MenuSection::App, plan)?
            .separator()
            .hide_with_text("Hide Writ")
            .hide_others()
            .show_all()
            .separator()
            .item(&quit_item)
            .build()?;

        let file_menu = fill_section(
            app,
            SubmenuBuilder::new(app, "File"),
            MenuSection::File,
            plan,
        )?
        .build()?;

        let edit_menu = SubmenuBuilder::new(app, "Edit")
            .undo()
            .redo()
            .separator()
            .cut()
            .copy()
            .paste()
            .select_all()
            .separator();
        let edit_menu = fill_section(app, edit_menu, MenuSection::Edit, plan)?.build()?;

        let view_menu = fill_section(
            app,
            SubmenuBuilder::new(app, "View"),
            MenuSection::View,
            plan,
        )?
        .build()?;

        // Chrome macOS expects of every app, built from the predefined items
        // rather than from the shared list: these are window operations the
        // OS performs, not Writ commands, and there is nothing for the
        // Windows and Linux menu to carry in their place — those titlebars
        // have their own window buttons.
        let window_menu = SubmenuBuilder::new(app, "Window")
            .minimize()
            .maximize()
            .separator()
            .bring_all_to_front()
            .build()?;

        let help_menu = fill_section(
            app,
            SubmenuBuilder::new(app, "Help"),
            MenuSection::Help,
            plan,
        )?
        .build()?;

        Ok(MenuBuilder::new(app)
            .items(&[
                &app_menu,
                &file_menu,
                &edit_menu,
                &view_menu,
                &window_menu,
                &help_menu,
            ])
            .build()?)
    }
}
