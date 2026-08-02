import { getCommand, executeCommand } from "../../commands/registry";
import { showAnchoredMenu, type MenuItem } from "../ContextMenu/ContextMenu";
import { formatKeybinding } from "../../lib/keybinding-format";

/**
 * The actions the macOS native menu bar exposes (`MENU_ACTION_IDS` in
 * `src-tauri/src/lib.rs`), plus the palette, which on a platform with no menu
 * bar is the other way to reach everything else. Labels and shortcuts are read
 * from the command registry rather than restated here, so this stays one list
 * of ids and never drifts into a second accelerator table.
 */
const MENU_COMMAND_IDS = [
  "file.open",
  "buffer.new",
  "buffer.close",
  "palette.open",
  "app.check_updates",
] as const;

/** Ids that open a group. A divider is drawn above them when they are not first. */
const GROUP_OPENERS: ReadonlySet<string> = new Set([
  "buffer.close",
  "palette.open",
  "app.check_updates",
]);

/**
 * Built per open, not once: commands register during `App`'s `onMount`, so a
 * list captured at module or component scope would read an empty registry.
 */
export function appMenuItems(): MenuItem[] {
  const items: MenuItem[] = [];
  for (const id of MENU_COMMAND_IDS) {
    const command = getCommand(id);
    if (!command) continue;
    items.push({
      label: command.label,
      kbd: formatKeybinding(command.keybinding) || undefined,
      separator: items.length > 0 && GROUP_OPENERS.has(id),
      action: () => {
        executeCommand(id);
      },
    });
  }
  return items;
}

/**
 * Windows/Linux have no menu bar to hang these actions on, so the titlebar
 * carries a single button that opens them. `ContextMenu` owns the popup,
 * including keyboard navigation and returning focus here on dismiss.
 */
export default function AppMenu() {
  let button: HTMLButtonElement | undefined;

  function openMenu() {
    if (!button) return;
    showAnchoredMenu(button.getBoundingClientRect(), appMenuItems(), button);
  }

  return (
    <button
      ref={button}
      type="button"
      class="titlebar-appmenu"
      aria-haspopup="menu"
      aria-label="Writ menu"
      onClick={openMenu}
    >
      Writ
    </button>
  );
}
