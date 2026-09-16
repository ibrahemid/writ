import { describe, it, expect, vi, beforeEach } from "vitest";
import { createSignal } from "solid-js";
import type { WritConfig } from "../../types/config";

// One connection serves the pane and the settings panel, so the pane holds no
// inventory of its own. What this pins down is that the pane reads the same
// catalog, drops it when the connection moves, and says which connection
// refused.

const mocks = vi.hoisted(() => ({
  aiListModels: vi.fn(),
  aiCheckConnection: vi.fn(),
  aiSetProvider: vi.fn(),
  aiProbeLocal: vi.fn(),
  aiOpenrouterConnect: vi.fn(),
  aiOpenrouterCancel: vi.fn(),
  groupOf: vi.fn<(provider: string) => string>(() => "hosted"),
  applyAi: vi.fn(),
  save: vi.fn(),
  load: vi.fn(),
  chatList: vi.fn(),
  chatOpen: vi.fn(),
  chatNew: vi.fn(),
  chatSend: vi.fn(),
  chatRenderReply: vi.fn(),
  chatAttachedSizes: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  aiListModels: mocks.aiListModels,
  aiCheckConnection: mocks.aiCheckConnection,
  aiSetProvider: mocks.aiSetProvider,
  aiProbeLocal: mocks.aiProbeLocal,
  aiOpenrouterConnect: mocks.aiOpenrouterConnect,
  aiOpenrouterCancel: mocks.aiOpenrouterCancel,
  chatState: vi.fn(),
  chatAttachedSizes: mocks.chatAttachedSizes,
  chatList: mocks.chatList,
  chatOpen: mocks.chatOpen,
  chatNew: mocks.chatNew,
  chatRename: vi.fn(),
  chatDelete: vi.fn(),
  chatRenderReply: mocks.chatRenderReply,
  chatSend: mocks.chatSend,
  chatStop: vi.fn(),
  chatApplyProposal: vi.fn(),
  chatDiscardProposal: vi.fn(),
}));

const [config, setConfig] = createSignal(ai("ollama", "qwen2.5"));

vi.mock("../../stores/global/config", () => ({
  configStore: {
    config: () => config(),
    applyAi: mocks.applyAi,
    save: mocks.save,
    load: mocks.load,
  },
}));

vi.mock("../../stores/global/ai-providers", () => ({
  aiProvidersStore: { groupOf: mocks.groupOf },
}));

import { aiConnectionStore } from "../../stores/global/ai-connection";
import { chatStore } from "../../stores/global/chat";

/** Only the part of the config these surfaces read. */
function ai(provider: string, model: string, chatEnabled = true): WritConfig {
  return {
    ai: {
      provider,
      base_url: "",
      model,
      consented_hosts: [],
      rewrite: {},
      chat: { enabled: chatEnabled, model: "", model_provider: "" },
    },
  } as unknown as WritConfig;
}

function catalog(provider: string, models: string[], source = "live") {
  return { provider, models, source, error: null };
}

function probe(kind: string, detail: string) {
  return { reachable: false, model_listed: null, kind, detail, models: [] };
}

async function flush() {
  await new Promise((resolve) => setTimeout(resolve, 0));
  await new Promise((resolve) => setTimeout(resolve, 0));
}

beforeEach(async () => {
  for (const mock of Object.values(mocks)) mock.mockReset();
  mocks.groupOf.mockReturnValue("hosted");
  mocks.chatList.mockResolvedValue([]);
  mocks.chatRenderReply.mockImplementation(async (text: string) => `<p>${text}</p>`);
  mocks.chatAttachedSizes.mockResolvedValue([]);
  mocks.chatNew.mockResolvedValue({
    id: "c1",
    title: "c1",
    created_at: "2026-09-16T10:00:00+00:00",
    updated_at: "2026-09-16T10:00:00+00:00",
    provider: "ollama",
    model: "qwen2.5",
    turns: [],
  });
  mocks.chatOpen.mockImplementation(async (id: string) => ({
    id,
    title: id,
    created_at: "2026-09-16T10:00:00+00:00",
    updated_at: "2026-09-16T10:00:00+00:00",
    provider: "ollama",
    model: "qwen2.5",
    turns: [],
  }));
  mocks.chatSend.mockResolvedValue({ conversation_id: "c1", request_id: "r", attached: [] });
  setConfig(ai("ollama", "qwen2.5"));
  aiConnectionStore.reset();
  chatStore.reset();
});

