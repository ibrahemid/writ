import { Show } from "solid-js";
import { resolvePlatform } from "../../lib/platform";
import { resolveLightsSlot } from "../../lib/window-chrome";
import { osWindowStore } from "../../stores/global/os-window";
import TrafficLights from "./TrafficLights";

/**
 * The one host for the macOS lights, pinned to the window's leading edge and
 * layered over the sidebar and the toolbar rather than parented to either. Both
 * of those move while the sidebar opens and closes; the lights do not.
 *
 * The layer itself takes no pointer events, so the 44px band around the lights
 * still reaches the drag region underneath. `deep` covers the gaps inside the
 * lights row, which the layer does take events for.
 */
export default function WindowLights() {
  const platform = resolvePlatform();
  return (
    <Show when={resolveLightsSlot(platform) === "window-lead"}>
      <div class="window-lights-layer" data-tauri-drag-region="deep">
        <TrafficLights focused={osWindowStore.focused()} />
      </div>
    </Show>
  );
}
