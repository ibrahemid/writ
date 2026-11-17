import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";
import type { AiEndpointState } from "../../../../services/tauri";
import type { DictationConfig } from "../../../../types/config";
import type {
  DictationCapabilities,
  DictationEndpointState,
  DictationLanguage,
} from "../../../../types/dictation";

const h = await vi.hoisted(async () => {
  const { createSignal } = await import("solid-js");
  const [capabilities, setCapabilities] = createSignal<DictationCapabilities | null>(null);
  const [languages, setLanguages] = createSignal<DictationLanguage[]>([]);
  const [endpoint, setEndpoint] = createSignal<DictationEndpointState | null>(null);
  return {
    updateConfig: vi.fn(async () => {}),
    getConfig: vi.fn(),
    aiEndpointState: vi.fn(),
    openExternal: vi.fn(async () => {}),
    showToast: vi.fn(),
    setCapabilities,
    setLanguages,
    setEndpoint,
    store: {
      capabilities,
      languages,
      endpoint,
      load: vi.fn(async () => {}),
      refreshEndpoint: vi.fn(async () => {}),
      consentHost: vi.fn(async () => {}),
      setKey: vi.fn(),
      clearKey: vi.fn(),
    },
  };
});

vi.mock("../../../../services/tauri", () => ({
  updateConfig: h.updateConfig,
  getConfig: h.getConfig,
}));
vi.mock("../../../../stores/global/notifications", () => ({
  showToast: h.showToast,
  dismissToast: vi.fn(),
}));
vi.mock("../../../../stores/global/ai-rewrite", () => ({
  aiRewriteStore: { endpointState: h.aiEndpointState },
}));
vi.mock("../../../../stores/global/link", () => ({
  linkStore: { openExternal: h.openExternal },
}));
vi.mock("../../../../stores/global/dictation-settings", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../stores/global/dictation-settings")>()),
  dictationSettingsStore: h.store,
}));

import { configStore } from "../../../../stores/global/config";
import { SearchContext } from "../../SettingsRow";
import DictationRows from "../DictationRows";

const SEARCH = { rowVisible: () => true, sectionVisible: () => true, highlighted: () => false };

function capabilities(overrides: Partial<DictationCapabilities> = {}): DictationCapabilities {
  return {
    on_device: false,
    on_device_languages: [],
    system_language: "en_US",
    mic_settings: true,
    effective_engine: "api",
    ...overrides,
  };
}

function endpoint(overrides: Partial<DictationEndpointState> = {}): DictationEndpointState {
  return {
    host: "api.openai.com",
    is_hosted: true,
    is_allowed: true,
    is_consented: true,
    needs_key: true,
    key_shared_with_ai: false,
    key: { is_set: false, memory_only: false },
    key_page: null,
    ...overrides,
  };
}

