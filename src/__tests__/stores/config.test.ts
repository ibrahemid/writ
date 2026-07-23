import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

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
  editor: { font_family: "JetBrains Mono", font_size: 16, word_wrap: true, tab_size: 4, autosave_debounce_ms: 500, markdown_typography: true, markdown_editing: true },
  window: { width: 1200, height: 800 },
  keybindings: {},
  history: { max_entries: 1000 },
  storage: { path: "~/.writ" },
  theme: { preset: "warp-dark", overrides: {} },
  commands: { usage: {} },
  workspace: { root: null },
  inbox: { path: null, focus: true },
  updater: { auto_check: true },
  ai: { enabled: false, preset: "ollama", base_url: "http://localhost:11434/v1", model: "", consented_hosts: [] },
  spelling: { enabled: false, dialect: "american", ignored_words: [] },
  preview: {
    default_layout_html: "split",
    default_layout_markdown: "split",
    live_render_threshold_mb: 1,
    render_confirm_threshold_mb: 5,
    render_refuse_threshold_mb: 50,
    debounce_ms: 200,
    run_scripts: true,
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
    // recordCommandUse/pruneCommandUsage schedule a debounced flush on the
    // shared singleton; fake timers here keep those timers from leaking real
    // pending callbacks into the other test files.
    beforeEach(() => {
      vi.useFakeTimers();
    });
    afterEach(() => {
      vi.clearAllTimers();
      vi.useRealTimers();
    });

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

    it("debounces the usage flush so rapid records coalesce into one updateConfig", async () => {
      await configStore.save(MOCK_CONFIG);
      mockedUpdateConfig.mockClear();

      configStore.recordCommandUse("palette.open", 1_715_000_000_000);
      configStore.recordCommandUse("palette.open", 1_715_000_000_500);

      await vi.advanceTimersByTimeAsync(749);
      expect(mockedUpdateConfig).not.toHaveBeenCalled();

      await vi.advanceTimersByTimeAsync(1);
      expect(mockedUpdateConfig).toHaveBeenCalledTimes(1);
    });
  });

  describe("editor font size", () => {
    beforeEach(() => {
      vi.useFakeTimers();
    });
    afterEach(() => {
      vi.clearAllTimers();
      vi.useRealTimers();
    });

    it("applies a new font size optimistically to the live config", async () => {
      await configStore.save(MOCK_CONFIG);

      configStore.setEditorFontSize(22);

      expect(configStore.config().editor.font_size).toBe(22);
    });

    it("clamps below the minimum and above the maximum", async () => {
      await configStore.save(MOCK_CONFIG);

      configStore.setEditorFontSize(2);
      expect(configStore.config().editor.font_size).toBe(8);

      configStore.setEditorFontSize(999);
      expect(configStore.config().editor.font_size).toBe(72);
    });

    it("rounds fractional sizes and ignores non-finite input", async () => {
      await configStore.save(MOCK_CONFIG);

      configStore.setEditorFontSize(18.6);
      expect(configStore.config().editor.font_size).toBe(19);

      configStore.setEditorFontSize(Number.NaN);
      expect(configStore.config().editor.font_size).toBe(14);
    });

    it("debounces persistence so a fast zoom coalesces into one updateConfig", async () => {
      await configStore.save(MOCK_CONFIG);
      mockedUpdateConfig.mockClear();

      configStore.setEditorFontSize(17);
      configStore.setEditorFontSize(18);
      configStore.setEditorFontSize(19);

      await vi.advanceTimersByTimeAsync(749);
      expect(mockedUpdateConfig).not.toHaveBeenCalled();

      await vi.advanceTimersByTimeAsync(1);
      expect(mockedUpdateConfig).toHaveBeenCalledTimes(1);
      expect(mockedUpdateConfig.mock.calls[0][0].editor.font_size).toBe(19);
    });

    it("does not schedule a write when the size is unchanged", async () => {
      await configStore.save({
        ...MOCK_CONFIG,
        editor: { ...MOCK_CONFIG.editor, font_size: 20 },
      });
      mockedUpdateConfig.mockClear();

      configStore.setEditorFontSize(20);

      await vi.advanceTimersByTimeAsync(1000);
      expect(mockedUpdateConfig).not.toHaveBeenCalled();
    });
  });
});
