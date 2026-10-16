import { Show } from "solid-js";
import CaptionButtons from "./CaptionButtons";
import AppMenu from "./AppMenu";
import Button from "../Button/Button";
import Kbd from "../Kbd/Kbd";
import Tooltip from "../Tooltip/Tooltip";
import { resolvePlatform } from "../../lib/platform";
import { resolveChromeLayout } from "../../lib/window-chrome";
import { executeCommand } from "../../commands/registry";
import { WAYLAND_LINE } from "../ShortcutEditor/any-app-chord";
import { openWindowChordSetting } from "../SettingsModal/state";
import { formatKeybinding } from "../../lib/keybinding-format";
import { configStore } from "../../stores/global/config";
import { hotkeyStore } from "../../stores/global/hotkey";
import { osWindowStore } from "../../stores/global/os-window";
import { createTitleBarRegion } from "./title-bar-region";
import "./TitleBar.css";

/**
 * The decorated shells only. macOS draws no title bar at all: its lights move
 * into the sidebar head and the toolbar becomes the drag region, so
 * `resolveChromeLayout("mac").titleBar` is false and this renders nothing.
 */
export default function TitleBar() {
  // Read per mount rather than at module load: a Solid component body runs once,
  // so this costs one navigator read and keeps the platform branch observable.
  const platform = resolvePlatform();
  const layout = resolveChromeLayout(platform);

  const region = createTitleBarRegion({ drag: true });

  const windowChord = () => hotkeyStore.chord() || configStore.config().hotkey.toggle;
  // The chord does nothing from other apps: another app holds it, or the
  // session cannot deliver it. The keycap goes quiet and says which.
  const chordUnheld = () => hotkeyStore.isTaken() || hotkeyStore.isUnsupported();
  const unheldLine = () =>
    hotkeyStore.isTaken()
      ? `Another app is using ${formatKeybinding(windowChord())}`
      : WAYLAND_LINE;

  return (
    <Show when={layout.titleBar}>
      <div
        class={`titlebar titlebar-${platform}`}
        classList={{ headerbar: layout.headerBar, "is-blurred": !osWindowStore.focused() }}
        onMouseDown={region.onMouseDown}
        onDblClick={region.onDblClick}
      >
        <div class="titlebar-start">
          <Show when={layout.composeInChrome}>
            <Button
              class="headerbar-compose"
              icon="note-pencil"
              onClick={() => executeCommand("note.new")}
            >
              New file
            </Button>
          </Show>
          <AppMenu compact={layout.headerBar} />
        </div>

        <Show when={layout.headerBar} fallback={<div class="titlebar-drag" />}>
          <div class="headerbar-title">Writ</div>
        </Show>

        <div class="titlebar-end">
          <Show
            when={chordUnheld()}
            fallback={
              <Tooltip label="Toggle Writ from anywhere">
                <div class="titlebar-right" data-no-drag>
                  <Kbd binding={windowChord()} />
                </div>
              </Tooltip>
            }
          >
            <Tooltip label={unheldLine()}>
              <button
                type="button"
                class="titlebar-right titlebar-chord-unheld"
                data-no-drag
                data-state={hotkeyStore.isTaken() ? "taken" : "unsupported"}
                aria-label={unheldLine()}
                onClick={() => openWindowChordSetting(hotkeyStore.isTaken())}
              >
                <Kbd binding={windowChord()} muted />
              </button>
            </Tooltip>
          </Show>
          <CaptionButtons kind={layout.caption} maximized={osWindowStore.maximized()} />
        </div>
      </div>
    </Show>
  );
}