function aiReady(overrides: Partial<AiEndpointState> = {}): AiEndpointState {
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

const LANGUAGES: DictationLanguage[] = [
  { code: "ar", name: "Arabic" },
  { code: "en", name: "English" },
  { code: "sw", name: "Swahili" },
];

async function setDictation(fields: Partial<DictationConfig>, aiModel = "deepseek-chat") {
  const prev = configStore.config();
  const dictation: DictationConfig = {
    enabled: true,
    provider: "openai",
    base_url: "",
    model: "gpt-transcribe",
    language: "",
    clean_up: false,
    consented_hosts: [],
    ...fields,
  };
  await configStore.save({ ...prev, ai: { ...prev.ai, model: aiModel }, dictation });
  h.updateConfig.mockClear();
}

function renderRows() {
  return render(() => (
    <SearchContext.Provider value={SEARCH}>
      <DictationRows />
    </SearchContext.Provider>
  ));
}

function row(container: HTMLElement, id: string): HTMLElement | null {
  return container.querySelector<HTMLElement>(`[data-setting-id='${id}']`);
}

function savedDictation(): DictationConfig {
  const calls = h.updateConfig.mock.calls as unknown as [{ dictation: DictationConfig }][];
  return calls[calls.length - 1][0].dictation;
}

beforeEach(async () => {
  h.setCapabilities(capabilities());
  h.setLanguages(LANGUAGES);
  h.setEndpoint(endpoint());
  h.aiEndpointState.mockReset().mockResolvedValue(aiReady());
  await setDictation({});
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("DictationRows", () => {
  it("reads the machine's capabilities and where audio goes when it mounts", async () => {
    renderRows();
    await waitFor(() => expect(h.store.load).toHaveBeenCalledTimes(1));
    expect(h.store.refreshEndpoint).toHaveBeenCalled();
  });

  it("draws the engine row only where this Mac transcribes", async () => {
    const { container } = renderRows();
    expect(row(container, "dictation.engine")).toBeNull();
    h.setCapabilities(capabilities({ on_device: true, effective_engine: "on_device" }));
    await waitFor(() => expect(row(container, "dictation.engine")).not.toBeNull());
    const select = container.querySelector<HTMLSelectElement>("[data-setting='dictation_engine']")!;
    expect(select.value).toBe("on_device");
    expect(row(container, "dictation.provider")).toBeNull();
    expect(row(container, "dictation.model")).toBeNull();
  });

  it("writes the engine key when one is chosen", async () => {
    h.setCapabilities(capabilities({ on_device: true, effective_engine: "on_device" }));
    const { container } = renderRows();
    const select = container.querySelector<HTMLSelectElement>("[data-setting='dictation_engine']")!;
    select.value = "api";
    fireEvent.change(select);
    await waitFor(() => expect(savedDictation().engine).toBe("api"));
  });

  it("warns when on-device is chosen on a Mac without it", async () => {
    await setDictation({ engine: "on_device" });
    const { container } = renderRows();
    expect(row(container, "dictation.engine")?.textContent).toContain(
      "On-device dictation needs macOS 26 or later.",
    );
  });

  it("asks for a base URL only for a custom server", async () => {
    const { container } = renderRows();
    expect(row(container, "dictation.base_url")).toBeNull();
    await setDictation({ provider: "custom", model: "" });
    await waitFor(() => expect(row(container, "dictation.base_url")).not.toBeNull());
  });

  it("fills the preset's model on a provider change", async () => {
    const { container } = renderRows();
    const select = container.querySelector<HTMLSelectElement>(
      "[data-setting='dictation_provider']",
    )!;
    select.value = "groq";
    fireEvent.change(select);
    await waitFor(() =>
      expect(savedDictation()).toMatchObject({ provider: "groq", model: "whisper-large-v3-turbo" }),
    );
  });

  it("keeps a typed model on a provider change", async () => {
    await setDictation({ model: "my-model" });
    const { container } = renderRows();
    const select = container.querySelector<HTMLSelectElement>(
      "[data-setting='dictation_provider']",
    )!;
    select.value = "groq";
    fireEvent.change(select);
    await waitFor(() => expect(savedDictation()).toMatchObject({ provider: "groq", model: "my-model" }));
  });

  it("names the AI connection's key instead of asking for another", async () => {
    h.setEndpoint(endpoint({ key_shared_with_ai: true, key: { is_set: true, memory_only: false } }));
    const { container } = renderRows();
    expect(container.querySelector("[data-note='key-shared']")?.textContent).toBe(
      "Uses the AI connection's key.",
    );
    expect(container.querySelector("[data-setting='dictation_api_key']")).toBeNull();
  });

  it("saves a key through the store and clears the field", async () => {
    h.store.setKey.mockResolvedValue({ is_set: true, memory_only: false });
    const { container } = renderRows();
    const input = container.querySelector<HTMLInputElement>("[data-setting='dictation_api_key']")!;
    fireEvent.input(input, { target: { value: "sk-test" } });
    fireEvent.click(container.querySelector("[data-action='dictation-set-key']")!);
    await waitFor(() => expect(h.store.setKey).toHaveBeenCalledWith("sk-test"));
    await waitFor(() => expect(input.value).toBe(""));
  });

  it("clears a stored key", async () => {
    h.store.clearKey.mockResolvedValue({ is_set: false, memory_only: false });
    h.setEndpoint(endpoint({ key: { is_set: true, memory_only: false } }));
    const { container } = renderRows();
    fireEvent.click(container.querySelector("[data-action='dictation-clear-key']")!);
    await waitFor(() => expect(h.store.clearKey).toHaveBeenCalledTimes(1));
  });

  it("says when a refused key could not be saved", async () => {
    h.store.setKey.mockRejectedValue("locked");
    const { container } = renderRows();
    fireEvent.input(container.querySelector("[data-setting='dictation_api_key']")!, {
      target: { value: "sk-test" },
    });
    fireEvent.click(container.querySelector("[data-action='dictation-set-key']")!);
    await waitFor(() =>
      expect(h.showToast).toHaveBeenCalledWith("Could not save the API key", "error"),
    );
  });

  it("opens the provider's key page", async () => {
    h.setEndpoint(endpoint({ key_page: "https://platform.openai.com/api-keys" }));
    const { container } = renderRows();
    fireEvent.click(container.querySelector("[data-action='dictation-key-page']")!);
    expect(h.openExternal).toHaveBeenCalledWith("https://platform.openai.com/api-keys");
  });

  it("hides the key row where the server needs none and none is stored", async () => {
    h.setEndpoint(endpoint({ needs_key: false }));
    const { container } = renderRows();
    expect(row(container, "dictation.api_key")).toBeNull();
  });

  it("asks before audio goes to a hosted service and records the choice", async () => {
    h.getConfig.mockResolvedValue(configStore.config());
    h.setEndpoint(endpoint({ host: "api.groq.com", is_consented: false }));
    const { container } = renderRows();
    const consent = container.querySelector("[data-consent='dictation']")!;
    expect(consent.textContent).toContain("What you dictate is sent to api.groq.com.");
    fireEvent.click(container.querySelector("[data-action='dictation-consent']")!);
    await waitFor(() => expect(h.store.consentHost).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(h.getConfig).toHaveBeenCalledTimes(1));
  });

  it("says when the consent could not be recorded", async () => {
    h.store.consentHost.mockRejectedValueOnce("write failed");
    h.setEndpoint(endpoint({ is_consented: false }));
    const { container } = renderRows();
    fireEvent.click(container.querySelector("[data-action='dictation-consent']")!);
    await waitFor(() =>
      expect(h.showToast).toHaveBeenCalledWith("Could not record the choice", "error"),
    );
  });

  it("asks for nothing on this Mac", async () => {
    h.setCapabilities(capabilities({ on_device: true, effective_engine: "on_device" }));
    h.setEndpoint(endpoint({ is_consented: false }));
    const { container } = renderRows();
    expect(container.querySelector("[data-consent='dictation']")).toBeNull();
    expect(row(container, "dictation.api_key")).toBeNull();
  });

  it("offers Automatic and every language for the API", () => {
    const { container } = renderRows();
    const options = [
      ...container.querySelectorAll<HTMLOptionElement>("[data-setting='dictation_language'] option"),
    ];
    expect(options.map((o) => o.textContent)).toEqual(["Automatic", "Arabic", "English", "Swahili"]);
    expect(row(container, "dictation.language")?.textContent).not.toContain("system language");
  });

  it("narrows the languages on this Mac and says what Automatic means there", () => {
    h.setCapabilities(
      capabilities({
        on_device: true,
        effective_engine: "on_device",
        on_device_languages: ["en_US", "ar_SA"],
      }),
    );
    const { container } = renderRows();
    const options = [
      ...container.querySelectorAll<HTMLOptionElement>("[data-setting='dictation_language'] option"),
    ];
    expect(options.map((o) => o.textContent)).toEqual(["Automatic", "Arabic", "English"]);
    expect(row(container, "dictation.language")?.textContent).toContain(
      "On this Mac, Automatic uses the system language.",
    );
  });

  it("flags a chosen language this Mac cannot transcribe", async () => {
    await setDictation({ language: "sw" });
    h.setCapabilities(
      capabilities({ on_device: true, effective_engine: "on_device", on_device_languages: ["en_US"] }),
    );
    const { container } = renderRows();
    const select = container.querySelector<HTMLSelectElement>("[data-setting='dictation_language']")!;
    expect(select.value).toBe("sw");
    expect(row(container, "dictation.language")?.textContent).toContain(
      "Not available on this Mac.",
    );
  });

  it("writes the chosen language", async () => {
    const { container } = renderRows();
    const select = container.querySelector<HTMLSelectElement>("[data-setting='dictation_language']")!;
    select.value = "ar";
    fireEvent.change(select);
    await waitFor(() => expect(savedDictation().language).toBe("ar"));
  });

  it("names where clean-up sends the text once the AI connection is ready", async () => {
    const { container } = renderRows();
    const toggle = container.querySelector<HTMLButtonElement>("[data-setting='dictation_clean_up']")!;
    await waitFor(() =>
      expect(row(container, "dictation.clean_up")?.textContent).toContain(
        "Sends the text to api.deepseek.com.",
      ),
    );
    expect(toggle.disabled).toBe(false);
    fireEvent.click(toggle);
    await waitFor(() => expect(savedDictation().clean_up).toBe(true));
  });

  it("says clean-up runs here for Apple's model", async () => {
    h.aiEndpointState.mockResolvedValue(aiReady({ is_on_device: true, host: null }));
    const { container } = renderRows();
    await waitFor(() =>
      expect(row(container, "dictation.clean_up")?.textContent).toContain("Runs on this Mac."),
    );
  });

  it("keeps clean-up off until the AI connection is set up", async () => {
    h.aiEndpointState.mockResolvedValue(aiReady({ is_consented: false }));
    const { container } = renderRows();
    await waitFor(() => expect(h.aiEndpointState).toHaveBeenCalled());
    const toggle = container.querySelector<HTMLButtonElement>("[data-setting='dictation_clean_up']")!;
    expect(toggle.disabled).toBe(true);
    expect(row(container, "dictation.clean_up")?.textContent).toContain(
      "Set up the AI connection on the Chat or Rewrite page first.",
    );
  });

  it("lets clean-up be turned off after the AI connection goes away", async () => {
    await setDictation({ clean_up: true });
    h.aiEndpointState.mockRejectedValue("unreadable");
    const { container } = renderRows();
    const toggle = container.querySelector<HTMLButtonElement>("[data-setting='dictation_clean_up']")!;
    expect(toggle.disabled).toBe(false);
    fireEvent.click(toggle);
    await waitFor(() => expect(savedDictation().clean_up).toBe(false));
  });
});
