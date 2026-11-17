import { describe, it, expect, vi, beforeEach } from "vitest";
import type { AiEndpointState } from "../../services/tauri";
import type { DictationCapabilities, DictationEndpointState } from "../../types/dictation";

const h = vi.hoisted(() => ({
  dictationCapabilities: vi.fn(),
  dictationLanguages: vi.fn(),
  dictationEndpointState: vi.fn(),
  dictationConsentHost: vi.fn(),
  dictationSetKey: vi.fn(),
  dictationClearKey: vi.fn(),
  logFailure: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  dictationCapabilities: h.dictationCapabilities,
  dictationLanguages: h.dictationLanguages,
  dictationEndpointState: h.dictationEndpointState,
  dictationConsentHost: h.dictationConsentHost,
  dictationSetKey: h.dictationSetKey,
  dictationClearKey: h.dictationClearKey,
}));
vi.mock("../../lib/log", () => ({ logFailure: h.logFailure }));

import {
  DICTATION_PROVIDERS,
  effectiveEngine,
  isAiConnectionReady,
  isLanguageUnsupported,
  languageChoices,
  modelForProvider,
  presetModel,
} from "../../stores/global/dictation-settings";

function capabilities(overrides: Partial<DictationCapabilities> = {}): DictationCapabilities {
  return {
    on_device: true,
    on_device_languages: ["en_US", "ar-SA", "fr_FR"],
    system_language: "en_US",
    mic_settings: true,
    effective_engine: "on_device",
    ...overrides,
  };
}

function endpoint(overrides: Partial<DictationEndpointState> = {}): DictationEndpointState {
  return {
    host: "api.groq.com",
    is_hosted: true,
    is_allowed: true,
    is_consented: false,
    needs_key: true,
    key_shared_with_ai: false,
    key: { is_set: false, memory_only: false },
    key_page: "https://console.groq.com/keys",
    ...overrides,
  };
}

function aiState(overrides: Partial<AiEndpointState> = {}): AiEndpointState {
  return {
    host: "api.deepseek.com",
    host_port: "api.deepseek.com",
    is_hosted: true,
    is_private_network: false,
    is_allowed: true,
    is_consented: true,
    provider: "deepseek",
    key_state: { is_set: true, memory_only: false },
    is_on_device: false,
    ...overrides,
  };
}

const LANGUAGES = [
  { code: "ar", name: "Arabic" },
  { code: "en", name: "English" },
  { code: "fr", name: "French" },
  { code: "sw", name: "Swahili" },
];

describe("dictation presets", () => {
  it("fills the model a preset names and leaves Custom to the person", () => {
    expect(DICTATION_PROVIDERS.map((p) => p.id)).toEqual(["openai", "groq", "custom"]);
    expect(presetModel("openai")).toBe("gpt-transcribe");
    expect(presetModel("groq")).toBe("whisper-large-v3-turbo");
    expect(presetModel("custom")).toBeNull();
  });

  it("replaces an empty model or the previous preset's on a provider change", () => {
    expect(modelForProvider("openai", "groq", "gpt-transcribe")).toBe("whisper-large-v3-turbo");
    expect(modelForProvider("custom", "openai", "")).toBe("gpt-transcribe");
    expect(modelForProvider("custom", "openai", "  ")).toBe("gpt-transcribe");
  });

  it("keeps a model the person typed", () => {
    expect(modelForProvider("openai", "groq", "my-model")).toBe("my-model");
    expect(modelForProvider("groq", "custom", "whisper-large-v3-turbo")).toBe(
      "whisper-large-v3-turbo",
    );
  });
});

describe("effectiveEngine", () => {
  it("prefers the configured engine, then what Rust resolved, then the API", () => {
    expect(effectiveEngine("api", capabilities())).toBe("api");
    expect(effectiveEngine(undefined, capabilities())).toBe("on_device");
    expect(effectiveEngine(undefined, capabilities({ effective_engine: "api" }))).toBe("api");
    expect(effectiveEngine(undefined, null)).toBe("api");
  });
});

describe("languageChoices", () => {
  it("lists every language for the API engine", () => {
    expect(languageChoices(LANGUAGES, "api", capabilities(), "")).toEqual(LANGUAGES);
  });

  it("narrows to the locales this Mac transcribes, by their language part", () => {
    expect(languageChoices(LANGUAGES, "on_device", capabilities(), "").map((l) => l.code)).toEqual(
      ["ar", "en", "fr"],
    );
  });

  it("keeps a configured language this Mac does not transcribe", () => {
    expect(
      languageChoices(LANGUAGES, "on_device", capabilities(), "sw").map((l) => l.code),
    ).toEqual(["ar", "en", "fr", "sw"]);
  });

  it("lists everything until the capabilities arrive", () => {
    expect(languageChoices(LANGUAGES, "on_device", null, "")).toEqual(LANGUAGES);
  });
});

describe("isLanguageUnsupported", () => {
  it("flags only a named language the on-device engine cannot use", () => {
    expect(isLanguageUnsupported("on_device", capabilities(), "sw")).toBe(true);
    expect(isLanguageUnsupported("on_device", capabilities(), "AR")).toBe(false);
    expect(isLanguageUnsupported("on_device", capabilities(), "")).toBe(false);
    expect(isLanguageUnsupported("api", capabilities(), "sw")).toBe(false);
    expect(isLanguageUnsupported("on_device", null, "sw")).toBe(false);
  });
});

