import { Show } from "solid-js";
import { resolvePlatform } from "../../lib/platform";
import { resolveLightsSlot } from "../../lib/window-chrome";
import { osWindowStore } from "../../stores/global/os-window";
import TrafficLights from "./TrafficLights";
import { createTitleBarRegion } from "./title-bar-region";

/**
 * The one host for the macOS lights, pinned to the window's leading edge and
 * layered over the sidebar and the toolbar rather than parented to either. Both
 * of those move while the sidebar opens and closes; the lights do not.
 *
 * The layer itself takes no pointer events, so the 44px band around the lights
 * still reaches the chrome underneath. The gaps inside the lights row do take
 * them, and the layer's handlers keep those moving and zooming the window.
 */
export default function WindowLights() {
  const platform = resolvePlatform();
  const region = createTitleBarRegion({ drag: true });
  return (
    <Show when={resolveLightsSlot(platform) === "window-lead"}>
      <div class="window-lights-layer" onMouseDown={region.onMouseDown} onDblClick={region.onDblClick}>
        <TrafficLights focused={osWindowStore.focused()} />
      </div>
    </Show>
  );
}
