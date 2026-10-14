import data from "./menu-commands.json";
import nativeChords from "./native-menu-chords.json";
import type { Platform } from "../lib/platform";
import type { AppId } from "../types/config";

// The one list of commands both menus carry: the macOS menu bar built in
// `src-tauri/src/menu.rs` and the titlebar menu button `AppMenu.tsx` opens on
// Windows and Linux. The list lives in JSON so Rust reads the same bytes
// through `include_str!` rather than a second table that can drift.
//
// Labels here are the menu's own Title Case wording. `AppMenu.tsx` reads its
// labels and shortcuts from the command registry instead, so the platform word
// in "Show notes folder in …" stays whatever the host calls its file manager.

// No "window" section: macOS gets a Window menu built from the platform's own
// predefined items (`src-tauri/src/app_menu.rs`), which are chrome the OS
// performs rather than commands, and the Windows and Linux titlebars carry
// their own window buttons. A section the shared list can never fill would claim a
// parity the two menus do not have.
export type MenuSection = "app" | "file" | "edit" | "view" | "help";

export interface MenuCommandEntry {
  /** Command id, routed through the registry on both platforms. */
  id: string;
  /** The macOS menu bar's wording. */
  label: string;
  /** Tauri accelerator, absent for an item the menu bar shows without one.
   * `Shift+Shift` is no accelerator: the macOS bar writes it into the label. */
  accelerator?: string;
  menu: MenuSection;
  /** Items of one group sit together; a divider is drawn between groups. */
  group: number;
  platforms: readonly Platform[];
  /** The app the item belongs to; the macOS menu bar leaves it out while that
   * app is off (`src-tauri/src/menu.rs`). */
  app?: AppId;
  /** The submenu the item sits in on the macOS bar; the Windows and Linux
   * menu heads the same run with this name. Items naming one sit together. */
  submenu?: string;
}

export const MENU_COMMANDS: readonly MenuCommandEntry[] = data as readonly MenuCommandEntry[];

/** Order the menus render in. */
export const MENU_SECTIONS: readonly MenuSection[] = [
  "app",
  "file",
  "edit",
  "view",
  "help",
];

export function menuCommandsFor(platform: Platform): readonly MenuCommandEntry[] {
  return MENU_COMMANDS.filter((entry) => entry.platforms.includes(platform));
}

/**
 * The chords the macOS menu bar's own items hold: Quit, Hide, the Window and
 * Edit items AppKit provides. They are no commands, so the list above cannot
 * carry them; `src-tauri/src/menu.rs` reads the same file, and the shortcut
 * editor counts them as taken on macOS.
 */
export const NATIVE_MENU_CHORDS: readonly { chord: string; label: string }[] = nativeChords;

/** The chords no command can take on `platform`, because a menu answers them first. */
export function fixedMenuChords(platform: Platform): readonly { chord: string; label: string }[] {
  return platform === "mac" ? NATIVE_MENU_CHORDS : [];
}
