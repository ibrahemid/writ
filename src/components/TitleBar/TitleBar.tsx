import { Show } from "solid-js";
import CaptionButtons from "./CaptionButtons";
import AppMenu from "./AppMenu";
import Button from "../Button/Button";
import Kbd from "../Kbd/Kbd";
import { resolvePlatform } from "../../lib/platform";
import { resolveChromeLayout } from "../../lib/window-chrome";
import { executeCommand } from "../../commands/registry";
import { configStore } from "../../stores/global/config";
import { osWindowStore } from "../../stores/global/os-window";
import "./TitleBar.css";

const INTERACTIVE_SELECTOR = 'button, input, select, [role="button"], [data-no-drag]';

export function isInteractiveTarget(target: EventTarget | null): boolean {
  // Element, not HTMLElement: a press landing on a caption button's glyph hits
  // an SVG element, which is not an HTMLElement and would read as bare titlebar
  // — dragging the window instead of clicking the button under the cursor.
  if (!(target instanceof Element)) return false;
  // This runs in a delegated ancestor handler, so closest() reads the live tree.
  // If a descendant handler detached the clicked node earlier in the same event,
  // closest() can no longer reach its interactive ancestor and would misread it
  // as the bare titlebar. The bare titlebar is never detached, so treat any
  // detached target as interactive: do not drag or maximize from it.
  if (!target.isConnected) return true;
  return Boolean(target.closest(INTERACTIVE_SELECTOR));
}

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

  function handleMouseDown(e: MouseEvent) {
    if (e.button !== 0) return;
    if (isInteractiveTarget(e.target)) return;
    e.preventDefault();
    osWindowStore.startDragging();
  }

  function handleDblClick(e: MouseEvent) {
    if (isInteractiveTarget(e.target)) return;
    osWindowStore.toggleMaximize();
  }

  return (
    <Show when={layout.titleBar}>
      <div
        class={`titlebar titlebar-${platform}`}
        classList={{ headerbar: layout.headerBar, "is-blurred": !osWindowStore.focused() }}
        onMouseDown={handleMouseDown}
        onDblClick={handleDblClick}
      >
        <div class="titlebar-start">
          <Show when={layout.composeInChrome}>
            <Button
              class="headerbar-compose"
              icon="note-pencil"
              onClick={() => executeCommand("note.new")}
            >
              New note
            </Button>
          </Show>
          <AppMenu compact={layout.headerBar} />
        </div>

        <Show when={layout.headerBar} fallback={<div class="titlebar-drag" />}>
          <div class="headerbar-title">Writ</div>
        </Show>

        <div class="titlebar-end">
          <div class="titlebar-right" title="Toggle Writ from anywhere" data-no-drag>
            <Kbd binding={configStore.config().hotkey.toggle} />
          </div>
          <CaptionButtons kind={layout.caption} maximized={osWindowStore.maximized()} />
        </div>
      </div>
    </Show>
  );
}
