import { For, createMemo } from "solid-js";
import { configStore } from "../../stores/global/config";
import { modelOptions } from "../../stores/global/ai-models";
import { showToast } from "../Notifications/Toast";

/** The value that stands for "whatever the connection is set to". */
export const CONNECTION_MODEL = "";

/** The ids the picker offers: the connection's own first, then the rest. */
export function pickerOptions(connectionModel: string, options: readonly string[]): string[] {
  const rest = options.filter((id) => id !== connectionModel);
  return connectionModel ? [connectionModel, ...rest] : rest;
}

/**
 * Which model this chat answers with.
 *
 * The connection's own model is the first row and the one a fresh chat uses;
 * picking another writes `ai.chat.model`, which is an override for chat alone
 * and leaves the connection where it is.
 */
export default function ModelPicker(props: { live: readonly string[] }) {
  const ai = () => configStore.config().ai;
  const options = createMemo(() => pickerOptions(ai().model, modelOptions(ai().provider, props.live)));
  const chosen = () => ai().chat.model || CONNECTION_MODEL;

  async function choose(id: string) {
    const previous = configStore.config();
    const next = id === ai().model ? CONNECTION_MODEL : id;
    if (next === previous.ai.chat.model) return;
    try {
      await configStore.save({
        ...previous,
        ai: { ...previous.ai, chat: { ...previous.ai.chat, model: next } },
      });
    } catch {
      showToast("Could not save your settings", "error");
    }
  }

  return (
    <select
      class="chat-model"
      aria-label="Model"
      value={chosen()}
      onChange={(event) => void choose(event.currentTarget.value)}
    >
      <For each={options()}>
        {(id) => (
          <option value={id === ai().model ? CONNECTION_MODEL : id}>
            {id === ai().model ? `${id} (default)` : id}
          </option>
        )}
      </For>
    </select>
  );
}
