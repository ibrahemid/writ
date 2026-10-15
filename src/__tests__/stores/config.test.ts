import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

vi.mock("../../services/tauri", () => ({
  getConfig: vi.fn(),
  updateConfig: vi.fn().mockResolvedValue(undefined),
  recordCommandUses: vi.fn().mockResolvedValue(undefined),
}));

import {
  configStore,
  SIDEBAR_WIDTH_DEFAULT,
  SIDEBAR_WIDTH_MAX,
  SIDEBAR_WIDTH_MIN,
} from "../../stores/global/config";
import { commandUsageStore } from "../../stores/global/command-usage";
import { createEffect, createRoot } from "solid-js";
import { getConfig, recordCommandUses, updateConfig } from "../../services/tauri";
import type { WritConfig } from "../../types/config";

const mockedGetConfig = vi.mocked(getConfig);
const mockedUpdateConfig = vi.mocked(updateConfig);

const MOCK_CONFIG: WritConfig = {
  hotkey: { toggle: "CmdOrCtrl+Shift+Space" },
  sidebar: { toggle: "CmdOrCtrl+\\", default_visible: false, position: "left", open: false, width: 240, collapsed: [], hidden: [] },
  panel: { open: false, width: 240 },
  chat_panel: { open: false, width: 380 },
  first_run: { hint_dismissed: false },
  editor: { font_size: 18, word_wrap: true, tab_size: 4, autosave_debounce_ms: 500, markdown_editing: true, status_bar: false, status_bar_counts: false, reading_width: false },
  window: { width: 1200, height: 800, maximized: false },
  keybindings: {},
  history: { max_entries: 1000 },
  theme: { preset: "warp-dark", overrides: {} },
  appearance: { polarity: "system", accent: "pine", prose_face: "system", interface_text_size: null },
  files: { default_extension: "txt" },
  workspace: { root: null },
  inbox: { path: null, focus: true },
  updater: { auto_check: true },
  ai: { provider: "ollama", base_url: "", model: "", consented_hosts: [], rewrite: { enabled: false }, chat: { enabled: false, model: "", model_provider: "" } },
  mcp: { enabled: false, approved_clients: [] },
  apps: { connections: false, graph: false, tags: false, offers: true, offered: [], opened: [] },
  spelling: { enabled: false, dialect: "american", ignored_words: [] },
  preview: {
    default_layout_html: "split",
    default_layout_markdown: "inline",
    live_render_threshold_mb: 1,
    render_confirm_threshold_mb: 5,
    render_refuse_threshold_mb: 50,
    debounce_ms: 200,
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
      expect(configStore.config().editor.font_size).toBe(18);
      expect(configStore.config().editor.tab_size).toBe(4);
    });

    // A config file written before Writ had a format to ask about carries no
    // [files] table; the answer it never gave is the one a first launch
    // starts on.
    it("fills the format in for a config that predates the question", async () => {
      const older: Partial<WritConfig> = { ...MOCK_CONFIG };
      delete older.files;
      mockedGetConfig.mockResolvedValueOnce(older as WritConfig);

      await configStore.load();

      expect(configStore.config().files.default_extension).toBe("txt");
    });

    it("keeps the format the config names", async () => {
      mockedGetConfig.mockResolvedValueOnce({
        ...MOCK_CONFIG,
        files: { default_extension: "md" },
      });

      await configStore.load();

      expect(configStore.config().files.default_extension).toBe("md");
    });

    it("resets to defaults on load failure", async () => {
      mockedGetConfig.mockResolvedValueOnce(MOCK_CONFIG);
      await configStore.load();
      expect(configStore.config().editor.font_size).toBe(18);

      mockedGetConfig.mockRejectedValueOnce(new Error("no file"));
      const consoleSpy = vi.spyOn(console, "error").mockImplementation(() => {});

      await configStore.load();

      const resetConfig = configStore.config();
      expect(resetConfig.editor.font_size).not.toBe(18);
      expect(resetConfig.hotkey.toggle).toBeTruthy();
      expect(resetConfig.editor.autosave_debounce_ms).toBeGreaterThan(0);
      // Matches default_status_bar in writ-core: the fallback config and the
      // one the backend writes have to agree on what a first launch shows.
      expect(resetConfig.editor.status_bar).toBe(true);
      expect(resetConfig.editor.status_bar_counts).toBe(false);
      consoleSpy.mockRestore();
    });
  });

  describe("save", () => {
    it("persists config to backend and updates local state", async () => {
      await configStore.save(MOCK_CONFIG);

      expect(mockedUpdateConfig).toHaveBeenCalledOnce();
      expect(mockedUpdateConfig).toHaveBeenCalledWith(MOCK_CONFIG);
      expect(configStore.config().editor.font_size).toBe(18);
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
      consoleSpy.mockRestore();
    });
  });

  // F6: the config is read field by field, so a write reruns the readers of
  // the fields it changed and nothing else.
  describe("fine-grained reads", () => {
    beforeEach(() => {
      vi.useFakeTimers();
    });
    afterEach(() => {
      vi.clearAllTimers();
      vi.useRealTimers();
    });

    /** Counts the runs of an effect reading `read`, after its first. */
    function countReruns(read: () => unknown): { reruns: () => number; dispose: () => void } {
      let runs = 0;
      const dispose = createRoot((done) => {
        createEffect(() => {
          read();
          runs += 1;
        });
        return done;
      });
      return { reruns: () => runs - 1, dispose };
    }

    it("running a command reruns no config reader and writes no config", async () => {
      await configStore.save(MOCK_CONFIG);
      mockedUpdateConfig.mockClear();
      const fontReader = countReruns(() => configStore.config().editor.font_size);
      const aiReader = countReruns(() => configStore.config().ai.provider);

      for (let i = 0; i < 5; i += 1) commandUsageStore.record("palette.open", 1_000 + i);
      await vi.advanceTimersByTimeAsync(1000);

      expect(fontReader.reruns()).toBe(0);
      expect(aiReader.reruns()).toBe(0);
      expect(mockedUpdateConfig).not.toHaveBeenCalled();
      expect(vi.mocked(recordCommandUses)).toHaveBeenCalledTimes(1);
      fontReader.dispose();
      aiReader.dispose();
    });

    it("a section setter reruns the readers of that field alone", async () => {
      await configStore.save(MOCK_CONFIG);
      const fontReader = countReruns(() => configStore.config().editor.font_size);
      const widthReader = countReruns(() => configStore.config().sidebar.width);

      configStore.setSidebarWidth(300);
      configStore.setSidebarSectionCollapsed("tags", true);

      expect(widthReader.reruns()).toBe(1);
      expect(fontReader.reruns()).toBe(0);
      fontReader.dispose();
      widthReader.dispose();
    });

    it("a reload reruns only the readers of the values that changed", async () => {
      mockedGetConfig.mockResolvedValueOnce(MOCK_CONFIG);
      await configStore.load();
      const fontReader = countReruns(() => configStore.config().editor.font_size);
      const hostsReader = countReruns(() => configStore.config().ai.consented_hosts.join("\n"));

      mockedGetConfig.mockResolvedValueOnce({
        ...MOCK_CONFIG,
        sidebar: { ...MOCK_CONFIG.sidebar, width: 288 },
      });
      await configStore.load();
      expect(fontReader.reruns()).toBe(0);
      expect(hostsReader.reruns()).toBe(0);

      mockedGetConfig.mockResolvedValueOnce({
        ...MOCK_CONFIG,
        editor: { ...MOCK_CONFIG.editor, font_size: 21 },
        ai: { ...MOCK_CONFIG.ai, consented_hosts: ["api.example.com"] },
      });
      await configStore.load();
      expect(fontReader.reruns()).toBe(1);
      expect(hostsReader.reruns()).toBe(1);
      fontReader.dispose();
      hostsReader.dispose();
    });

    it("never hands the store's own objects to a write", async () => {
      await configStore.save(MOCK_CONFIG);
      const sent = mockedUpdateConfig.mock.calls[mockedUpdateConfig.mock.calls.length - 1][0];

      configStore.setEditorFontSize(30);

      expect(sent.editor.font_size).toBe(18);
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
      expect(configStore.config().editor.font_size).toBe(16);
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

  describe("sidebar bounds", () => {
    // One source of truth: the drag handle clamps to the same numbers the
    // stylesheet lays the panel out with.
    it("are the token values, not a second copy of them", () => {
      // Read out of the generated file as text: comparing the constants to the
      // same import they are computed from asserts nothing. The three numbers
      // are the baseline's Sidebar width row, 240 resizable 200 to 320.
      const generated = readFileSync(
        resolve(process.cwd(), "src/styles/generated/tokens.ts"),
        "utf8",
      );
      const block = generated.slice(generated.indexOf("export const SIDEBAR"));
      const px = (key: string): number => {
        const found = new RegExp(`^\\s*${key}:\\s*"(\\d+(?:\\.\\d+)?)px"`, "m").exec(block);
        expect(found, `SIDEBAR.${key} is stated in px`).not.toBeNull();
        return Number(found![1]);
      };
      expect([px("minWidth"), px("maxWidth"), px("width")]).toEqual([200, 320, 240]);
      expect(SIDEBAR_WIDTH_MIN).toBe(px("minWidth"));
      expect(SIDEBAR_WIDTH_MAX).toBe(px("maxWidth"));
      expect(SIDEBAR_WIDTH_DEFAULT).toBe(px("width"));
    });

    it("are read from the token module rather than written out", () => {
      const source = readFileSync(resolve(process.cwd(), "src/stores/global/config.ts"), "utf8");
      const bounds = source.slice(source.indexOf("export const SIDEBAR_WIDTH_MIN"));
      expect(bounds.slice(0, bounds.indexOf("export function clampSidebarWidth"))).not.toMatch(
        /\b(200|320|240)\b/,
      );
    });
  });

  describe("sidebar width", () => {
    beforeEach(() => {
      vi.useFakeTimers();
    });
    afterEach(() => {
      vi.clearAllTimers();
      vi.useRealTimers();
    });

    it("defaults to 240 when the stored config predates the setting", async () => {
      const legacy = { ...MOCK_CONFIG, sidebar: { ...MOCK_CONFIG.sidebar } } as WritConfig;
      delete (legacy.sidebar as Partial<WritConfig["sidebar"]>).width;
      mockedGetConfig.mockResolvedValueOnce(legacy);

      await configStore.load();

      expect(configStore.config().sidebar.width).toBe(240);
    });

    it("clamps below the minimum and above the maximum", async () => {
      await configStore.save(MOCK_CONFIG);

      configStore.setSidebarWidth(120);
      expect(configStore.config().sidebar.width).toBe(200);

      configStore.setSidebarWidth(900);
      expect(configStore.config().sidebar.width).toBe(320);
    });

    it("persists the settled width through the store", async () => {
      await configStore.save(MOCK_CONFIG);
      mockedUpdateConfig.mockClear();

      configStore.setSidebarWidth(288);
      expect(configStore.config().sidebar.width).toBe(288);

      await vi.advanceTimersByTimeAsync(750);
      expect(mockedUpdateConfig).toHaveBeenCalledTimes(1);
      expect(mockedUpdateConfig.mock.calls[0][0].sidebar.width).toBe(288);
    });

    it("does not schedule a write when the width is unchanged", async () => {
      await configStore.save({
        ...MOCK_CONFIG,
        sidebar: { ...MOCK_CONFIG.sidebar, width: 260 },
      });
      mockedUpdateConfig.mockClear();

      configStore.setSidebarWidth(260);

      await vi.advanceTimersByTimeAsync(1000);
      expect(mockedUpdateConfig).not.toHaveBeenCalled();
    });
  });

  describe("sidebar sections", () => {
    beforeEach(() => {
      vi.useFakeTimers();
    });
    afterEach(() => {
      vi.clearAllTimers();
      vi.useRealTimers();
    });

    it("reads no folded or hidden sections from a config that predates them", async () => {
      const legacy = { ...MOCK_CONFIG, sidebar: { ...MOCK_CONFIG.sidebar } } as WritConfig;
      delete (legacy.sidebar as Partial<WritConfig["sidebar"]>).collapsed;
      delete (legacy.sidebar as Partial<WritConfig["sidebar"]>).hidden;
      mockedGetConfig.mockResolvedValueOnce(legacy);

      await configStore.load();

      expect(configStore.config().sidebar.collapsed).toEqual([]);
      expect(configStore.config().sidebar.hidden).toEqual([]);
      expect(configStore.isSidebarSectionCollapsed("tags")).toBe(false);
      expect(configStore.isSidebarSectionHidden("tags")).toBe(false);
    });

    it("keeps only the sections it knows from a hand-edited file", async () => {
      mockedGetConfig.mockResolvedValueOnce({
        ...MOCK_CONFIG,
        sidebar: {
          ...MOCK_CONFIG.sidebar,
          collapsed: ["open", "recent", "recent"],
          hidden: ["tags", 4],
        } as unknown as WritConfig["sidebar"],
      });

      await configStore.load();

      expect(configStore.config().sidebar.collapsed).toEqual(["recent"]);
      expect(configStore.config().sidebar.hidden).toEqual(["tags"]);
    });

    it("persists a fold once per flip and restores it on the next read", async () => {
      await configStore.save(MOCK_CONFIG);
      mockedUpdateConfig.mockClear();

      configStore.setSidebarSectionCollapsed("recent", true);
      expect(configStore.isSidebarSectionCollapsed("recent")).toBe(true);
      configStore.setSidebarSectionCollapsed("folder", true);

      await vi.advanceTimersByTimeAsync(750);
      expect(mockedUpdateConfig).toHaveBeenCalledTimes(1);
      const written = mockedUpdateConfig.mock.calls[0][0];
      expect(written.sidebar.collapsed).toEqual(["folder", "recent"]);

      mockedGetConfig.mockResolvedValueOnce(written);
      await configStore.load();
      expect(configStore.isSidebarSectionCollapsed("folder")).toBe(true);
      expect(configStore.isSidebarSectionCollapsed("recent")).toBe(true);
      expect(configStore.isSidebarSectionCollapsed("tags")).toBe(false);
    });

    it("unfolds a section and writes the change", async () => {
      await configStore.save({
        ...MOCK_CONFIG,
        sidebar: { ...MOCK_CONFIG.sidebar, collapsed: ["tags", "recent"] },
      });
      mockedUpdateConfig.mockClear();

      configStore.setSidebarSectionCollapsed("tags", false);

      expect(configStore.config().sidebar.collapsed).toEqual(["recent"]);
      await vi.advanceTimersByTimeAsync(750);
      expect(mockedUpdateConfig.mock.calls[0][0].sidebar.collapsed).toEqual(["recent"]);
    });

    it("hides and shows a section through the same write", async () => {
      await configStore.save(MOCK_CONFIG);
      mockedUpdateConfig.mockClear();

      configStore.setSidebarSectionHidden("inbox", true);
      expect(configStore.isSidebarSectionHidden("inbox")).toBe(true);
      expect(configStore.isSidebarSectionCollapsed("inbox")).toBe(false);

      await vi.advanceTimersByTimeAsync(750);
      expect(mockedUpdateConfig.mock.calls[0][0].sidebar.hidden).toEqual(["inbox"]);

      configStore.setSidebarSectionHidden("inbox", false);
      expect(configStore.config().sidebar.hidden).toEqual([]);
    });

    it("does not schedule a write when the state is unchanged", async () => {
      await configStore.save(MOCK_CONFIG);
      mockedUpdateConfig.mockClear();

      configStore.setSidebarSectionCollapsed("tags", false);
      configStore.setSidebarSectionHidden("tags", false);

      await vi.advanceTimersByTimeAsync(1000);
      expect(mockedUpdateConfig).not.toHaveBeenCalled();
    });
  });
  describe("app offers and first switch-ons", () => {
    it("reads offers as allowed and none made from a file that predates them", async () => {
      const older = { ...MOCK_CONFIG, apps: { connections: true, graph: false, tags: true } };
      mockedGetConfig.mockResolvedValueOnce(older as WritConfig);
      await configStore.load();
      expect(configStore.appOffersOn()).toBe(true);
      expect(configStore.wasAppOffered("tags")).toBe(false);
      expect(configStore.wasAppOpened("graph")).toBe(false);
    });

    it("keeps the apps it names once each and drops a word that names no app", async () => {
      mockedGetConfig.mockResolvedValueOnce({
        ...MOCK_CONFIG,
        apps: {
          ...MOCK_CONFIG.apps,
          offered: ["tags", "nonsense", "tags"],
          opened: ["graph"],
        },
      } as unknown as WritConfig);
      await configStore.load();
      expect(configStore.config().apps.offered).toEqual(["tags"]);
      expect(configStore.config().apps.opened).toEqual(["graph"]);
    });

    it("writes an offer, the switch and an opened app at once", async () => {
      await configStore.save(MOCK_CONFIG);
      mockedUpdateConfig.mockClear();

      configStore.noteAppOffered("connections");
      expect(mockedUpdateConfig.mock.lastCall![0].apps.offered).toEqual(["connections"]);
      configStore.setAppOffersOn(false);
      expect(mockedUpdateConfig.mock.lastCall![0].apps.offers).toBe(false);
      configStore.noteAppsOpened(["graph", "graph"]);
      expect(mockedUpdateConfig.mock.lastCall![0].apps.opened).toEqual(["graph"]);
      expect(mockedUpdateConfig).toHaveBeenCalledTimes(3);

      configStore.noteAppOffered("connections");
      expect(mockedUpdateConfig).toHaveBeenCalledTimes(3);
    });
  });

  describe("noteFirstRunHintDismissed", () => {
    beforeEach(() => vi.useFakeTimers());
    afterEach(() => vi.useRealTimers());

    it("carries the dismissal into the next whole-config write", async () => {
      await configStore.save({
        ...MOCK_CONFIG,
        first_run: { hint_dismissed: false },
      });
      mockedUpdateConfig.mockClear();

      configStore.noteFirstRunHintDismissed();
      configStore.setSidebarWidth(292);
      await vi.advanceTimersByTimeAsync(750);

      expect(mockedUpdateConfig.mock.calls[0][0].first_run.hint_dismissed).toBe(true);
    });
  });
});
