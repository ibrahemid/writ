import { Show } from "solid-js";
import TabBar from "../Editor/TabBar";
import TrafficLights from "./TrafficLights";
import Kbd from "../Kbd/Kbd";
import { detectPlatform } from "../../lib/platform";
import { configStore } from "../../stores/config";
import { osWindowStore } from "../../stores/os-window";
import "./TitleBar.css";

const PLATFORM = detectPlatform();
const TRAFFIC_LIGHTS_ON_LEFT = PLATFORM === "mac";
const INTERACTIVE_SELECTOR = 'button, input, select, [role="button"], [data-no-drag]';

function isInteractiveTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return Boolean(target.closest(INTERACTIVE_SELECTOR));
}

export default function TitleBar() {
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
      class={`titlebar titlebar-${PLATFORM}`}
      onMouseDown={handleMouseDown}
      onDblClick={handleDblClick}
    >
      <Show when={TRAFFIC_LIGHTS_ON_LEFT}>
        <TrafficLights platform={PLATFORM} focused={osWindowStore.focused()} />
      </Show>
      <div class="titlebar-tabs">
        <TabBar />
      </div>
      <div class="titlebar-right" title="Toggle Writ from anywhere" data-no-drag>
        <Kbd binding={configStore.config().hotkey.toggle} />
      </div>
      <Show when={!TRAFFIC_LIGHTS_ON_LEFT}>
        <TrafficLights platform={PLATFORM} focused={osWindowStore.focused()} />
      </Show>
    </div>
  );
}
