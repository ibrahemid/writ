import { Show } from "solid-js";
import { osWindowStore } from "../../stores/global/os-window";
import type { Platform } from "../../lib/platform";

interface Props {
  platform: Platform;
  focused: boolean;
}

export default function TrafficLights(props: Props) {
  return (
    <Show
      when={props.platform === "mac"}
      fallback={
        <div class="titlebar-controls titlebar-controls-win">
          <button
            type="button"
            class="winctrl winctrl-min"
            onClick={osWindowStore.minimize}
            title="Minimize"
            aria-label="Minimize window"
          >
            <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
              <path d="M1 5H9" stroke="currentColor" stroke-width="1" />
            </svg>
          </button>
          <button
            type="button"
            class="winctrl winctrl-max"
            onClick={osWindowStore.toggleMaximize}
            title="Maximize"
            aria-label="Maximize window"
          >
            <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
              <rect x="1.5" y="1.5" width="7" height="7" stroke="currentColor" stroke-width="1" fill="none" />
            </svg>
          </button>
          <button
            type="button"
            class="winctrl winctrl-close"
            onClick={osWindowStore.hide}
            title="Close"
            aria-label="Hide window"
          >
            <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
              <path d="M1.5 1.5L8.5 8.5M8.5 1.5L1.5 8.5" stroke="currentColor" stroke-width="1" />
            </svg>
          </button>
        </div>
      }
    >
      <div
        class={`titlebar-controls titlebar-controls-mac ${props.focused ? "" : "is-blurred"}`}
             >
        <button
          type="button"
          class="maclight maclight-close"
          onClick={osWindowStore.hide}
          title="Hide"
          aria-label="Hide window"
                 >
          <svg viewBox="0 0 12 12" aria-hidden="true">
            <path d="M3 3L9 9M9 3L3 9" stroke="#4d0000" stroke-width="1.25" stroke-linecap="round" />
          </svg>
        </button>
        <button
          type="button"
          class="maclight maclight-min"
          onClick={osWindowStore.minimize}
          title="Minimize"
          aria-label="Minimize window"
                 >
          <svg viewBox="0 0 12 12" aria-hidden="true">
            <path d="M2.5 6H9.5" stroke="#5a3300" stroke-width="1.25" stroke-linecap="round" />
          </svg>
        </button>
        <button
          type="button"
          class="maclight maclight-max"
          onClick={osWindowStore.toggleMaximize}
          title="Maximize"
          aria-label="Maximize window"
                 >
          <svg viewBox="0 0 12 12" aria-hidden="true">
            <path d="M4 4L4 7L7 4ZM8 8L8 5L5 8Z" fill="#003800" />
          </svg>
        </button>
      </div>
    </Show>
  );
}
