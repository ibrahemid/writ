import { Show } from "solid-js";
import Button from "../Button/Button";
import { chatStore, type Readiness } from "../../stores/global/chat";
import { aiConnectionStore } from "../../stores/global/ai-connection";
import { openSettings } from "../SettingsModal/SettingsModal";

/** The one thing to press for each way a connection is not ready. */
export function recoveryLabel(readiness: Exclude<Readiness, { state: "ready" }>): string {
  if (readiness.state === "model_unavailable") return "Use the connection's model";
  return readiness.action.kind === "check" ? "Retry detection" : "Open settings";
}

/**
 * What the connection still needs, said before a message is written rather
 * than after it is sent.
 *
 * The store reads this from evidence only, so a connection nobody has checked
 * shows nothing at all: the line appears when something answered.
 */
export default function ChatReadiness() {
  const state = () => chatStore.readiness();

  function recover(readiness: Exclude<Readiness, { state: "ready" }>) {
    if (readiness.state === "model_unavailable") {
      void aiConnectionStore.selectChatModel(null);
      return;
    }
    if (readiness.action.kind === "check") {
      void aiConnectionStore.check();
      return;
    }
    openSettings(readiness.action.section, readiness.action.setting);
  }

  return (
    <Show when={state().state !== "ready" ? (state() as Exclude<Readiness, { state: "ready" }>) : null}>
      {(readiness) => (
        <div class="chat-readiness" role="status" data-state={readiness().state}>
          <p class="chat-readiness-text">{readiness().message}</p>
          <Button onClick={() => recover(readiness())}>{recoveryLabel(readiness())}</Button>
        </div>
      )}
    </Show>
  );
}
