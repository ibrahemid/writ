import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

interface AiFixture {
  provider: string;
  base_url: string;
  model: string;
  consented_hosts: string[];
  rewrite: { enabled: boolean };
  chat: { enabled: boolean; model: string; model_provider: string };
}

const hoisted = vi.hoisted(() => ({
  aiCheckConnection: vi.fn(),
  aiConsentHost: vi.fn(),
  aiProviders: vi.fn(),
  aiListModels: vi.fn(),
  aiSetProvider: vi.fn(),
  configLoad: vi.fn(),
  configSave: vi.fn(),
  // Assigned once solid is imported; nothing reads it before then.
  read: null as null | (() => { ai: AiFixture }),
  ai: {
    provider: "ollama",
    base_url: "",
    model: "llama3",
    consented_hosts: [] as string[],
    rewrite: { enabled: true },
    chat: { enabled: false, model: "", model_provider: "" },
  } as AiFixture,
}));

vi.mock("../../services/tauri", () => ({
  aiCheckConnection: (...a: unknown[]) => hoisted.aiCheckConnection(...a),
  aiProviders: (...a: unknown[]) => hoisted.aiProviders(...a),
  aiListModels: (...a: unknown[]) => hoisted.aiListModels(...a),
  aiSetProvider: (...a: unknown[]) => hoisted.aiSetProvider(...a),
  aiConsentHost: (...a: unknown[]) => hoisted.aiConsentHost(...a),
}));

vi.mock("../../stores/global/config", () => ({
  configStore: {
    config: () => hoisted.read!(),
    load: (...a: unknown[]) => hoisted.configLoad(...a),
    save: (...a: unknown[]) => hoisted.configSave(...a),
  },
}));

import { createRoot, createSignal } from "solid-js";

// The connection follows the config, so the config a test reads has to be a
// signal: an effect over a plain object would never re-run.
const [configValue, setConfigValue] = createRoot(() => createSignal({ ai: hoisted.ai }));
hoisted.read = configValue;

/** Points the config at a provider and lets the effects settle. */
async function setProvider(id: string): Promise<void> {
  hoisted.ai = { ...hoisted.ai, provider: id };
  setConfigValue({ ai: hoisted.ai });
  await new Promise((resolve) => setTimeout(resolve, 0));
}

import { aiConnectionStore, connectionDisplay, modelListDisplay } from "../../stores/global/ai-connection";
import { aiProvidersStore } from "../../stores/global/ai-providers";

function catalog(provider: string, models: string[]) {
  return { provider, models, source: "live" as const, error: null };
}

function providerRow(id: string, group: "local" | "hosted" | "custom") {
  return {
    id,
    label: id,
    group,
    wire: "openai" as const,
    base_url: "",
    models_url: "",
    key_page_url: null,
    default_model: "",
    needs_key: group === "hosted",
    supports_connect: false,
    probe_port: null,
    curated_models: [] as string[],
  };
}

// `connectionDisplay` reads the provider's group, so the table it reads has to
// be loaded before the refused line can name a local runtime.
async function loadTable() {
  hoisted.aiProviders.mockResolvedValue([
    providerRow("ollama", "local"),
    providerRow("groq", "hosted"),
  ]);
  await aiProvidersStore.load();
}

const OK = { reachable: true, model_listed: true, kind: "ok", detail: "", models: ["llama3"] };

