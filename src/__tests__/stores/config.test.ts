import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  getConfig: vi.fn(),
  updateConfig: vi.fn().mockResolvedValue(undefined),
}));

import { configStore } from "../../stores/global/config";
import { getConfig, updateConfig } from "../../services/tauri";
import type { WritConfig } from "../../types/config";

const mockedGetConfig = vi.mocked(getConfig);
const mockedUpdateConfig = vi.mocked(updateConfig);

const MOCK_CONFIG: WritConfig = {
  hotkey: { toggle: "CmdOrCtrl+Shift+Space" },
  sidebar: { toggle: "CmdOrCtrl+S", default_visible: false, position: "left", open: false },
  editor: { font_family: "JetBrains Mono", font_size: 16, word_wrap: true, tab_size: 4, autosave_debounce_ms: 500 },
  window: { width: 1200, height: 800 },
  keybindings: {},
  history: { max_entries: 1000 },
  storage: { path: "~/.writ" },
  theme: { preset: "warp-dark", overrides: {} },
  commands: { usage: {} },
  preview: {
    default_layout_html: "split",
    default_layout_markdown: "split",
    default_layout_pdf: "preview",
    default_layout_image: "preview",
    default_layout_svg: "preview",
    live_render_threshold_mb: 1,
    render_confirm_threshold_mb: 5,
    render_refuse_threshold_mb: 50,
    debounce_ms: 200,
    detach_on_open: false,
  },
};

describe("configStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe("load", () => {
    it("loads config from backend", async () => {
      mockedGetConfig.mockResolvedValueOnce(MOCK_CONFIG);

      await configStore.load();

      expect(mockedGetConfig).toHaveBeenCalledOnce();
      expect(configStore.config().editor.font_size).toBe(16);
      expect(configStore.config().editor.tab_size).toBe(4);
    });

    it("resets to defaults on load failure", async () => {
      mockedGetConfig.mockResolvedValueOnce(MOCK_CONFIG);
      await configStore.load();
      expect(configStore.config().editor.font_size).toBe(16);

      mockedGetConfig.mockRejectedValueOnce(new Error("no file"));
      const consoleSpy = vi.spyOn(console, "error").mockImplementation(() => {});

      await configStore.load();

      const resetConfig = configStore.config();
      expect(resetConfig.editor.font_size).not.toBe(16);
      expect(resetConfig.hotkey.toggle).toBeTruthy();
      expect(resetConfig.editor.autosave_debounce_ms).toBeGreaterThan(0);
      consoleSpy.mockRestore();
    });
  });

  describe("save", () => {
    it("persists config to backend and updates local state", async () => {
      await configStore.save(MOCK_CONFIG);

      expect(mockedUpdateConfig).toHaveBeenCalledOnce();
      expect(mockedUpdateConfig).toHaveBeenCalledWith(MOCK_CONFIG);
      expect(configStore.config().editor.font_size).toBe(16);
    });

    it("propagates save errors", async () => {
      mockedUpdateConfig.mockRejectedValueOnce(new Error("write failed"));

      await expect(configStore.save(MOCK_CONFIG)).rejects.toThrow("write failed");
    });
  });

  describe("defaults", () => {
    it("restores defaults on fresh load failure", async () => {
      mockedGetConfig.mockRejectedValueOnce(new Error("no config"));
      const consoleSpy = vi.spyOn(console, "error").mockImplementation(() => {});

      await configStore.load();

      const config = configStore.config();
      expect(config.hotkey.toggle).toBeTruthy();
      expect(config.editor.autosave_debounce_ms).toBeGreaterThan(0);
      expect(config.history.max_entries).toBeGreaterThan(0);
      expect(config.commands.usage).toEqual({});
      consoleSpy.mockRestore();
    });

    it("normalizes a config that is missing the commands section", async () => {
      const partial = { ...MOCK_CONFIG } as Partial<WritConfig> as WritConfig;
      delete (partial as { commands?: unknown }).commands;
      mockedGetConfig.mockResolvedValueOnce(partial);

      await configStore.load();

      expect(configStore.config().commands.usage).toEqual({});
    });
  });

  describe("command usage tracking", () => {
    it("increments count and stamps last_used_ms on recordCommandUse", async () => {
      await configStore.save(MOCK_CONFIG);

      configStore.recordCommandUse("palette.open", 1_715_000_000_000);
      configStore.recordCommandUse("palette.open", 1_715_000_001_000);

      const entry = configStore.config().commands.usage["palette.open"];
      expect(entry.count).toBe(2);
      expect(entry.last_used_ms).toBe(1_715_000_001_000);
    });

    it("clears all usage entries via clearCommandUsage", async () => {
      await configStore.save({
        ...MOCK_CONFIG,
        commands: { usage: { "x.y": { count: 4, last_used_ms: 100 } } },
      });

      await configStore.clearCommandUsage();

      expect(configStore.config().commands.usage).toEqual({});
      expect(mockedUpdateConfig).toHaveBeenCalled();
    });

    it("prunes usage entries whose command id is unknown", async () => {
      await configStore.save({
        ...MOCK_CONFIG,
        commands: {
          usage: {
            "live.cmd": { count: 1, last_used_ms: 1 },
            "removed.cmd": { count: 5, last_used_ms: 2 },
          },
        },
      });

      configStore.pruneCommandUsage(new Set(["live.cmd"]));

      expect(configStore.config().commands.usage).toEqual({
        "live.cmd": { count: 1, last_used_ms: 1 },
      });
    });
  });
});
