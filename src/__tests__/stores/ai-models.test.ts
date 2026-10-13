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
  seedModel,
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

describe("seedModel", () => {
  it("starts from the row's default when the list holds it, whatever sorts first", () => {
    expect(seedModel("gemini", ["gemini-2.0-flash", "gemini-2.5-flash", "gemini-2.5-flash-lite"])).toBe(
      "gemini-2.5-flash",
    );
    expect(seedModel("groq", ["allam-2-7b", "llama-3.3-70b-versatile"])).toBe("llama-3.3-70b-versatile");
  });

  it("falls to the first suggestion the list holds, in the row's order", () => {
    expect(seedModel("gemini", ["gemini-2.0-flash", "gemini-2.5-flash-lite"])).toBe("gemini-2.5-flash-lite");
    expect(seedModel("ollama", ["mistral:latest", "qwen3:8b"])).toBe("qwen3:8b");
  });

  it("seeds nothing from a list that holds none of the row's ids", () => {
    expect(seedModel("ollama", ["all-minilm:latest", "mistral:latest"])).toBe("");
    expect(seedModel("lmstudio", ["gemma-3-12b", "qwen2.5-7b-instruct"])).toBe("");
    expect(seedModel("groq", [])).toBe("");
    expect(seedModel("nope", ["anything"])).toBe("");
  });

  it("seeds the one model a local runtime lists, and only a local runtime", () => {
    expect(seedModel("lmstudio", ["qwen2.5-7b-instruct"])).toBe("qwen2.5-7b-instruct");
    expect(seedModel("ollama", ["phi4"])).toBe("phi4");
    expect(seedModel("groq", ["allam-2-7b"])).toBe("");
    expect(seedModel("custom", ["my-model"])).toBe("");
  });
});

describe("resolveAutoModel", () => {
  it("fills an empty model with the seed, and from nothing else", () => {
    expect(resolveAutoModel({ provider: "ollama", model: "", live: ["phi4", "qwen3:4b"], userSelected: false })).toBe(
      "qwen3:4b",
    );
    // Ids the list holds but nobody chose are left for the picker.
    expect(
      resolveAutoModel({ provider: "ollama", model: "", live: ["mistral:latest", "phi4"], userSelected: false }),
    ).toBeNull();
    // A runtime that has one model installed starts from it.
    expect(resolveAutoModel({ provider: "ollama", model: "", live: ["phi4"], userSelected: false })).toBe("phi4");
    // A suggestion is not the account's inventory: the row a person picked is
    // left without a model until its own list answers, rather than filled with
    // an id the provider may have retired.
    expect(resolveAutoModel({ provider: "ollama", model: "", live: [], userSelected: false })).toBeNull();
    expect(resolveAutoModel({ provider: "groq", model: "", live: [], userSelected: false })).toBeNull();
  });

  it("keeps a valid model", () => {
    expect(resolveAutoModel({ provider: "ollama", model: "phi4", live: ["phi4"], userSelected: false })).toBeNull();
  });

  it("switches Ollama off a not-installed model to the seed, unless the user chose it", () => {
    expect(resolveAutoModel({ provider: "ollama", model: "phi4", live: ["qwen3:8b"], userSelected: false })).toBe(
      "qwen3:8b",
    );
    expect(
      resolveAutoModel({ provider: "ollama", model: "phi4", live: ["qwen3:8b"], userSelected: true }),
    ).toBeNull();
    // With nothing to seed, the model the file names is left for the picker to
    // replace rather than swapped for whatever is installed.
    expect(
      resolveAutoModel({
        provider: "ollama",
        model: "phi4",
        live: ["llama3.1:8b", "mistral:latest"],
        userSelected: false,
      }),
    ).toBeNull();
    // The one model installed is the seed.
    expect(
      resolveAutoModel({ provider: "ollama", model: "phi4", live: ["mistral:latest"], userSelected: false }),
    ).toBe("mistral:latest");
  });

  it("does not second-guess a hosted model that is set", () => {
    // Hosted /models may omit ids or differ; a set model is left alone.
    expect(
      resolveAutoModel({ provider: "groq", model: "some-model", live: ["other"], userSelected: false }),
    ).toBeNull();
  });
});