describe("isAiConnectionReady", () => {
  it("is ready for Apple's model without a key or a model", () => {
    expect(isAiConnectionReady(aiState({ is_on_device: true, is_allowed: false }), "")).toBe(true);
  });

  it("needs a permitted, consented endpoint with its key and a model", () => {
    expect(isAiConnectionReady(aiState(), "deepseek-chat")).toBe(true);
    expect(isAiConnectionReady(null, "m")).toBe(false);
    expect(isAiConnectionReady(aiState({ is_allowed: false }), "m")).toBe(false);
    expect(isAiConnectionReady(aiState({ host: null }), "m")).toBe(false);
    expect(isAiConnectionReady(aiState({ is_consented: false }), "m")).toBe(false);
    expect(
      isAiConnectionReady(aiState({ key_state: { is_set: false, memory_only: false } }), "m"),
    ).toBe(false);
    expect(isAiConnectionReady(aiState(), " ")).toBe(false);
  });

  it("takes a local server without consent or a key", () => {
    const local = aiState({
      is_hosted: false,
      is_consented: false,
      key_state: { is_set: false, memory_only: false },
    });
    expect(isAiConnectionReady(local, "llama3")).toBe(true);
  });

  it("takes a private-network address with consent and no key", () => {
    const lan = aiState({
      host: "192.168.1.20",
      is_private_network: true,
      key_state: { is_set: false, memory_only: false },
    });
    expect(isAiConnectionReady(lan, "llama3")).toBe(true);
    expect(isAiConnectionReady({ ...lan, is_consented: false }, "llama3")).toBe(false);
  });
});

describe("dictationSettingsStore", () => {
  async function freshStore() {
    vi.resetModules();
    return (await import("../../stores/global/dictation-settings")).dictationSettingsStore;
  }

  beforeEach(() => {
    for (const fn of Object.values(h)) fn.mockReset();
  });

  it("reads the capabilities and languages once", async () => {
    h.dictationCapabilities.mockResolvedValue(capabilities());
    h.dictationLanguages.mockResolvedValue(LANGUAGES);
    const store = await freshStore();
    await Promise.all([store.load(), store.load()]);
    await store.load();
    expect(h.dictationCapabilities).toHaveBeenCalledTimes(1);
    expect(h.dictationLanguages).toHaveBeenCalledTimes(1);
    expect(store.capabilities()?.on_device).toBe(true);
    expect(store.languages()).toEqual(LANGUAGES);
  });

  it("logs a failed read and tries again on the next load", async () => {
    h.dictationCapabilities.mockRejectedValueOnce("no command").mockResolvedValue(capabilities());
    h.dictationLanguages.mockResolvedValue(LANGUAGES);
    const store = await freshStore();
    await store.load();
    expect(store.capabilities()).toBeNull();
    expect(h.logFailure).toHaveBeenCalledWith("the dictation capabilities could not be read");
    await store.load();
    expect(store.capabilities()?.effective_engine).toBe("on_device");
  });

  it("keeps the latest endpoint answer when two reads overlap", async () => {
    let answerFirst!: (state: DictationEndpointState) => void;
    h.dictationEndpointState
      .mockReturnValueOnce(new Promise((resolve) => (answerFirst = resolve)))
      .mockResolvedValueOnce(endpoint({ host: "api.openai.com" }));
    const store = await freshStore();
    const first = store.refreshEndpoint();
    await store.refreshEndpoint();
    answerFirst(endpoint({ host: "stale.example.com" }));
    await first;
    expect(store.endpoint()?.host).toBe("api.openai.com");
  });

  it("clears the endpoint when it cannot be read", async () => {
    h.dictationEndpointState.mockResolvedValueOnce(endpoint()).mockRejectedValueOnce("gone");
    const store = await freshStore();
    await store.refreshEndpoint();
    expect(store.endpoint()).not.toBeNull();
    await store.refreshEndpoint();
    expect(store.endpoint()).toBeNull();
    expect(h.logFailure).toHaveBeenCalledWith("the dictation endpoint state could not be read");
  });

  it("takes the consented state Rust answers", async () => {
    h.dictationConsentHost.mockResolvedValue(endpoint({ is_consented: true }));
    const store = await freshStore();
    await store.consentHost();
    expect(store.endpoint()?.is_consented).toBe(true);
  });

  it("passes a refused consent to the caller", async () => {
    h.dictationConsentHost.mockRejectedValue("write failed");
    const store = await freshStore();
    await expect(store.consentHost()).rejects.toBe("write failed");
  });

  it("puts a saved or cleared key into the endpoint state", async () => {
    h.dictationEndpointState.mockResolvedValue(endpoint());
    h.dictationSetKey.mockResolvedValue({ is_set: true, memory_only: true });
    h.dictationClearKey.mockResolvedValue({ is_set: false, memory_only: false });
    const store = await freshStore();
    await store.refreshEndpoint();
    expect(await store.setKey("gsk_123")).toEqual({ is_set: true, memory_only: true });
    expect(h.dictationSetKey).toHaveBeenCalledWith("gsk_123");
    expect(store.endpoint()?.key).toEqual({ is_set: true, memory_only: true });
    await store.clearKey();
    expect(store.endpoint()?.key.is_set).toBe(false);
  });

  it("passes a refused key to the caller", async () => {
    h.dictationSetKey.mockRejectedValue("keychain locked");
    const store = await freshStore();
    await expect(store.setKey("k")).rejects.toBe("keychain locked");
  });
});
