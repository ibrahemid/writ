import { createEffect, createSignal, For, onMount, Show } from "solid-js";
import { configStore } from "../../../stores/global/config";
import { showToast } from "../../../stores/global/notifications";
import { aiRewriteStore, type AiEndpointState } from "../../../stores/global/ai-rewrite";
import { linkStore } from "../../../stores/global/link";
import {
  DICTATION_PROVIDERS,
  dictationSettingsStore,
  effectiveEngine,
  isAiConnectionReady,
  isLanguageUnsupported,
  languageChoices,
  modelForProvider,
  presetModel,
} from "../../../stores/global/dictation-settings";
import type { DictationConfig, DictationEngine, DictationProvider } from "../../../types/config";
import Button from "../../Button/Button";
import { patchConfig } from "../controls";
import { SettingsRow, SwitchRow } from "../SettingsRow";
import "./DictationRows.css";

const ENGINES: readonly { id: DictationEngine; label: string }[] = [
  { id: "api", label: "API" },
  { id: "on_device", label: "On this Mac" },
];

function patchDictation(fields: Partial<DictationConfig>): Promise<void> {
  return patchConfig((prev) => ({ ...prev, dictation: { ...prev.dictation, ...fields } }));
}

export default function DictationRows() {
  const store = dictationSettingsStore;
  const cfg = () => configStore.config().dictation;
  const ai = () => configStore.config().ai;
  const engine = () => effectiveEngine(cfg().engine, store.capabilities());
  const isApi = () => engine() === "api";
  const endpoint = () => store.endpoint();

  const [keyInput, setKeyInput] = createSignal("");
  const [keyBusy, setKeyBusy] = createSignal(false);
  const [aiEndpoint, setAiEndpoint] = createSignal<AiEndpointState | null>(null);

  onMount(() => {
    void store.load();
  });

  createEffect(() => {
    void cfg().provider;
    void cfg().base_url;
    void cfg().consented_hosts.join("\n");
    void ai().provider;
    void ai().base_url;
    void store.refreshEndpoint();
  });

  createEffect(() => {
    void ai().provider;
    void ai().base_url;
    void ai().consented_hosts.join("\n");
    void aiRewriteStore
      .endpointState()
      .then(setAiEndpoint)
      .catch(() => setAiEndpoint(null));
  });

  const showEngine = () => store.capabilities()?.on_device === true || cfg().engine === "on_device";
  const engineUnsupported = () =>
    engine() === "on_device" && store.capabilities()?.on_device === false;

  const keyShared = () => endpoint()?.key_shared_with_ai === true;
  const keySet = () => endpoint()?.key.is_set === true;
  const showKey = () => {
    const state = endpoint();
    return isApi() && state !== null && (state.needs_key || state.key.is_set);
  };
  const consentNeeded = () => {
    const state = endpoint();
    return Boolean(isApi() && state?.is_allowed && state.is_hosted && !state.is_consented);
  };

  const languages = () =>
    languageChoices(store.languages(), engine(), store.capabilities(), cfg().language);
  const languageUnsupported = () =>
    isLanguageUnsupported(engine(), store.capabilities(), cfg().language);

  const aiReady = () => isAiConnectionReady(aiEndpoint(), ai().model);
  const aiHost = () => {
    const state = aiEndpoint();
    return state?.host_port ?? state?.host ?? "";
  };
  const cleanUpDetail = () => {
    if (!aiReady()) return undefined;
    if (aiEndpoint()?.is_on_device) return "Runs on this Mac.";
    return aiHost() ? `Sends the text to ${aiHost()}.` : undefined;
  };

  function onProviderChange(next: DictationProvider) {
    const previous = cfg().provider;
    if (next === previous) return;
    void patchDictation({ provider: next, model: modelForProvider(previous, next, cfg().model) });
  }

  async function onConsent() {
    try {
      await store.consentHost();
      await configStore.load();
    } catch {
      showToast("Could not record the choice", "error");
    }
  }

  async function onSetKey() {
    const key = keyInput();
    if (!key || keyBusy()) return;
    setKeyBusy(true);
    try {
      const state = await store.setKey(key);
      setKeyInput("");
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
      await store.clearKey();
    } catch {
      showToast("Could not clear the API key", "error");
    } finally {
      setKeyBusy(false);
    }
  }

  return (
    <div class="settings-app-detail" data-app-detail="dictation">
      <Show when={showEngine()}>
        <SettingsRow
          id="dictation.engine"
          labelFor="setting-dictation-engine"
          caution={engineUnsupported() ? "On-device dictation needs macOS 26 or later." : undefined}
        >
          <select
            id="setting-dictation-engine"
            class="settings-select"
            data-setting="dictation_engine"
            value={engine()}
            onChange={(e) => void patchDictation({ engine: e.currentTarget.value as DictationEngine })}
          >
            <For each={ENGINES}>
              {(option) => (
                <option value={option.id} selected={option.id === engine()}>
                  {option.label}
                </option>
              )}
            </For>
          </select>
        </SettingsRow>
      </Show>

      <Show when={isApi()}>
        <SettingsRow id="dictation.provider" labelFor="setting-dictation-provider">
          <select
            id="setting-dictation-provider"
            class="settings-select"
            data-setting="dictation_provider"
            value={cfg().provider}
            onChange={(e) => onProviderChange(e.currentTarget.value as DictationProvider)}
          >
            <For each={DICTATION_PROVIDERS}>
              {(option) => (
                <option value={option.id} selected={option.id === cfg().provider}>
                  {option.label}
                </option>
              )}
            </For>
          </select>
        </SettingsRow>

        <Show when={cfg().provider === "custom"}>
          <SettingsRow id="dictation.base_url" labelFor="setting-dictation-base-url">
            <input
              id="setting-dictation-base-url"
              type="text"
              class="settings-input"
              data-setting="dictation_base_url"
              spellcheck={false}
              autocomplete="off"
              placeholder="https://api.example.com/v1"
              value={cfg().base_url}
              onChange={(e) => void patchDictation({ base_url: e.currentTarget.value.trim() })}
            />
          </SettingsRow>
        </Show>

        <Show when={showKey()}>
          <SettingsRow
            id="dictation.api_key"
            labelFor={keyShared() ? undefined : "setting-dictation-api-key"}
            caution={keyShared() && !keySet() ? "Add it on the Chat or Rewrite page." : undefined}
            labelAside={
              <Show when={!keyShared() && endpoint()?.key_page}>
                {(url) => (
                  <button
                    type="button"
                    class="settings-ai-link"
                    data-action="dictation-key-page"
                    onClick={() => void linkStore.openExternal(url())}
                  >
                    Get a key
                  </button>
                )}
              </Show>
            }
          >
            <Show
              when={!keyShared()}
              fallback={
                <span class="settings-dictation-key-shared" data-note="key-shared">
                  Uses the AI connection's key.
                </span>
              }
            >
              <span class="settings-inbox-controls">
                <input
                  id="setting-dictation-api-key"
                  type="password"
                  class="settings-input"
                  data-setting="dictation_api_key"
                  spellcheck={false}
                  autocomplete="off"
                  placeholder={keySet() ? "Key set" : "Not set"}
                  value={keyInput()}
                  onInput={(e) => setKeyInput(e.currentTarget.value)}
                />
                <Button
                  data-action="dictation-set-key"
                  disabled={keyBusy() || keyInput().length === 0}
                  onClick={() => void onSetKey()}
                >
                  Save
                </Button>
                <Show when={keySet()}>
                  <Button
                    data-action="dictation-clear-key"
                    disabled={keyBusy()}
                    onClick={() => void onClearKey()}
                  >
                    Clear
                  </Button>
                </Show>
              </span>
            </Show>
          </SettingsRow>
        </Show>
        <Show when={!keyShared() && keySet() && endpoint()?.key.memory_only}>
          <div class="settings-ai-note">
            Your key is not saved. You will enter it again next time you open Writ.
          </div>
        </Show>

        <Show when={consentNeeded()}>
          <div class="settings-ai-consent" role="note" data-consent="dictation">
            <p class="settings-ai-consent-text">
              What you dictate is sent to {endpoint()?.host}.
            </p>
            <Button data-action="dictation-consent" onClick={() => void onConsent()}>
              Allow
            </Button>
          </div>
        </Show>

        <SettingsRow id="dictation.model" labelFor="setting-dictation-model">
          <input
            id="setting-dictation-model"
            type="text"
            class="settings-input"
            data-setting="dictation_model"
            spellcheck={false}
            autocomplete="off"
            placeholder={presetModel(cfg().provider) ?? "Model id"}
            value={cfg().model}
            onChange={(e) => void patchDictation({ model: e.currentTarget.value.trim() })}
          />
        </SettingsRow>
      </Show>

      <SettingsRow
        id="dictation.language"
        labelFor="setting-dictation-language"
        caution={
          languageUnsupported()
            ? "Not available on this Mac. Choose another language or the API engine."
            : undefined
        }
        detail={
          engine() === "on_device" && cfg().language === ""
            ? "On this Mac, Automatic uses the system language."
            : undefined
        }
      >
        <select
          id="setting-dictation-language"
          class="settings-select"
          data-setting="dictation_language"
          value={cfg().language}
          onChange={(e) => void patchDictation({ language: e.currentTarget.value })}
        >
          <option value="" selected={cfg().language === ""}>
            Automatic
          </option>
          {/* The list arrives after this mounts, so the option carries the choice. */}
          <For each={languages()}>
            {(language) => (
              <option value={language.code} selected={language.code === cfg().language}>
                {language.name}
              </option>
            )}
          </For>
        </select>
      </SettingsRow>

      <SwitchRow
        id="dictation.clean_up"
        setting="dictation_clean_up"
        checked={cfg().clean_up}
        disabled={!cfg().clean_up && !aiReady()}
        detail={cleanUpDetail()}
        caution={aiReady() ? undefined : "Set up the AI connection on the Chat or Rewrite page first."}
        onChange={() => void patchDictation({ clean_up: !cfg().clean_up })}
      />
    </div>
  );
}
