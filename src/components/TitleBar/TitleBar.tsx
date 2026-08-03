import { Show } from "solid-js";
import TabBar from "../Editor/TabBar";
import TrafficLights from "./TrafficLights";
import AppMenu from "./AppMenu";
import Kbd from "../Kbd/Kbd";
import { detectPlatform } from "../../lib/platform";
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

export default function TitleBar() {
  // Read per mount rather than at module load: a Solid component body runs once,
  // so this costs one navigator read and keeps the platform branch observable.
  const platform = detectPlatform();
  const trafficLightsOnLeft = platform === "mac";

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
    <div
      class={`titlebar titlebar-${platform}`}
      onMouseDown={handleMouseDown}
      onDblClick={handleDblClick}
    >
      <Show when={trafficLightsOnLeft}>
        <TrafficLights
          platform={platform}
          focused={osWindowStore.focused()}
          maximized={osWindowStore.maximized()}
        />
      </Show>
      <Show when={platform === "win" || platform === "linux"}>
        <AppMenu />
      </Show>
      <div class="titlebar-tabs">
        <TabBar />
      </div>
      <div class="titlebar-right" title="Toggle Writ from anywhere" data-no-drag>
        <Kbd binding={configStore.config().hotkey.toggle} />
      </div>
      <Show when={!trafficLightsOnLeft}>
        <TrafficLights
          platform={platform}
          focused={osWindowStore.focused()}
          maximized={osWindowStore.maximized()}
        />
      </Show>
    </div>
  );
}
