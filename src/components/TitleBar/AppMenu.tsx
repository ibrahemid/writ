import { Show } from "solid-js";
import { getCommand, executeCommand } from "../../commands/registry";
import { showAnchoredMenu, type MenuItem } from "../ContextMenu/ContextMenu";
import { formatKeybinding } from "../../lib/keybinding-format";
import { MENU_SECTIONS, menuCommandsFor } from "../../commands/menu-commands";
import { resolvePlatform } from "../../lib/platform";

/**
 * Built per open, not once: commands register during `App`'s `onMount`, so a
 * list captured at module or component scope would read an empty registry.
 *
 * The order, the grouping and the membership all come from the shared list
 * (`src/commands/menu-commands.json`), which is the same file the macOS menu
 * bar is built from. Labels and shortcuts are read from the command registry
 * rather than that list, so a label that names the platform's file manager
 * says the word this platform uses.
 */
export function appMenuItems(): MenuItem[] {
  const items: MenuItem[] = [];
  const available = menuCommandsFor(resolvePlatform());

  for (const section of MENU_SECTIONS) {
    const entries = available.filter((entry) => entry.menu === section);
    let lastGroup: number | null = null;

    for (const entry of entries) {
      const command = getCommand(entry.id);
      if (!command) continue;
      // A section always opens a run, so its first item divides from the one
      // above it as a group change does.
      const dividesFromPrevious = lastGroup === null || lastGroup !== entry.group;
      lastGroup = entry.group;
      items.push({
        label: command.label,
        kbd: formatKeybinding(command.keybinding) || undefined,
        separator: items.length > 0 && dividesFromPrevious,
        // Deferred so the command runs after `ContextMenu.close()` has put
        // focus back on the button: a surface that takes focus synchronously
        // would otherwise have it taken straight back.
        action: () => {
          queueMicrotask(() => executeCommand(entry.id));
        },
      });
    }
  }
  return items;
}

/**
 * Windows/Linux have no menu bar to hang these actions on, so the titlebar
 * carries a single button that opens them. `ContextMenu` owns the popup,
 * including keyboard navigation and returning focus here on dismiss.
 *
 * Passing the button as the trigger is what makes Escape land back on it.
 * `ContextMenu.close()` restores that focus right after the action runs, which
 * is why `appMenuItems` runs the command in a microtask: the close happens
 * first, and a surface that takes focus keeps it, whether it does so
 * synchronously or a frame later.
 */
interface Props {
  /** GNOME carries the primary menu as a glyph, not as the app name. */
  compact?: boolean;
}

export default function AppMenu(props: Props) {
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
      classList={{ "titlebar-appmenu-compact": props.compact }}
      aria-haspopup="menu"
      aria-label="Writ menu"
      onClick={openMenu}
    >
      <Show
        when={props.compact}
        fallback={
          <>
            Writ
            <svg width="8" height="8" viewBox="0 0 8 8" aria-hidden="true">
              <path
                d="M1.5 3L4 5.5L6.5 3"
                stroke="currentColor"
                stroke-width="1"
                stroke-linecap="round"
                stroke-linejoin="round"
                fill="none"
              />
            </svg>
          </>
        }
      >
        <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
          <path
            d="M2.5 4.5H13.5M2.5 8H13.5M2.5 11.5H13.5"
            stroke="currentColor"
            stroke-width="1.5"
            stroke-linecap="round"
          />
        </svg>
      </Show>
    </button>
  );
}
