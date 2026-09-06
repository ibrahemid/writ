import data from "./menu-commands.json";
import type { Platform } from "../lib/platform";

// The one list of commands both menus carry: the macOS menu bar built in
// `src-tauri/src/menu.rs` and the titlebar menu button `AppMenu.tsx` opens on
// Windows and Linux. The list lives in JSON so Rust reads the same bytes
// through `include_str!` rather than a second table that can drift.
//
// Labels here are the menu's own Title Case wording. `AppMenu.tsx` reads its
// labels and shortcuts from the command registry instead, so the platform word
// in "Show notes folder in …" stays whatever the host calls its file manager.
//
// The folder graph gets a View entry when it lands; it is not in this list yet
// because the view it opens does not exist.

export type MenuSection = "app" | "file" | "edit" | "view" | "window" | "help";

export interface MenuCommandEntry {
  /** Command id, routed through the registry on both platforms. */
  id: string;
  /** The macOS menu bar's wording. */
  label: string;
  /** Tauri accelerator, absent for an item the menu bar shows without one. */
  accelerator?: string;
  menu: MenuSection;
  /** Items of one group sit together; a divider is drawn between groups. */
  group: number;
  platforms: readonly Platform[];
}

export const MENU_COMMANDS: readonly MenuCommandEntry[] = data as readonly MenuCommandEntry[];

/** Order the menus render in. */
export const MENU_SECTIONS: readonly MenuSection[] = [
  "app",
  "file",
  "edit",
  "view",
  "window",
  "help",
];

export function menuCommandsFor(platform: Platform): readonly MenuCommandEntry[] {
  return MENU_COMMANDS.filter((entry) => entry.platforms.includes(platform));
}