describe("ai connection store", () => {
  beforeEach(async () => {
    hoisted.aiCheckConnection.mockReset();
    hoisted.aiListModels.mockReset();
    hoisted.aiSetProvider.mockReset();
    hoisted.aiConsentHost.mockReset();
    hoisted.configLoad.mockReset();
    hoisted.ai = { ...hoisted.ai, model: "llama3", consented_hosts: [] };
    await setProvider("ollama");
    aiConnectionStore.reset();
  });

  it("checks and stores the status when enabled", async () => {
    hoisted.aiCheckConnection.mockResolvedValue(OK);
    await aiConnectionStore.check();
    expect(hoisted.aiCheckConnection).toHaveBeenCalledTimes(1);
    expect(aiConnectionStore.status()?.kind).toBe("ok");
    expect(aiConnectionStore.checking()).toBe(false);
  });

  // One connection, checked whether or not a feature is switched on: the check
  // is what tells a person the connection works before they turn one on.
  it("checks with both features off", async () => {
    hoisted.ai.rewrite.enabled = false;
    hoisted.ai.chat.enabled = false;
    hoisted.aiCheckConnection.mockResolvedValue(OK);
    await aiConnectionStore.check();
    expect(hoisted.aiCheckConnection).toHaveBeenCalledTimes(1);
    expect(aiConnectionStore.status()?.kind).toBe("ok");
  });

  it("records an error status when the probe throws", async () => {
    hoisted.aiCheckConnection.mockRejectedValue(new Error("boom"));
    await aiConnectionStore.check();
    expect(aiConnectionStore.status()?.kind).toBe("error");
    expect(aiConnectionStore.checking()).toBe(false);
  });

  it("drops a catalog answered for a provider that is no longer current", async () => {
    // The answer names the provider it was read for, so a list that arrives
    // after the connection moved is discarded rather than offered.
    await setProvider("deepseek");
    hoisted.aiListModels.mockResolvedValue(catalog("ollama", ["qwen3:4b"]));
    await aiConnectionStore.refreshCatalog();
    expect(aiConnectionStore.catalog()).toBeNull();

    hoisted.aiListModels.mockResolvedValue(catalog("deepseek", ["deepseek-chat"]));
    await aiConnectionStore.refreshCatalog();
    expect(aiConnectionStore.catalog()?.models).toEqual(["deepseek-chat"]);

    // And a catalog held from before a change stops being current the moment
    // the provider does.
    await setProvider("groq");
    expect(aiConnectionStore.catalog()).toBeNull();
  });

  it("refetches the catalog when the provider changes", async () => {
    hoisted.aiListModels.mockImplementation(() =>
      Promise.resolve(catalog(hoisted.ai.provider, ["a-model"])),
    );
    aiConnectionStore.watch();
    await new Promise((resolve) => setTimeout(resolve, 0));
    const before = hoisted.aiListModels.mock.calls.length;
    expect(before).toBeGreaterThan(0);

    await setProvider("groq");
    expect(hoisted.aiListModels.mock.calls.length).toBeGreaterThan(before);
    expect(aiConnectionStore.catalog()?.provider).toBe("groq");
  });

  // The defect an operator hit: the consent was recorded in Rust and the
  // frontend kept a copy that predated it, so the next settings write emptied
  // `consented_hosts` on disk and the pane still said the host was not
  // allowed.
  it("re-reads the config after a consent, which brings the live list", async () => {
    await setProvider("deepseek");
    hoisted.aiConsentHost.mockResolvedValue({
      host: "api.deepseek.com",
      host_port: "api.deepseek.com",
      is_hosted: true,
      is_allowed: true,
      is_consented: true,
      key_state: { is_set: true, memory_only: false },
      provider: "deepseek",
    });
    // `configStore.load` re-reads what Rust wrote, which is the consent.
    hoisted.configLoad.mockImplementation(async () => {
      hoisted.ai = { ...hoisted.ai, consented_hosts: ["api.deepseek.com"] };
      setConfigValue({ ai: hoisted.ai });
      return true;
    });
    // The list waits for consent, so the first read answers the refusal and
    // the second answers the provider's own ids.
    hoisted.aiListModels.mockResolvedValue({
      provider: "deepseek",
      models: ["deepseek-flash", "deepseek-v4-pro"],
      source: "curated",
      error: { kind: "consent_required" },
    });
    aiConnectionStore.watch();
    await new Promise((resolve) => setTimeout(resolve, 0));
    const before = hoisted.aiListModels.mock.calls.length;

    hoisted.aiListModels.mockResolvedValue(
      catalog("deepseek", ["deepseek-flash", "deepseek-v4-pro"]),
    );
    const state = await aiConnectionStore.consentHost();

    expect(state.is_consented).toBe(true);
    expect(hoisted.configLoad).toHaveBeenCalled();
    expect(hoisted.ai.consented_hosts).toEqual(["api.deepseek.com"]);
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(hoisted.aiListModels.mock.calls.length).toBeGreaterThan(before);
    expect(aiConnectionStore.catalog()?.source).toBe("live");
    expect(aiConnectionStore.catalog()?.models).toEqual(["deepseek-flash", "deepseek-v4-pro"]);
  });

  it("debounces scheduled checks into one call", async () => {
    vi.useFakeTimers();
    hoisted.aiCheckConnection.mockResolvedValue(OK);
    aiConnectionStore.scheduleCheck(400);
    aiConnectionStore.scheduleCheck(400);
    expect(hoisted.aiCheckConnection).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(400);
    expect(hoisted.aiCheckConnection).toHaveBeenCalledTimes(1);
    vi.useRealTimers();
  });
});

