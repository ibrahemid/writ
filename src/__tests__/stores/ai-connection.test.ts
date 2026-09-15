import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

const hoisted = vi.hoisted(() => ({
  aiCheckConnection: vi.fn(),
  aiProviders: vi.fn(),
  ai: {
    provider: "ollama",
    base_url: "",
    model: "llama3",
    consented_hosts: [] as string[],
    rewrite: { enabled: true },
    chat: { enabled: false, model: "" },
  },
}));

vi.mock("../../services/tauri", () => ({
  aiCheckConnection: (...a: unknown[]) => hoisted.aiCheckConnection(...a),
  aiProviders: (...a: unknown[]) => hoisted.aiProviders(...a),
}));

vi.mock("../../stores/global/config", () => ({
  configStore: { config: () => ({ ai: hoisted.ai }) },
}));

import { aiConnectionStore, connectionDisplay, modelListDisplay } from "../../stores/global/ai-connection";
import { aiProvidersStore } from "../../stores/global/ai-providers";

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
  beforeEach(() => {
    hoisted.aiCheckConnection.mockReset();
    hoisted.ai.provider = "ollama";
    hoisted.ai.model = "llama3";
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
