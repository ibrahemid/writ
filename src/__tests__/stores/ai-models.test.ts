import { describe, it, expect, beforeAll, vi } from "vitest";

const hoisted = vi.hoisted(() => ({ aiProviders: vi.fn() }));

vi.mock("../../services/tauri", () => ({
  aiProviders: (...a: unknown[]) => hoisted.aiProviders(...a),
}));

import {
  curatedModels,
  curatedProviderIds,
  defaultModelFor,
  modelOptions,
  resolveAutoModel,
} from "../../stores/global/ai-models";
import { aiProvidersStore } from "../../stores/global/ai-providers";

// The curated ids are rows of the provider table `writ-core` owns (ADR-040
// section 2), so these read the table rather than a second copy. The ids
// themselves are pinned on the Rust side.
const TABLE = [
  row("ollama", "local", "", ["qwen3:4b", "qwen3:8b"]),
  row("lmstudio", "local", "", []),
  row("groq", "hosted", "llama-3.3-70b-versatile", ["llama-3.3-70b-versatile", "llama-3.1-8b-instant"]),
  row("gemini", "hosted", "gemini-2.5-flash", ["gemini-2.5-flash", "gemini-2.5-flash-lite"]),
  row("custom", "custom", "", []),
];

function row(
  id: string,
  group: "local" | "hosted" | "custom",
  defaultModel: string,
  curated: string[],
) {
  return {
    id,
    label: id,
    group,
    wire: "openai" as const,
    base_url: "",
    models_url: "",
    key_page_url: null,
    default_model: defaultModel,
    needs_key: group === "hosted",
    supports_connect: false,
    probe_port: null,
    curated_models: curated,
  };
}

beforeAll(async () => {
  hoisted.aiProviders.mockResolvedValue(TABLE);
  await aiProvidersStore.load();
});

describe("model picker options", () => {
  it("prefers the live list, falls back to curated", () => {
    expect(modelOptions("ollama", ["qwen3:8b", "phi4"])).toEqual(["qwen3:8b", "phi4"]);
    expect(modelOptions("ollama", [])).toEqual(["qwen3:4b", "qwen3:8b"]);
    expect(modelOptions("groq", [])).toEqual(["llama-3.3-70b-versatile", "llama-3.1-8b-instant"]);
  });

  it("answers for every provider in the table", () => {
    expect([...curatedProviderIds()].sort()).toEqual(TABLE.map((r) => r.id).sort());
  });

  it("custom and LM Studio have no curated list", () => {
    expect(curatedModels("custom")).toEqual([]);
    expect(defaultModelFor("custom")).toBe("");
    expect(curatedModels("lmstudio")).toEqual([]);
  });

  it("gives each provider a default: the row's own, else its first suggestion", () => {
    expect(defaultModelFor("groq")).toBe("llama-3.3-70b-versatile");
    expect(defaultModelFor("gemini")).toBe("gemini-2.5-flash");
    expect(defaultModelFor("ollama")).toBe("qwen3:4b");
  });

  it("answers nothing for a provider the table does not carry", () => {
    expect(curatedModels("nope")).toEqual([]);
    expect(defaultModelFor("nope")).toBe("");
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
