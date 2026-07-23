import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

const hoisted = vi.hoisted(() => ({
  aiCheckConnection: vi.fn(),
  ai: {
    enabled: true,
    preset: "ollama",
    base_url: "http://localhost:11434/v1",
    model: "llama3",
    consented_hosts: [] as string[],
  },
}));

vi.mock("../../services/tauri", () => ({
  aiCheckConnection: (...a: unknown[]) => hoisted.aiCheckConnection(...a),
}));

vi.mock("../../stores/global/config", () => ({
  configStore: { config: () => ({ ai: hoisted.ai }) },
}));

import { aiConnectionStore, connectionDisplay } from "../../stores/global/ai-connection";

const OK = { reachable: true, model_listed: true, kind: "ok", detail: "", models: ["llama3"] };

describe("ai connection store", () => {
  beforeEach(() => {
    hoisted.aiCheckConnection.mockReset();
    hoisted.ai.enabled = true;
    hoisted.ai.base_url = "http://localhost:11434/v1";
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

  it("does nothing when the feature is off", async () => {
    hoisted.ai.enabled = false;
    await aiConnectionStore.check();
    expect(hoisted.aiCheckConnection).not.toHaveBeenCalled();
    expect(aiConnectionStore.status()).toBeNull();
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
  afterEach(() => {
    hoisted.ai.base_url = "http://localhost:11434/v1";
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
      { reachable: true, model_listed: null, kind: "unauthorized", detail: "401", models: [] },
      "",
    );
    expect(auth.tone).toBe("error");
    expect(auth.text).toContain("401");

    hoisted.ai.base_url = "http://localhost:11434/v1";
    const refusedLocal = connectionDisplay(
      { reachable: false, model_listed: null, kind: "refused", detail: "127.0.0.1:11434", models: [] },
      "",
    );
    expect(refusedLocal.tone).toBe("error");
    expect(refusedLocal.text.toLowerCase()).toContain("ollama");

    hoisted.ai.base_url = "https://api.groq.com/openai/v1";
    const refusedHosted = connectionDisplay(
      { reachable: false, model_listed: null, kind: "refused", detail: "api.groq.com", models: [] },
      "",
    );
    expect(refusedHosted.text.toLowerCase()).not.toContain("ollama");
  });
});
