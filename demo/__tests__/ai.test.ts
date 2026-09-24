import { describe, expect, it } from "vitest";
import type { AiConfig } from "../../src/types/config";
import {
  AiRefusal,
  findChatRefusal,
  getChatState,
  buildChatTransportFrame,
  checkConnection,
  consentHost,
  getEndpointState,
  listModels,
  getMcpServerCommand,
  MCP_TOOLS,
  PROVIDERS,
  resolveEndpoint,
  findRewriteRefusal,
  getRewriteStreamError,
  applyProvider,
} from "../backend/ai";
import { DEMO_CONFIG } from "../backend/config";

const NO_KEY = { is_set: false, memory_only: false };
const base = (): AiConfig => structuredClone(DEMO_CONFIG.ai);
const hosted = (consented: boolean): AiConfig => ({
  ...base(),
  provider: "anthropic",
  model: "claude-sonnet-5",
  consented_hosts: consented ? ["api.anthropic.com"] : [],
  rewrite: { enabled: true },
  chat: { enabled: true, model: "", model_provider: "" },
});

describe("the provider table and endpoint rules", () => {
  it("carries the thirteen rows writ-core serialises", () => {
    expect(PROVIDERS.map((row) => row.id)).toEqual([
      "ollama", "lmstudio", "anthropic", "openai", "gemini", "openrouter", "groq", "deepseek", "mistral", "xai", "together", "fireworks", "custom",
    ]);
  });

  it("resolves hosts the way polish::resolve_endpoint does", () => {
    expect(resolveEndpoint("http://localhost:11434/v1")).toEqual({ host: "localhost", host_port: "localhost:11434", is_hosted: false, is_allowed: true });
    expect(resolveEndpoint("https://api.anthropic.com")).toMatchObject({ host_port: "api.anthropic.com", is_hosted: true, is_allowed: true });
    expect(resolveEndpoint("http://example.com")?.is_allowed).toBe(false);
    expect(resolveEndpoint("not a url")).toBeNull();
  });

  it("reports a local endpoint as needing no consent and no key", () => {
    expect(getEndpointState(base(), { is_set: true, memory_only: true })).toEqual({
      host: "localhost",
      host_port: "localhost:11434",
      is_hosted: false,
      is_allowed: true,
      is_consented: true,
      provider: "ollama",
      key_state: NO_KEY,
    });
    expect(getChatState(base(), NO_KEY)).toMatchObject({ enabled: false, model: "", is_consented: true });
  });
});

describe("what a page answers when nothing is reachable", () => {
  it("lists the curated models with the failure that stopped the live list", () => {
    expect(listModels(base())).toEqual({ provider: "ollama", models: ["qwen3:4b", "qwen3:8b", "gemma3:4b", "llama3.2:3b"], source: "curated", error: { kind: "unreachable" } });
    expect(listModels({ ...base(), provider: "lmstudio" })).toEqual({ provider: "lmstudio", models: [], source: "none", error: { kind: "unreachable" } });
    expect(listModels(hosted(false)).error).toEqual({ kind: "consent_required" });
  });

  it("checks the connection in the command's gate order", () => {
    expect(checkConnection(base(), false)).toEqual({ reachable: false, model_listed: null, kind: "refused", detail: "localhost:11434", models: [] });
    expect(checkConnection(hosted(false), false).kind).toBe("consent_required");
    expect(checkConnection(hosted(true), false)).toMatchObject({ kind: "key_required", detail: "api.anthropic.com" });
    expect(checkConnection(hosted(true), true)).toMatchObject({ kind: "refused", detail: "api.anthropic.com" });
    expect(checkConnection({ ...base(), provider: "custom", base_url: "" }, false).kind).toBe("invalid_url");
  });

  it("refuses a rewrite for the reason the app gives, and fails a sent one as unreachable", () => {
    expect(findRewriteRefusal(base(), "proofread", "text", null, false)).toBe("Rewriting is turned off.");
    const on = { ...base(), rewrite: { enabled: true } };
    expect(findRewriteRefusal(on, "custom", "text", " ", false)).toBe("a custom rewrite needs an instruction");
    expect(findRewriteRefusal(on, "proofread", "  ", null, false)).toBe("there is no text to rewrite");
    expect(findRewriteRefusal(on, "proofread", "text", null, false)).toBe("Choose a model in AI settings.");
    expect(findRewriteRefusal(hosted(false), "polish", "text", null, false)).toBe("Confirm sending text to api.anthropic.com first.");
    expect(findRewriteRefusal(hosted(true), "polish", "text", null, false)).toBe("Add an API key for api.anthropic.com first.");
    expect(findRewriteRefusal({ ...on, model: "qwen3:4b" }, "polish", "text", null, false)).toBeNull();
    expect(getRewriteStreamError(on)).toBe("Could not reach the local model server. Is Ollama running?");
  });

  it("refuses or fails a chat send the way the chat command does", () => {
    const chat = { ...base(), chat: { enabled: true, model: "", model_provider: "" } };
    expect(findChatRefusal(base(), "hi", false)).toBe("Chat is turned off.");
    expect(findChatRefusal(chat, "hi", false)).toBe("Choose a chat model in AI settings.");
    expect(findChatRefusal(hosted(false), "hi", true)).toBe("Confirm sending your files to api.anthropic.com first.");
    expect(buildChatTransportFrame({ ...chat, model: "qwen3:4b" })).toEqual({
      kind: "local_server_offline",
      message: "Ollama is not running at localhost:11434.",
      provider: "ollama",
      model: "qwen3:4b",
      status: null,
    });
  });
});

describe("changing the connection", () => {
  it("records consent for a hosted host only", () => {
    expect(consentHost(hosted(false)).consented_hosts).toEqual(["api.anthropic.com"]);
    expect(() => consentHost(base())).toThrow(AiRefusal);
    expect(() => consentHost(base())).toThrow("This endpoint is on your machine; nothing is sent.");
  });

  it("switches provider, drops a model the old one owned and refuses an unknown row", () => {
    const switched = applyProvider({ ...base(), model: "qwen3:4b", chat: { enabled: true, model: "qwen3:8b", model_provider: "ollama" } }, "openai");
    expect(switched).toMatchObject({ provider: "openai", model: "", chat: { enabled: true, model: "", model_provider: "" } });
    expect(applyProvider({ ...base(), model: "mine" }, "custom").model).toBe("mine");
    expect(() => applyProvider(base(), "nope")).toThrow("That provider is not one this version knows.");
  });
});

describe("connected programs", () => {
  it("hands out the bundled binary's command and the static tool list", () => {
    expect(getMcpServerCommand()).toEqual({
      path: "/Applications/Writ.app/Contents/MacOS/writ",
      command: '"/Applications/Writ.app/Contents/MacOS/writ" mcp',
    });
    expect(MCP_TOOLS.write).toEqual(["write_note", "create_note", "rename_note"]);
    expect(MCP_TOOLS.read).toContain("note_backlinks");
  });
});
