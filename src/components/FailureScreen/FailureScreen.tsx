import { Show } from "solid-js";
import Button from "../Button/Button";
import CaptionButtons from "../TitleBar/CaptionButtons";
import WindowLights from "../TitleBar/WindowLights";
import { isInteractiveTarget } from "../TitleBar/title-bar-region";
import { clipboardStore } from "../../stores/global/clipboard";
import { showToast } from "../../stores/global/notifications";
import { osWindowStore } from "../../stores/global/os-window";
import { resolvePlatform } from "../../lib/platform";
import { resolveChromeLayout } from "../../lib/window-chrome";
import "../TitleBar/TitleBar.css";
import "./FailureScreen.css";

interface Props {
  heading: string;
  message: string;
  /** What "Copy the details" copies, shown under the message. */
  details: string;
  retryLabel: string;
  onRetry: () => void;
  /**
   * Draws the window's own controls. The crash screen replaces the title bar
   * and the lights along with everything else, and without them a window with
   * no native decorations can be neither moved nor closed.
   */
  withWindowControls?: boolean;
}

/** The screen a window shows when it cannot show the editor. */
export default function FailureScreen(props: Props) {
  const layout = resolveChromeLayout(resolvePlatform());

  function onBarMouseDown(event: MouseEvent) {
    if (event.button !== 0 || isInteractiveTarget(event.target)) return;
    event.preventDefault();
    void osWindowStore.startDragging();
  }

  async function onCopy() {
    try {
      await clipboardStore.copyText(props.details);
      showToast("Copied the details", "success");
    } catch {
      showToast("Could not copy the details", "error");
    }
  }

  return (
    <div class="failure-screen">
      <div class="failure-screen-bar" onMouseDown={onBarMouseDown}>
        <Show when={props.withWindowControls && layout.caption !== "none"}>
          <CaptionButtons kind={layout.caption} maximized={osWindowStore.maximized()} />
        </Show>
      </div>
      <Show when={props.withWindowControls}>
        <WindowLights />
      </Show>
      <div class="failure-screen-body" role="alert">
        <div class="failure-screen-title">{props.heading}</div>
        <p class="failure-screen-text">{props.message}</p>
        <pre class="failure-screen-details">{props.details}</pre>
        <div class="failure-screen-actions">
          <Button variant="primary" onClick={() => props.onRetry()}>
            {props.retryLabel}
          </Button>
          <Button data-action="copy-crash-details" onClick={() => void onCopy()}>
            Copy the details
          </Button>
        </div>
      </div>
    </div>
  );
}
