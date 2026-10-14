import { For, Match, Show, Switch, createEffect, createSignal } from "solid-js";
import Button from "../Button/Button";
import { chatStore, type Readiness } from "../../stores/global/chat";
import { aiConnectionStore, type LocalProbe } from "../../stores/global/ai-connection";
import { aiProvidersStore } from "../../stores/global/ai-providers";
import { aiRewriteStore } from "../../stores/global/ai-rewrite";
import { configStore } from "../../stores/global/config";
import { openSettings } from "../SettingsModal/SettingsModal";
import { openConnectionControl } from "./ChatConnectionControl";
import { isImeComposing } from "../../lib/ime";

/** The local runtimes the probe answers for, by provider id. */
const RUNTIMES: { id: keyof LocalProbe; label: string }[] = [
  { id: "ollama", label: "Ollama" },
  { id: "lmstudio", label: "LM Studio" },
  { id: "llamacpp", label: "llama.cpp" },
  { id: "jan", label: "Jan" },
];

/** The one thing to press for each way a connection is not ready. */
export function recoveryLabel(readiness: Exclude<Readiness, { state: "ready" }>): string {
  if (readiness.state === "model_unavailable") return "Use the connection's model";
  if (readiness.state === "no_model") return "Choose a model";
  return readiness.action.kind === "check" ? "Retry detection" : "Open settings";
}

/**
 * What the connection still needs, said before a message is written rather
 * than after it is sent.
 *
 * The pane checks the connection as it opens, so this reads what the check
 * found. The two states a first use meets are answered here rather than in
 * Settings (CH-22): with no model, the runtimes running on this machine are
 * offered beside the model picker, and with no key, the key goes in a field on
 * the card.
 */
export default function ChatReadiness() {
  const state = () => chatStore.readiness();
  const [running, setRunning] = createSignal<typeof RUNTIMES>([]);
  const [key, setKey] = createSignal("");
  const [saving, setSaving] = createSignal(false);
  const [keyError, setKeyError] = createSignal<string | null>(null);

  const providerId = () => configStore.config().ai.provider;
  const providerLabel = () => aiProvidersStore.byId(providerId())?.label ?? providerId();

  // A model nobody has chosen is the first-use state: the local runtimes are
  // asked whether they are running, keyless, so the card can offer them.
  let probed = false;
  async function probeRuntimes() {
    try {
      const answer = await aiConnectionStore.probeLocal();
      setRunning(RUNTIMES.filter((row) => answer[row.id]));
    } catch {
      setRunning([]);
    }
  }
  createEffect(() => {
    if (state().state !== "no_model" || probed) return;
    probed = true;
    void probeRuntimes();
  });

  function recover(readiness: Exclude<Readiness, { state: "ready" }>) {
    if (readiness.state === "model_unavailable") {
      void aiConnectionStore.selectChatModel(null);
      return;
    }
    if (readiness.state === "no_model") {
      openConnectionControl();
      return;
    }
    if (readiness.action.kind === "check") {
      void aiConnectionStore.check();
      return;
    }
    openSettings(readiness.action.section, readiness.action.setting);
  }

  async function useRuntime(id: string) {
    await aiConnectionStore.selectProvider(id);
    await aiConnectionStore.check();
  }

  async function saveKey() {
    const typed = key().trim();
    if (!typed || saving()) return;
    setSaving(true);
    setKeyError(null);
    try {
      await aiRewriteStore.setApiKey(providerId(), typed);
      setKey("");
      await aiConnectionStore.check();
    } catch {
      setKeyError("The key was not saved.");
    } finally {
      setSaving(false);
    }
  }

  return (
    <Show when={state().state !== "ready" ? (state() as Exclude<Readiness, { state: "ready" }>) : null}>
      {(readiness) => (
        <div class="chat-readiness" role="status" data-state={readiness().state}>
          <div class="chat-readiness-body">
            <p class="chat-readiness-text">{readiness().message}</p>
            <Show when={readiness().state === "no_model"}>
              <p class="chat-readiness-help">Chat sends what you attach to a model you choose.</p>
            </Show>
          </div>
          <Switch
            fallback={
              <Button onClick={() => recover(readiness())}>{recoveryLabel(readiness())}</Button>
            }
          >
            <Match when={readiness().state === "no_model"}>
              <div class="chat-readiness-actions">
                <For each={running()}>
                  {(row) => (
                    <Button variant="primary" onClick={() => void useRuntime(row.id)}>
                      Use {aiProvidersStore.byId(row.id)?.label ?? row.label}
                    </Button>
                  )}
                </For>
                <Button onClick={() => recover(readiness())}>{recoveryLabel(readiness())}</Button>
              </div>
            </Match>
            <Match when={readiness().state === "no_key"}>
              <div class="chat-readiness-key">
                <input
                  class="chat-readiness-input"
                  type="password"
                  autocomplete="off"
                  spellcheck={false}
                  aria-label={`API key for ${providerLabel()}`}
                  placeholder="API key"
                  value={key()}
                  onInput={(event) => setKey(event.currentTarget.value)}
                  onKeyDown={(event) => {
                    if (isImeComposing(event)) return;
                    if (event.key === "Enter") {
                      event.preventDefault();
                      void saveKey();
                    }
                  }}
                />
                <Button
                  variant="primary"
                  disabled={key().trim().length === 0 || saving()}
                  onClick={() => void saveKey()}
                >
                  Save key
                </Button>
              </div>
              <Show when={keyError()}>
                {(why) => (
                  <p class="chat-readiness-error" role="alert">
                    {why()}
                  </p>
                )}
              </Show>
            </Match>
          </Switch>
        </div>
      )}
    </Show>
  );
}
