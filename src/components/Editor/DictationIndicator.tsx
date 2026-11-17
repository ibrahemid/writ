import { Show } from "solid-js";
import { configStore } from "../../stores/global/config";
import { useWindow } from "../WindowProvider/WindowProvider";
import Icon from "../Icon/Icon";
import Tooltip from "../Tooltip/Tooltip";
import "./DictationIndicator.css";

/** `seconds` as m:ss. */
export function formatElapsed(seconds: number): string {
  const whole = Math.max(0, Math.floor(seconds));
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")}`;
}

/** The status-bar mark of a live dictation session; pressing it stops the
 * session. Absent while Dictation is off or nothing is being dictated. */
export default function DictationIndicator() {
  const win = useWindow();
  const dictation = () => win.dictation;
  const shown = () => configStore.isAppOn("dictation") && dictation().isLive();
  const stopping = () => dictation().phase() === "stopping";
  const label = () => (stopping() ? "Transcribing" : "Stop dictation");

  const note = (): string | null => {
    const phase = dictation().phase();
    if (phase === "preparing") {
      const progress = dictation().progress();
      return progress === null
        ? "Getting the speech model"
        : `Getting the speech model ${Math.round(Math.min(1, Math.max(0, progress)) * 100)}%`;
    }
    return phase === "stopping" && dictation().pending() > 0 ? "Transcribing" : null;
  };

  return (
    <Show when={shown()}>
      <Tooltip label={label()} kbd={stopping() ? undefined : "dictation.toggle"}>
        <button
          type="button"
          class="dictation-indicator"
          classList={{ "is-recording": dictation().phase() === "recording" }}
          aria-label={label()}
          aria-disabled={stopping() ? "true" : undefined}
          onClick={() => void dictation().stop()}
        >
          <Icon name="microphone" size={13} class="dictation-indicator-icon" />
          <Show when={dictation().phase() === "recording"}>
            <span
              class="dictation-indicator-level"
              style={{ "--dictation-level": String(dictation().level()) }}
              aria-hidden="true"
            >
              <span class="dictation-indicator-level-fill" />
            </span>
          </Show>
          <Show when={dictation().elapsedSeconds() !== null}>
            <span class="dictation-indicator-time">
              {formatElapsed(dictation().elapsedSeconds() ?? 0)}
            </span>
          </Show>
          <Show when={note()}>{(text) => <span class="dictation-indicator-note">{text()}</span>}</Show>
        </button>
      </Tooltip>
    </Show>
  );
}
