import { osWindowStore } from "../../stores/global/os-window";

interface Props {
  focused: boolean;
}

/**
 * The macOS lights. The window carries no title bar, so `WindowLights` pins
 * these to its leading edge for good — never nowhere, since a window with no
 * native decorations has no other way left to hide itself.
 */
export default function TrafficLights(props: Props) {
  return (
    <div class="window-lights" classList={{ "is-blurred": !props.focused }}>
      <button
        type="button"
        class="maclight maclight-close"
        onClick={osWindowStore.hide}
        title="Hide"
        aria-label="Hide window"
      >
        <svg viewBox="0 0 12 12" aria-hidden="true">
          <path d="M3 3L9 9M9 3L3 9" stroke-width="1.25" stroke-linecap="round" />
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
          <path d="M2.5 6H9.5" stroke-width="1.25" stroke-linecap="round" />
        </svg>
      </button>
      <button
        type="button"
        class="maclight maclight-max"
        onClick={osWindowStore.toggleFullscreen}
        title="Full Screen"
        aria-label="Toggle full screen"
      >
        <svg viewBox="0 0 12 12" aria-hidden="true">
          <path d="M4 4L4 7L7 4ZM8 8L8 5L5 8Z" />
        </svg>
      </button>
    </div>
  );
}
