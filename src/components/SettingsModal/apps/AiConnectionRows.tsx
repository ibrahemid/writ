import { createEffect, createSignal, For, Show } from "solid-js";
import { configStore } from "../../../stores/global/config";
import { showToast } from "../../../stores/global/notifications";
import {
  aiRewriteStore,
  type AiKeyState,
  type AiEndpointState,
} from "../../../stores/global/ai-rewrite";
import {
  aiConnectionStore,
  connectionDisplay,
  modelListDisplay,
  CHOOSE_A_MODEL,
  type LocalProbe,
  type ModelListError,
} from "../../../stores/global/ai-connection";
import { aiProvidersStore } from "../../../stores/global/ai-providers";
import { modelOptions, resolveAutoModel } from "../../../stores/global/ai-models";
import { linkStore } from "../../../stores/global/link";
import Button from "../../Button/Button";
import { patchConfig } from "../controls";
import { settingTitle } from "../../../settings";
import { SettingsRow, ToggleSwitch } from "../SettingsRow";

const OLLAMA_DOWNLOAD_URL = "https://ollama.com/download";

/** The one-line help under a local runtime that did not answer its port. The
 * probe cannot tell "installed but stopped" from "not installed", so each line
 * states the fact it knows and, for Ollama, the link covers the other case. */
const LOCAL_HELP: Record<string, string> = {
  ollama: "Writ could not reach Ollama on port 11434.",
  lmstudio: "Writ could not reach LM Studio on port 1234. Start its server in the Developer tab.",
};

/** The one connection Chat and Rewrite share (ADR-040 section 1), drawn on the
 * page of each while either is on (ADR-042 section 4).
 *
 * Every row reads the provider table `writ-core` owns, so the base URL, the
 * key page, the default model and the probe port are never spelled out here. */
