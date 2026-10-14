import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

const h = vi.hoisted(() => ({
  aiListModels: vi.fn(),
  getConfig: vi.fn(),
  updateConfig: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  aiListModels: (...a: unknown[]) => h.aiListModels(...a),
  aiCheckConnection: vi.fn(),
  aiConsentHost: vi.fn(),
  aiProbeLocal: vi.fn(),
  aiOpenrouterCancel: vi.fn(),
  aiOpenrouterConnect: vi.fn(),
  aiSetProvider: vi.fn(),
  aiProviders: vi.fn().mockResolvedValue([]),
  getConfig: (...a: unknown[]) => h.getConfig(...a),
  updateConfig: (...a: unknown[]) => h.updateConfig(...a),
}));

import { configStore } from "../../stores/global/config";
import { aiConnectionStore } from "../../stores/global/ai-connection";
import type { WritConfig } from "../../types/config";

/** Lets the effects and the requests they start settle. */
async function settle(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0));
}

// AM-09: the model list follows the connection and nothing else. With a
// hosted provider consented to, a settings write that is not about the
// connection must not send the key-carrying request to the provider.
describe("the model list and the rest of the settings", () => {
  beforeEach(async () => {
    h.aiListModels.mockReset();
    h.aiListModels.mockResolvedValue({
      provider: "openrouter",
      models: ["a"],
      source: "live",
      error: null,
    });
    h.updateConfig.mockReset();
    h.updateConfig.mockResolvedValue(undefined);
    await configStore.save({
      ...configStore.config(),
      ai: {
        ...configStore.config().ai,
        provider: "openrouter",
        base_url: "",
        consented_hosts: ["openrouter.ai"],
        chat: { enabled: true, model: "", model_provider: "" },
      },
    });
    aiConnectionStore.watch();
    await settle();
    h.aiListModels.mockClear();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("a non-AI write sends no model-list request", async () => {
    configStore.setSidebarSectionCollapsed("tags", !configStore.isSidebarSectionCollapsed("tags"));
    configStore.setSidebarWidth(configStore.config().sidebar.width === 300 ? 280 : 300);
    configStore.setPanelOpen(!configStore.config().panel.open);
    await configStore.save({
      ...configStore.config(),
      editor: { ...configStore.config().editor, word_wrap: !configStore.config().editor.word_wrap },
    });
    await settle();

    expect(h.aiListModels).not.toHaveBeenCalled();
  });

  it("a reload of a file whose connection did not change sends none", async () => {
    const onDisk: WritConfig = JSON.parse(JSON.stringify(configStore.config()));
    onDisk.sidebar.width = onDisk.sidebar.width === 260 ? 270 : 260;
    h.getConfig.mockResolvedValueOnce(onDisk);

    await configStore.load();
    await settle();

    expect(h.aiListModels).not.toHaveBeenCalled();
  });

  it("a change to the connection reads the list again", async () => {
    await configStore.save({
      ...configStore.config(),
      ai: { ...configStore.config().ai, base_url: "https://openrouter.ai/api/v2" },
    });
    await settle();

    expect(h.aiListModels).toHaveBeenCalledTimes(1);
  });
});