describe("the catalog the pane reads", () => {
  it("clears the stale list when config changes on disk", async () => {
    mocks.aiListModels.mockImplementation(async (provider: string) =>
      catalog(provider, provider === "ollama" ? ["qwen2.5"] : ["claude-4"]),
    );
    aiConnectionStore.watch();
    await flush();
    expect(chatStore.liveModels()).toEqual(["qwen2.5"]);

    // An external edit of config.toml reaches the running config through
    // config-sync, and the list read for the connection it replaced is not an
    // inventory of the new one.
    setConfig(ai("anthropic", "claude-4"));
    expect(chatStore.liveModels()).toEqual([]);

    await flush();
    expect(chatStore.liveModels()).toEqual(["claude-4"]);
  });
});

describe("what the pane says about the connection", () => {
  it("an error names the provider and the model that was refused", async () => {
    chatStore.setDraft("what does it argue");
    await chatStore.send();
    const requestId = mocks.chatSend.mock.calls[0][4] as string;

    chatStore.handleStreamEvent({
      conversation_id: "c1",
      request_id: requestId,
      kind: "error",
      text: "The reply did not arrive.",
      error: {
        kind: "provider_rejected",
        message: "DeepSeek rejected qwen2.5-coder:0.5b: the model does not exist.",
        provider: "deepseek",
        model: "qwen2.5-coder:0.5b",
        status: 400,
      },
    });
    await flush();

    expect(chatStore.status()).toBe("error");
    expect(chatStore.errorKind()).toBe("provider_rejected");
    expect(chatStore.errorIdentity()).toMatchObject({
      provider: "deepseek",
      model: "qwen2.5-coder:0.5b",
    });
    expect(chatStore.errorMessage()).toContain("qwen2.5-coder:0.5b");
  });

  it("keeps an untyped failure's own sentence", async () => {
    chatStore.setDraft("what does it argue");
    await chatStore.send();

    chatStore.handleStreamEvent({
      conversation_id: "c1",
      request_id: mocks.chatSend.mock.calls[0][4] as string,
      kind: "error",
      text: "The model did not answer.",
    });
    await flush();

    expect(chatStore.errorMessage()).toBe("The model did not answer.");
    expect(chatStore.errorKind()).toBe("");
  });

  it("readiness reports an offline local server", async () => {
    mocks.groupOf.mockReturnValue("local");
    mocks.aiCheckConnection.mockResolvedValue(probe("refused", "localhost:11434"));

    await aiConnectionStore.check();

    expect(chatStore.readiness()).toEqual({
      state: "offline_local",
      message: "Nothing is answering at localhost:11434.",
      action: { kind: "check" },
    });
  });

  it("says nothing is wrong until something says so", () => {
    expect(chatStore.readiness()).toEqual({ state: "ready" });
  });

  it("reports a model the provider's own list does not hold", async () => {
    mocks.aiListModels.mockResolvedValue(catalog("ollama", ["llama3"]));
    aiConnectionStore.watch();
    await aiConnectionStore.refreshCatalog();

    expect(chatStore.readiness()).toMatchObject({ state: "model_unavailable" });
  });

  it("reports chat being switched off", () => {
    setConfig(ai("ollama", "qwen2.5", false));
    expect(chatStore.readiness()).toMatchObject({ state: "off" });
  });
});