export default function AiConnectionRows(props: { withChatModel: boolean }) {
  const cfg = () => configStore.config().ai;
  const [keyState, setKeyState] = createSignal<AiKeyState | null>(null);
  const [keyInput, setKeyInput] = createSignal("");
  const [keyBusy, setKeyBusy] = createSignal(false);
  const [connecting, setConnecting] = createSignal(false);
  const [probe, setProbe] = createSignal<LocalProbe | null>(null);
  // The catalog lives in the connection store, so the panel and the chat pane
  // read one list and it is stamped with the provider it was read for.
  const liveModels = (): string[] => {
    const held = aiConnectionStore.catalog();
    return held?.source === "live" ? held.models : [];
  };
  const listError = (): ModelListError | null => aiConnectionStore.catalog()?.error ?? null;
  // The user has picked (or typed) the current model this session; guards the
  // Ollama auto-select from replacing a deliberate choice.
  const [userSelected, setUserSelected] = createSignal(false);
  // Free-text model entry is showing (chose "Custom…", or the model is not in
  // the offered list).
  const [customMode, setCustomMode] = createSignal(false);
  // The chat is set to a model of its own. Opening the disclosure seeds it, so
  // the select always shows the model chat would actually use.
  const [chatModelOpen, setChatModelOpen] = createSignal(cfg().chat.model !== "");

  const provider = () => aiProvidersStore.byId(cfg().provider);
  const isLocal = () => provider()?.group === "local";
  const isCustom = () => cfg().provider === "custom";
  const needsKey = () => provider()?.needs_key === true || (isCustom() && !isLocal());
  const keyPageUrl = () => provider()?.key_page_url ?? null;

  void aiProvidersStore.load();

  /** Whether the selected local runtime answered. Unknown until the probe
   * lands, which is what keeps the pill off a row that was never probed. */
  const localRunning = (): boolean | null => {
    const p = probe();
    if (!p || !isLocal()) return null;
    return cfg().provider === "ollama" ? p.ollama : p.lmstudio;
  };

  async function refreshProbe(): Promise<void> {
    try {
      setProbe(await aiConnectionStore.probeLocal());
    } catch {
      setProbe(null);
    }
  }

  async function refreshModels(): Promise<void> {
    await aiConnectionStore.refreshCatalog();
  }

  // The table, the probe and the model list, once the section can be seen and
  // again whenever the provider changes. The probe answers both local rows at
  // once, so the pill on either is one request.
  createEffect(() => {
    void cfg().provider;
    void aiProvidersStore.load();
    void refreshProbe();
    void refreshModels();
  });

  // The key belongs to the provider, so reading it follows the provider.
  createEffect(() => {
    const id = cfg().provider;
    void aiRewriteStore
      .hasApiKey(id)
      .then(setKeyState)
      .catch(() => setKeyState(null));
  });

  // Check the connection when the section opens and whenever the target
  // changes, debounced so typing a URL does not fire a request per keystroke.
  createEffect(() => {
    // Track the fields that define the target. `consented_hosts` is one of
    // them: a hosted endpoint is not reached before consent, so granting it is
    // what makes the next check possible.
    void cfg().provider;
    void cfg().base_url;
    void cfg().model;
    void cfg().consented_hosts.join("\n");
    aiConnectionStore.scheduleCheck();
  });

  const connection = () => {
    const failed = listError();
    if (failed && !aiConnectionStore.status()) {
      return modelListDisplay(failed, endpointHost());
    }
    return connectionDisplay(aiConnectionStore.status(), cfg().model);
  };

  const modelOptionList = () => modelOptions(cfg().provider, liveModels());
  const usingCurated = () => liveModels().length === 0;
  // An empty model is a choice still to make, so the list says there is none
  // rather than showing an empty custom id.
  const choosingModel = () => !customMode() && !cfg().model.trim();
  const modelSelectValue = () => {
    if (customMode()) return "__custom__";
    if (choosingModel()) return "";
    return modelOptionList().includes(cfg().model) ? cfg().model : "__custom__";
  };
  const showModelInput = () =>
    customMode() || (!choosingModel() && !modelOptionList().includes(cfg().model));
  const modelOptionLabel = (id: string) => (usingCurated() ? `${id} (suggested)` : id);

  // Keep a working model without a setup decision: fill an empty one, and for a
  // local server that has just listed its models, switch off a not-installed id.
  createEffect(() => {
    const auto = resolveAutoModel({
      provider: cfg().provider,
      model: cfg().model,
      live: liveModels(),
      userSelected: userSelected(),
    });
    if (auto && auto !== cfg().model) {
      void patchConfig((prev) => ({ ...prev, ai: { ...prev.ai, model: auto } }));
    }
  });

  // Where the endpoint points and what it still needs. Resolved in Rust by the
  // same code the request guard uses, so this panel can never disagree with the
  // guard about which host consent applies to.
  const [endpoint, setEndpoint] = createSignal<AiEndpointState | null>(null);

  createEffect(() => {
    // Track the fields that change the answer.
    void cfg().base_url;
    void cfg().provider;
    void cfg().consented_hosts.join("\n");
    void aiRewriteStore
      .endpointState()
      .then(setEndpoint)
      .catch(() => setEndpoint(null));
  });

  const endpointHost = () => {
    const e = endpoint();
    return e?.host_port ?? e?.host ?? "";
  };

  // Hosted (non-local) and not yet consented to — shown for a provider switch
  // or a hand-edited base URL alike.
  const hostedUnconsented = () => {
    const e = endpoint();
    return Boolean(e?.is_allowed && e.is_hosted && !e.is_consented);
  };

  const showLocalLine = () => Boolean(isLocal() && endpointHost());

  function seedProvider(id: string) {
    // A new provider is a fresh context: reset the selection guard, and let
    // the connection store make the change. Rust seeds the row's model and
    // drops a chat model that belonged to the old provider, so the panel and
    // the pane cannot clear an override differently.
    setUserSelected(false);
    setCustomMode(id === "custom");
    setChatModelOpen(false);
    void aiConnectionStore.selectProvider(id).catch(() => {
      showToast("Could not save your settings", "error");
    });
  }

  function onProviderChange(raw: string) {
    seedProvider(raw);
  }

  function onBaseUrlChange(raw: string) {
    void patchConfig((prev) => ({ ...prev, ai: { ...prev.ai, base_url: raw.trim() } }));
  }

  function onModelText(raw: string) {
    setUserSelected(true);
    void patchConfig((prev) => ({ ...prev, ai: { ...prev.ai, model: raw.trim() } }));
  }

  function onSelectModel(value: string) {
    if (value === "__custom__") {
      setCustomMode(true);
      return;
    }
    setCustomMode(false);
    setUserSelected(true);
    void patchConfig((prev) => ({ ...prev, ai: { ...prev.ai, model: value } }));
  }

  function onChatModelDisclosure() {
    if (chatModelOpen()) {
      setChatModelOpen(false);
      void aiConnectionStore.selectChatModel(null);
      return;
    }
    setChatModelOpen(true);
    void aiConnectionStore.selectChatModel(cfg().model || modelOptionList()[0] || "");
  }

  // Consent is recorded host-side: the command resolves the host itself and
  // persists it, so the stored value is exactly what the guard checks.
  async function onConsent() {
    try {
      setEndpoint(await aiRewriteStore.consentHost());
      await configStore.load();
      await refreshModels();
    } catch {
      showToast("Could not record the choice", "error");
    }
  }

  async function onSetKey() {
    const key = keyInput();
    if (!key || keyBusy()) return;
    setKeyBusy(true);
    try {
      const state = await aiRewriteStore.setApiKey(cfg().provider, key);
      setKeyState(state);
      setKeyInput("");
      await refreshModels();
      if (state.memory_only) {
        showToast("Writ could not save your key, so you will enter it again next time", "info");
      }
    } catch {
      showToast("Could not save the API key", "error");
    } finally {
      setKeyBusy(false);
    }
  }

  async function onClearKey() {
    if (keyBusy()) return;
    setKeyBusy(true);
    try {
      const state = await aiRewriteStore.clearApiKey(cfg().provider);
      setKeyState(state);
    } catch {
      showToast("Could not clear the API key", "error");
    } finally {
      setKeyBusy(false);
    }
  }

  async function onConnect() {
    if (connecting()) {
      void aiConnectionStore.cancelOpenrouter();
      return;
    }
    setConnecting(true);
    try {
      setKeyState(await aiConnectionStore.connectOpenrouter());
      await refreshModels();
    } catch (err) {
      showToast(typeof err === "string" ? err : "The key exchange failed.", "error");
    } finally {
      setConnecting(false);
    }
  }

  return (
    <div class="settings-app-detail" data-app-detail="ai-connection">
      <SettingsRow
        id="ai.provider"
        labelFor="setting-ai-provider"
      >
        <span class="settings-provider-control">
          <select
            id="setting-ai-provider"
            class="settings-select"
            data-setting="ai_provider"
            value={cfg().provider}
            onChange={(e) => onProviderChange(e.currentTarget.value)}
          >
            <For each={aiProvidersStore.grouped()}>
              {(group) => (
                <optgroup label={group.label}>
                  <For each={group.providers}>
                    {/* The table loads after this mounts, so the option carries the
                        choice: `value` on the select alone leaves the first showing. */}
                    {(row) => (
                      <option value={row.id} selected={row.id === cfg().provider}>
                        {row.label}
                      </option>
                    )}
                  </For>
                </optgroup>
              )}
            </For>
          </select>
          <Show when={localRunning() !== null}>
            <span class="settings-ai-pill" data-running={localRunning() ? "yes" : "no"}>
              {localRunning() ? "Running" : "Not running"}
            </span>
          </Show>
        </span>
      </SettingsRow>

      <Show when={localRunning() === false}>
        <div class="settings-ai-note">
          {LOCAL_HELP[cfg().provider]}
          <Show when={cfg().provider === "ollama"}>
            {" "}
            <button
              type="button"
              class="settings-ai-link"
              data-action="get-ollama"
              onClick={() => void linkStore.openExternal(OLLAMA_DOWNLOAD_URL)}
            >
              Get Ollama
            </button>
          </Show>
        </div>
      </Show>

      <Show when={isCustom()}>
        <SettingsRow
          id="ai.base_url"
          labelFor="setting-ai-base-url"
        >
          <input
            id="setting-ai-base-url"
            type="text"
            class="settings-input"
            data-setting="ai_base_url"
            spellcheck={false}
            autocomplete="off"
            placeholder="https://api.example.com/v1"
            value={cfg().base_url}
            onChange={(e) => onBaseUrlChange(e.currentTarget.value)}
          />
        </SettingsRow>
      </Show>

      <Show when={hostedUnconsented()}>
        <div class="settings-ai-consent" role="note">
          <p class="settings-ai-consent-text">
            The files you attach and the text you rewrite are sent to {endpointHost()} with your API
            key. Writ also sends the key on its own to check the host is reachable. The check sends
            no text.
          </p>
          <Button data-action="ai-consent" onClick={() => void onConsent()}>
            Allow
          </Button>
        </div>
      </Show>

      <Show when={showLocalLine()}>
        <div class="settings-ai-note" data-note="local-endpoint">
          Requests go to {endpointHost()} on this machine. Nothing leaves it.
        </div>
      </Show>

      <Show when={needsKey()}>
        <SettingsRow
          id="ai.api_key"
          labelAside={
            <Show when={keyPageUrl()}>
              <button
                type="button"
                class="settings-ai-link"
                data-action="ai-key-page"
                onClick={() => void linkStore.openExternal(keyPageUrl()!)}
              >
                Get a key
              </button>
            </Show>
          }
        >
          <span class="settings-inbox-controls">
            <input
              type="password"
              class="settings-input"
              data-setting="ai_api_key"
              spellcheck={false}
              autocomplete="off"
              placeholder={keyState()?.is_set ? "Key set" : "Not set"}
              value={keyInput()}
              onInput={(e) => setKeyInput(e.currentTarget.value)}
            />
            <Button
              data-action="ai-set-key"
              disabled={keyBusy() || keyInput().length === 0}
              onClick={() => void onSetKey()}
            >
              Save
            </Button>
            <Show when={keyState()?.is_set}>
              <Button
                data-action="ai-clear-key"
                disabled={keyBusy()}
                onClick={() => void onClearKey()}
              >
                Clear
              </Button>
            </Show>
            <Show when={provider()?.supports_connect}>
              <Button data-action="ai-connect" onClick={() => void onConnect()}>
                {connecting() ? "Cancel" : "Connect"}
              </Button>
            </Show>
          </span>
        </SettingsRow>
      </Show>
      <Show when={keyState()?.is_set && keyState()?.memory_only}>
        <div class="settings-ai-note">
          Your key is not saved. You will enter it again next time you open Writ.
        </div>
      </Show>

      <SettingsRow
        id="ai.model"
        labelFor="setting-ai-model"
      >
        <div class="settings-model-picker">
          <select
            id="setting-ai-model"
            class="settings-select"
            data-setting="ai_model"
            value={modelSelectValue()}
            onChange={(e) => onSelectModel(e.currentTarget.value)}
          >
            <Show when={choosingModel()}>
              <option value="" disabled>
                No model
              </option>
            </Show>
            <For each={modelOptionList()}>
              {(id) => <option value={id}>{modelOptionLabel(id)}</option>}
            </For>
            <option value="__custom__">Custom…</option>
          </select>
          <Show when={showModelInput()}>
            <input
              type="text"
              class="settings-input"
              data-setting="ai_model_custom"
              spellcheck={false}
              autocomplete="off"
              placeholder="Model id"
              value={cfg().model}
              onChange={(e) => onModelText(e.currentTarget.value)}
            />
          </Show>
        </div>
      </SettingsRow>
      <Show when={keyState()?.is_set && !cfg().model.trim()}>
        <div class="settings-ai-note" data-note="choose-a-model">
          {CHOOSE_A_MODEL}
        </div>
      </Show>

      <SettingsRow
        id="ai.connection"
      >
        <span class="settings-ai-connection">
          <span
            class="settings-ai-connection-status"
            data-tone={connection().tone}
            aria-live="polite"
          >
            {connection().text}
          </span>
          <Button
            data-action="ai-recheck"
            disabled={aiConnectionStore.checking()}
            onClick={() => void aiConnectionStore.check()}
          >
            {aiConnectionStore.checking() ? "Checking…" : "Check"}
          </Button>
        </span>
      </SettingsRow>

      <Show when={props.withChatModel && configStore.isAppOn("chat")}>
        <SettingsRow
          id="ai.chat.model"
        >
          <span class="settings-model-picker">
            <ToggleSwitch
              setting="ai_chat_model_disclosure"
              label={settingTitle("ai.chat.model")}
              checked={chatModelOpen()}
              onChange={onChatModelDisclosure}
            />
            <Show when={chatModelOpen()}>
              <select
                class="settings-select"
                data-setting="ai_chat_model"
                aria-label="Chat model"
                value={cfg().chat.model}
                onChange={(e) => void aiConnectionStore.selectChatModel(e.currentTarget.value)}
              >
                {/* The catalog arrives after this mounts and `chat.model` does not
                    change with it, so the option carries the choice. */}
                <For each={modelOptionList()}>
                  {(id) => (
                    <option value={id} selected={id === cfg().chat.model}>
                      {modelOptionLabel(id)}
                    </option>
                  )}
                </For>
              </select>
            </Show>
          </span>
        </SettingsRow>
      </Show>
    </div>
  );
}
