import { createSignal, onMount, onCleanup, Show } from "solid-js";
import TabBar from "../Editor/TabBar";
import TrafficLights from "./TrafficLights";
import Kbd from "../Kbd/Kbd";
import { detectPlatform } from "../../lib/platform";
import { configStore } from "../../stores/config";
import { onWindowFocusChange, startDraggingWindow, toggleMaximizeWindow } from "../../services/tauri";
import "./TitleBar.css";

const PLATFORM = detectPlatform();
const TRAFFIC_LIGHTS_ON_LEFT = PLATFORM === "mac";
const INTERACTIVE_SELECTOR = 'button, input, select, [role="button"], [data-no-drag]';

function isInteractiveTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return Boolean(target.closest(INTERACTIVE_SELECTOR));
}

export default function TitleBar() {
  const [focused, setFocused] = createSignal(true);
  let unlisten: (() => void) | undefined;

  onMount(async () => {
    unlisten = await onWindowFocusChange(setFocused);
  });

  onCleanup(() => {
    unlisten?.();
  });

  function handleMouseDown(e: MouseEvent) {
    if (e.button !== 0) return;
    if (isInteractiveTarget(e.target)) return;
    e.preventDefault();
    startDraggingWindow();
  }

  function handleDblClick(e: MouseEvent) {
    if (isInteractiveTarget(e.target)) return;
    toggleMaximizeWindow();
  }

  return (
    <div
      class={`titlebar titlebar-${PLATFORM}`}
      onMouseDown={handleMouseDown}
      onDblClick={handleDblClick}
    >
      <Show when={TRAFFIC_LIGHTS_ON_LEFT}>
        <TrafficLights platform={PLATFORM} focused={focused()} />
      </Show>
      <div class="titlebar-tabs">
        <TabBar />
      </div>
      <div class="titlebar-right" title="Toggle Writ from anywhere" data-no-drag>
        <Kbd binding={configStore.config().hotkey.toggle} />
      </div>
      <Show when={!TRAFFIC_LIGHTS_ON_LEFT}>
        <TrafficLights platform={PLATFORM} focused={focused()} />
      </Show>
    </div>
  );
}