describe("connectionDisplay", () => {
  beforeEach(async () => {
    await loadTable();
  });

  afterEach(() => {
    hoisted.ai.provider = "ollama";
  });

  it("maps every state to a line and tone", () => {
    expect(connectionDisplay(null, "llama3")).toEqual({ text: "Not checked", tone: "idle" });
    expect(
      connectionDisplay({ reachable: true, model_listed: true, kind: "ok", detail: "", models: [] }, "llama3"),
    ).toEqual({ text: "Connected", tone: "ok" });

    const missing = connectionDisplay(
      { reachable: true, model_listed: false, kind: "model_missing", detail: "llama3", models: [] },
      "llama3",
    );
    expect(missing.tone).toBe("warn");
    expect(missing.text).toContain("llama3");

    const auth = connectionDisplay(
      { reachable: true, model_listed: null, kind: "unauthorized", detail: "api.groq.com", models: [] },
      "",
    );
    expect(auth.tone).toBe("error");
    expect(auth.text).toContain("api.groq.com");

    hoisted.ai.provider = "ollama";
    const refusedLocal = connectionDisplay(
      { reachable: false, model_listed: null, kind: "refused", detail: "127.0.0.1:11434", models: [] },
      "",
    );
    expect(refusedLocal.tone).toBe("error");
    expect(refusedLocal.text.toLowerCase()).toContain("ollama");

    hoisted.ai.provider = "groq";
    const refusedHosted = connectionDisplay(
      { reachable: false, model_listed: null, kind: "refused", detail: "api.groq.com", models: [] },
      "",
    );
    expect(refusedHosted.text.toLowerCase()).not.toContain("ollama");
  });

  // A hosted endpoint is not probed before consent, so this state means "no
  // request was made", not "the endpoint failed".
  it("reads a blocked hosted probe as needing consent, not as a failure", () => {
    const consent = connectionDisplay(
      {
        reachable: false,
        model_listed: null,
        kind: "consent_required",
        detail: "api.groq.com",
        models: [],
      },
      "llama3",
    );
    expect(consent.tone).toBe("warn");
    expect(consent.text).toBe("Not checked until you allow api.groq.com");
  });

  // A hosted provider with no key is not reached at all, and the line has to
  // name the missing step rather than report a dead endpoint.
  it("names the missing key instead of a failure", () => {
    const needsKey = connectionDisplay(
      { reachable: false, model_listed: null, kind: "key_required", detail: "api.groq.com", models: [] },
      "",
    );
    expect(needsKey).toEqual({
      text: "Add an API key to check the connection.",
      tone: "warn",
    });
  });
});

describe("modelListDisplay", () => {
  it("writes a line for every kind the list can fail with", () => {
    expect(modelListDisplay({ kind: "unreachable" }, "api.groq.com")).toEqual({
      text: "Could not reach api.groq.com",
      tone: "error",
    });
    expect(modelListDisplay({ kind: "timeout" }, "api.groq.com")).toEqual({
      text: "No response from api.groq.com within 5 seconds",
      tone: "error",
    });
    expect(modelListDisplay({ kind: "unauthorized" }, "api.groq.com")).toEqual({
      text: "api.groq.com rejected the API key. Check it.",
      tone: "error",
    });
    expect(modelListDisplay({ kind: "status", code: 503 }, "api.groq.com")).toEqual({
      text: "The server returned status 503",
      tone: "error",
    });
    expect(modelListDisplay({ kind: "malformed" }, "api.groq.com")).toEqual({
      text: "The server's model list could not be read",
      tone: "error",
    });
    expect(modelListDisplay({ kind: "consent_required" }, "api.groq.com")).toEqual({
      text: "Not checked until you allow api.groq.com",
      tone: "warn",
    });
  });

  // The error's own text is never shown (ADR-031 rule 5.3).
  it("never shows the error text itself", () => {
    const line = modelListDisplay({ kind: "status", code: 418 }, "api.groq.com");
    expect(line.text).not.toContain("status\":");
  });
});
