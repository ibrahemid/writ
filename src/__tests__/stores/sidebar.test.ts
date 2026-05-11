import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  searchBuffers: vi.fn().mockResolvedValue([]),
  getConfig: vi.fn(),
  updateConfig: vi.fn().mockResolvedValue(undefined),
}));

import { sidebarStore } from "../../stores/sidebar";
import { configStore } from "../../stores/config";
import { searchBuffers, getConfig, updateConfig } from "../../services/tauri";
import type { WritConfig } from "../../types/config";

const mockedSearch = vi.mocked(searchBuffers);
const mockedGetConfig = vi.mocked(getConfig);
const mockedUpdateConfig = vi.mocked(updateConfig);

function buildConfig(overrides: Partial<WritConfig["sidebar"]> = {}): WritConfig {
  return {
    hotkey: { toggle: "CmdOrCtrl+Shift+Space" },
    sidebar: {
      toggle: "CmdOrCtrl+S",
      default_visible: false,
      position: "left",
      open: false,
      ...overrides,
    },
    editor: { font_family: "monospace", font_size: 14, word_wrap: true, tab_size: 2, autosave_debounce_ms: 300 },
    window: { width: 800, height: 600 },
    keybindings: {},
    history: { max_entries: 500 },
    storage: { path: "~/.writ" },
  };
}

describe("sidebarStore", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    sidebarStore.hide();
    sidebarStore.setSearchQuery("");
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  describe("visibility", () => {
    it("starts hidden", () => {
      expect(sidebarStore.isOpen()).toBe(false);
    });

    it("show makes it open", () => {
      sidebarStore.show();
      expect(sidebarStore.isOpen()).toBe(true);
    });

    it("hide closes it", () => {
      sidebarStore.show();
      sidebarStore.hide();
      expect(sidebarStore.isOpen()).toBe(false);
    });

    it("toggle flips open state", () => {
      sidebarStore.toggle();
      expect(sidebarStore.isOpen()).toBe(true);

      sidebarStore.toggle();
      expect(sidebarStore.isOpen()).toBe(false);
    });
  });

  describe("persistence", () => {
    it("persists open state to configStore on toggle", async () => {
      mockedGetConfig.mockResolvedValueOnce(buildConfig({ open: false }));
      await configStore.load();

      sidebarStore.toggle();
      await vi.runAllTimersAsync();

      const calls = mockedUpdateConfig.mock.calls;
      const saved = calls[calls.length - 1]?.[0] as WritConfig;
      expect(saved.sidebar.open).toBe(true);
    });

    it("hydrates open from configStore", async () => {
      mockedGetConfig.mockResolvedValueOnce(buildConfig({ open: true }));
      await configStore.load();
      sidebarStore.hydrateFromConfig();

      expect(sidebarStore.isOpen()).toBe(true);
    });
  });

  describe("search", () => {
    it("updates search query", () => {
      sidebarStore.setSearchQuery("hello");
      expect(sidebarStore.searchQuery()).toBe("hello");
    });

    it("clears search results when query is empty", () => {
      sidebarStore.setSearchQuery("");
      expect(sidebarStore.searchResultIds()).toEqual([]);
      expect(mockedSearch).not.toHaveBeenCalled();
    });

    it("clears search results when query is whitespace", () => {
      sidebarStore.setSearchQuery("   ");
      expect(sidebarStore.searchResultIds()).toEqual([]);
      expect(mockedSearch).not.toHaveBeenCalled();
    });

    it("debounces search calls", async () => {
      mockedSearch.mockResolvedValueOnce(["id-1", "id-2"]);

      sidebarStore.setSearchQuery("test");

      expect(mockedSearch).not.toHaveBeenCalled();

      await vi.advanceTimersByTimeAsync(200);

      expect(mockedSearch).toHaveBeenCalledOnce();
      expect(mockedSearch).toHaveBeenCalledWith("test");
      expect(sidebarStore.searchResultIds()).toEqual(["id-1", "id-2"]);
    });

    it("resets results on search failure", async () => {
      mockedSearch.mockRejectedValueOnce(new Error("db error"));

      sidebarStore.setSearchQuery("fail");
      await vi.advanceTimersByTimeAsync(200);

      expect(sidebarStore.searchResultIds()).toEqual([]);
    });

    it("cancels previous search when query changes rapidly", async () => {
      sidebarStore.setSearchQuery("fir");
      await vi.advanceTimersByTimeAsync(100);

      mockedSearch.mockResolvedValueOnce(["final-result"]);
      sidebarStore.setSearchQuery("final");
      await vi.advanceTimersByTimeAsync(200);

      expect(mockedSearch).toHaveBeenCalledOnce();
      expect(mockedSearch).toHaveBeenCalledWith("final");
    });
  });
});
