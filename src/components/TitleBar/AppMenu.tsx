import { Show } from "solid-js";
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
  "note.new",
  "file.open",
  "note.rename",
  "note.saveCopy",
  "buffer.close",
  "palette.open",
  "app.check_updates",
] as const;

/** Ids that open a group. A divider is drawn above them when they are not first. */
const GROUP_OPENERS: ReadonlySet<string> = new Set([
  "note.rename",
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
 *
 * Passing the button as the trigger is what makes Escape land back on it. The
 * cost: `ContextMenu.close()` restores that focus synchronously right after the
 * action runs, so an entry added here must not focus its surface synchronously
 * or the button steals it straight back. Every command listed above defers
 * (the palette focuses in a requestAnimationFrame, the tab commands are async),
 * which is why the trigger is safe to pass today.
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
