import { Show, onMount, onCleanup } from "solid-js";
import { osWindowStore } from "../../stores/global/os-window";
import type { Platform } from "../../lib/platform";

interface Props {
  platform: Platform;
  focused: boolean;
  maximized: boolean;
}

export default function TrafficLights(props: Props) {
  let maximizeRef: HTMLButtonElement | undefined;

  onMount(() => {
    if (props.platform !== "win" || !maximizeRef) return;
    // ResizeObserver is absent in jsdom, and this path only runs on Windows.
    if (typeof ResizeObserver === "undefined") return;

    const button = maximizeRef;
    let dispose: (() => void) | null = null;
    let disposed = false;

    // The report waits for the first layout that measures the button: the
    // window can still be hidden at mount, where a zero-sized measurement is
    // rejected outright and there would be no second attempt. One report is
    // still all it takes afterwards, since the measurement is a distance from
    // the window's right edge and the titlebar's metrics are fixed tokens.
    const observer = new ResizeObserver(() => {
      const rect = button.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) return;
      observer.disconnect();

      void osWindowStore
        .installSnapOverlay({
          offsetFromRight: window.innerWidth - rect.right,
          top: rect.top,
          width: rect.width,
          height: rect.height,
        })
        .then((teardown) => {
          if (disposed) teardown();
          else dispose = teardown;
        });
    });
    observer.observe(button);

    onCleanup(() => {
      disposed = true;
      observer.disconnect();
      dispose?.();
    });
  });

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
              <path d="M1 5.5H9" stroke="currentColor" stroke-width="1" />
            </svg>
          </button>
          {/* Windows sends this button's pointer state back over IPC, from the
              child window it hit-tests as the caption maximize button. That
              window swallows the real mouse events, so hover and press arrive
              as classes and the DOM click path is left for the keyboard. */}
          <button
            ref={maximizeRef}
            type="button"
            class="winctrl winctrl-max"
            classList={{
              "is-snap-hovered": osWindowStore.snapHovered(),
              "is-snap-pressed": osWindowStore.snapPressed(),
            }}
            onClick={osWindowStore.toggleMaximize}
            title={props.maximized ? "Restore" : "Maximize"}
            aria-label={props.maximized ? "Restore window" : "Maximize window"}
          >
            <Show
              when={props.maximized}
              fallback={
                <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
                  <rect x="1.5" y="1.5" width="7" height="7" stroke="currentColor" stroke-width="1" fill="none" />
                </svg>
              }
            >
              <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
                <rect x="0.5" y="2.5" width="6" height="6" stroke="currentColor" stroke-width="1" fill="none" />
                <path d="M2.5 2.5V0.5H8.5V6.5H6.5" stroke="currentColor" stroke-width="1" fill="none" />
              </svg>
            </Show>
          </button>
          <button
            type="button"
            class="winctrl winctrl-close"
            onClick={osWindowStore.hide}
            title="Hide"
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
    </Show>
  );
}
