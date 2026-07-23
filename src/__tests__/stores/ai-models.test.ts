import { describe, it, expect } from "vitest";
import {
  curatedModels,
  defaultModelFor,
  modelOptions,
  resolveAutoModel,
} from "../../stores/global/ai-models";

describe("model picker options", () => {
  it("prefers the live list, falls back to curated", () => {
    expect(modelOptions("ollama", ["qwen3:8b", "phi4"])).toEqual(["qwen3:8b", "phi4"]);
    expect(modelOptions("ollama", [])).toEqual(curatedModels("ollama"));
    expect(modelOptions("groq", [])).toEqual(["llama-3.3-70b-versatile", "llama-3.1-8b-instant"]);
  });

  it("custom has no curated list", () => {
    expect(curatedModels("custom")).toEqual([]);
    expect(defaultModelFor("custom")).toBe("");
  });

  it("gives each provider a default", () => {
    expect(defaultModelFor("ollama")).toBe("qwen3:4b");
    expect(defaultModelFor("groq")).toBe("llama-3.3-70b-versatile");
    expect(defaultModelFor("gemini")).toBe("gemini-2.5-flash");
    expect(defaultModelFor("deepseek")).toBe("deepseek-chat");
  });
});

describe("resolveAutoModel", () => {
  it("fills an empty model from the live list first, else curated", () => {
    expect(resolveAutoModel({ preset: "ollama", model: "", live: ["phi4"], userSelected: false })).toBe("phi4");
    expect(resolveAutoModel({ preset: "ollama", model: "", live: [], userSelected: false })).toBe("qwen3:4b");
    expect(resolveAutoModel({ preset: "groq", model: "", live: [], userSelected: false })).toBe(
      "llama-3.3-70b-versatile",
    );
  });

  it("keeps a valid model", () => {
    expect(resolveAutoModel({ preset: "ollama", model: "phi4", live: ["phi4"], userSelected: false })).toBeNull();
  });

  it("switches Ollama off a not-installed model, unless the user chose it", () => {
    expect(resolveAutoModel({ preset: "ollama", model: "phi4", live: ["qwen3:8b"], userSelected: false })).toBe(
      "qwen3:8b",
    );
    expect(
      resolveAutoModel({ preset: "ollama", model: "phi4", live: ["qwen3:8b"], userSelected: true }),
    ).toBeNull();
  });

  it("does not second-guess a hosted model that is set", () => {
    // Hosted /models may omit ids or differ; a set model is left alone.
    expect(
      resolveAutoModel({ preset: "groq", model: "some-model", live: ["other"], userSelected: false }),
    ).toBeNull();
  });
});
