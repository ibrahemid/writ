import { describe, it, expect } from "vitest";
import {
  curatedModels,
  curatedProviderIds,
  defaultModelFor,
  modelOptions,
  resolveAutoModel,
} from "../../stores/global/ai-models";

// The ids of the provider table (ADR-040 section 2), as a literal. `AiPreset`
// was an exhaustive union and is gone, so the pin is what now catches a
// provider added to the table and forgotten here.
const PROVIDER_IDS = [
  "ollama",
  "lmstudio",
  "anthropic",
  "openai",
  "gemini",
  "openrouter",
  "groq",
  "deepseek",
  "mistral",
  "xai",
  "together",
  "fireworks",
  "custom",
];

describe("model picker options", () => {
  it("prefers the live list, falls back to curated", () => {
    expect(modelOptions("ollama", ["qwen3:8b", "phi4"])).toEqual(["qwen3:8b", "phi4"]);
    expect(modelOptions("ollama", [])).toEqual(curatedModels("ollama"));
    expect(modelOptions("groq", [])).toEqual(["llama-3.3-70b-versatile", "llama-3.1-8b-instant"]);
  });

  it("answers for every provider in the table", () => {
    expect([...curatedProviderIds()].sort()).toEqual([...PROVIDER_IDS].sort());
  });

  it("custom and LM Studio have no curated list", () => {
    expect(curatedModels("custom")).toEqual([]);
    expect(defaultModelFor("custom")).toBe("");
    expect(curatedModels("lmstudio")).toEqual([]);
  });

  it("gives each provider a default", () => {
    expect(defaultModelFor("ollama")).toBe("qwen3:4b");
    expect(defaultModelFor("groq")).toBe("llama-3.3-70b-versatile");
    expect(defaultModelFor("gemini")).toBe("gemini-2.5-flash");
    expect(defaultModelFor("deepseek")).toBe("deepseek-chat");
    expect(defaultModelFor("anthropic")).toBe("claude-sonnet-5");
    expect(defaultModelFor("openai")).toBe("gpt-5-mini");
    expect(defaultModelFor("mistral")).toBe("mistral-small-latest");
    expect(defaultModelFor("xai")).toBe("grok-4");
    expect(defaultModelFor("together")).toBe("meta-llama/Llama-3.3-70B-Instruct-Turbo");
    expect(defaultModelFor("fireworks")).toBe("accounts/fireworks/models/llama-v3p3-70b-instruct");
  });
});

describe("resolveAutoModel", () => {
  it("fills an empty model from the live list first, else curated", () => {
    expect(resolveAutoModel({ provider: "ollama", model: "", live: ["phi4"], userSelected: false })).toBe("phi4");
    expect(resolveAutoModel({ provider: "ollama", model: "", live: [], userSelected: false })).toBe("qwen3:4b");
    expect(resolveAutoModel({ provider: "groq", model: "", live: [], userSelected: false })).toBe(
      "llama-3.3-70b-versatile",
    );
  });

  it("keeps a valid model", () => {
    expect(resolveAutoModel({ provider: "ollama", model: "phi4", live: ["phi4"], userSelected: false })).toBeNull();
  });

  it("switches Ollama off a not-installed model, unless the user chose it", () => {
    expect(resolveAutoModel({ provider: "ollama", model: "phi4", live: ["qwen3:8b"], userSelected: false })).toBe(
      "qwen3:8b",
    );
    expect(
      resolveAutoModel({ provider: "ollama", model: "phi4", live: ["qwen3:8b"], userSelected: true }),
    ).toBeNull();
  });

  it("does not second-guess a hosted model that is set", () => {
    // Hosted /models may omit ids or differ; a set model is left alone.
    expect(
      resolveAutoModel({ provider: "groq", model: "some-model", live: ["other"], userSelected: false }),
    ).toBeNull();
  });
});
