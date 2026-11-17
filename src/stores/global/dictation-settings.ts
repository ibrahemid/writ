import { createRoot, createSignal } from "solid-js";
import {
  dictationCapabilities,
  dictationClearKey,
  dictationConsentHost,
  dictationEndpointState,
  dictationLanguages,
  dictationSetKey,
  type AiEndpointState,
  type AiKeyState,
} from "../../services/tauri";
import { logFailure } from "../../lib/log";
import type { DictationEngine, DictationProvider } from "../../types/config";
import type {
  DictationCapabilities,
  DictationEndpointState,
  DictationLanguage,
} from "../../types/dictation";

export interface DictationProviderOption {
  id: DictationProvider;
  label: string;
  model: string | null;
}

/** Mirrors `writ_core::dictation::PRESETS`; Rust reads the base URL and key page. */
export const DICTATION_PROVIDERS: readonly DictationProviderOption[] = [
  { id: "openai", label: "OpenAI", model: "gpt-transcribe" },
  { id: "groq", label: "Groq", model: "whisper-large-v3-turbo" },
  { id: "custom", label: "Custom", model: null },
];

export function presetModel(provider: DictationProvider): string | null {
  return DICTATION_PROVIDERS.find((option) => option.id === provider)?.model ?? null;
}

export function modelForProvider(
  previous: DictationProvider,
  next: DictationProvider,
  model: string,
): string {
  const typed = model.trim();
  const nextModel = presetModel(next);
  if (nextModel === null) return model;
  if (typed === "" || typed === presetModel(previous)) return nextModel;
  return model;
}

export function effectiveEngine(
  configured: DictationEngine | undefined,
  capabilities: DictationCapabilities | null,
): DictationEngine {
  return configured ?? capabilities?.effective_engine ?? "api";
}

function languagePart(locale: string): string {
  return locale.split(/[-_]/)[0].toLowerCase();
}

export function languageChoices(
  languages: readonly DictationLanguage[],
  engine: DictationEngine,
  capabilities: DictationCapabilities | null,
  configured: string,
): DictationLanguage[] {
  if (engine !== "on_device" || capabilities === null) return [...languages];
  const supported = new Set(capabilities.on_device_languages.map(languagePart));
  return languages.filter(
    (language) => supported.has(language.code.toLowerCase()) || language.code === configured,
  );
}

export function isLanguageUnsupported(
  engine: DictationEngine,
  capabilities: DictationCapabilities | null,
  configured: string,
): boolean {
  if (engine !== "on_device" || capabilities === null || configured === "") return false;
  return !capabilities.on_device_languages.some(
    (locale) => languagePart(locale) === configured.toLowerCase(),
  );
}

export function isAiConnectionReady(state: AiEndpointState | null, model: string): boolean {
  if (state === null) return false;
  if (state.is_on_device) return true;
  if (!state.is_allowed || !state.host) return false;
  if (state.is_hosted && !state.is_private_network && !state.key_state.is_set) return false;
  if (state.is_hosted && !state.is_consented) return false;
  return model.trim() !== "";
}

// Singleton state — Writ is single-window
function createDictationSettingsStore() {
  const [capabilities, setCapabilities] = createSignal<DictationCapabilities | null>(null);
  const [languages, setLanguages] = createSignal<DictationLanguage[]>([]);
  const [endpoint, setEndpoint] = createSignal<DictationEndpointState | null>(null);
  let staticLoad: Promise<void> | null = null;
  let endpointAsk = 0;

  async function readCapabilities(): Promise<void> {
    try {
      setCapabilities(await dictationCapabilities());
    } catch {
      logFailure("the dictation capabilities could not be read");
    }
  }

  async function readLanguages(): Promise<void> {
    try {
      setLanguages(await dictationLanguages());
    } catch {
      logFailure("the dictation languages could not be read");
    }
  }

  function load(): Promise<void> {
    if (capabilities() !== null && languages().length > 0) return Promise.resolve();
    staticLoad ??= Promise.all([readCapabilities(), readLanguages()]).then(() => {
      staticLoad = null;
    });
    return staticLoad;
  }

  async function refreshEndpoint(): Promise<void> {
    const ask = ++endpointAsk;
    try {
      const state = await dictationEndpointState();
      if (ask === endpointAsk) setEndpoint(state);
    } catch {
      if (ask === endpointAsk) setEndpoint(null);
      logFailure("the dictation endpoint state could not be read");
    }
  }

  async function consentHost(): Promise<void> {
    endpointAsk++;
    setEndpoint(await dictationConsentHost());
  }

  function withKey(key: AiKeyState): void {
    const state = endpoint();
    if (state !== null) setEndpoint({ ...state, key });
  }

  async function setKey(key: string): Promise<AiKeyState> {
    const state = await dictationSetKey(key);
    withKey(state);
    return state;
  }

  async function clearKey(): Promise<AiKeyState> {
    const state = await dictationClearKey();
    withKey(state);
    return state;
  }

  return {
    capabilities,
    languages,
    endpoint,
    load,
    refreshEndpoint,
    consentHost,
    setKey,
    clearKey,
  };
}

export type DictationSettingsStore = ReturnType<typeof createDictationSettingsStore>;
export const dictationSettingsStore = createRoot(createDictationSettingsStore);
