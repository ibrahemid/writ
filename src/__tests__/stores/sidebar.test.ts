import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  searchBuffers: vi.fn().mockResolvedValue({ hits: [], total: 0 }),
  getConfig: vi.fn(),
  updateConfig: vi.fn().mockResolvedValue(undefined),
}));

function hit(id: string) {
  return { buffer_id: id, title: id, line: 1, snippet: [{ text: id, matched: true }] };
}

vi.mock("../../services/autosave", () => ({
  flushAutosave: vi.fn().mockResolvedValue(undefined),
}));

import { createSidebarStore, type SidebarStore } from "../../stores/window/sidebar-store";
import { configStore } from "../../stores/global/config";
import { searchBuffers, getConfig, updateConfig } from "../../services/tauri";
import { flushAutosave } from "../../services/autosave";
import type { WritConfig } from "../../types/config";

const mockedSearch = vi.mocked(searchBuffers);
const mockedFlush = vi.mocked(flushAutosave);
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
    editor: { font_family: "monospace", font_size: 14, word_wrap: true, tab_size: 2, autosave_debounce_ms: 300, markdown_typography: true, markdown_editing: true },
    window: { width: 800, height: 600 },
    keybindings: {},
    history: { max_entries: 500 },
    storage: { path: "~/.writ" },
    theme: { preset: "warp-dark", overrides: {} },
    commands: { usage: {} },
  workspace: { root: null },
  inbox: { path: null, focus: true },
  updater: { auto_check: true },
  ai: { enabled: false, preset: "ollama", base_url: "http://localhost:11434/v1", model: "", consented_hosted: false },
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
}

describe("sidebar-store (per-window factory)", () => {
  let sidebarStore: SidebarStore;

  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    sidebarStore = createSidebarStore();
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
      expect(sidebarStore.searchHits()).toEqual([]);
      expect(sidebarStore.searchTotal()).toBe(0);
      expect(sidebarStore.searchMs()).toBeNull();
      expect(mockedSearch).not.toHaveBeenCalled();
    });

    it("clears search results when query is whitespace", () => {
      sidebarStore.setSearchQuery("   ");
      expect(sidebarStore.searchHits()).toEqual([]);
      expect(mockedSearch).not.toHaveBeenCalled();
    });

    it("debounces search calls and stores hits, total, and timing", async () => {
      mockedSearch.mockResolvedValueOnce({ hits: [hit("id-1"), hit("id-2")], total: 7 });

      sidebarStore.setSearchQuery("test");

      expect(mockedSearch).not.toHaveBeenCalled();

      await vi.advanceTimersByTimeAsync(200);

      expect(mockedSearch).toHaveBeenCalledOnce();
      expect(mockedSearch).toHaveBeenCalledWith("test");
      expect(sidebarStore.searchHits().map((h) => h.buffer_id)).toEqual(["id-1", "id-2"]);
      expect(sidebarStore.searchTotal()).toBe(7);
      expect(sidebarStore.searchMs()).not.toBeNull();
    });

    it("resets results on search failure", async () => {
      mockedSearch.mockRejectedValueOnce(new Error("db error"));

      sidebarStore.setSearchQuery("fail");
      await vi.advanceTimersByTimeAsync(200);

      expect(sidebarStore.searchHits()).toEqual([]);
      expect(sidebarStore.searchTotal()).toBe(0);
      expect(sidebarStore.searchMs()).toBeNull();
    });

    it("cancels previous search when query changes rapidly", async () => {
      sidebarStore.setSearchQuery("fir");
      await vi.advanceTimersByTimeAsync(100);

      mockedSearch.mockResolvedValueOnce({ hits: [hit("final-result")], total: 1 });
      sidebarStore.setSearchQuery("final");
      await vi.advanceTimersByTimeAsync(200);

      expect(mockedSearch).toHaveBeenCalledOnce();
      expect(mockedSearch).toHaveBeenCalledWith("final");
    });

    it("ignores a stale resolved response after the query changed", async () => {
      let resolveFirst: (v: { hits: never[]; total: number }) => void = () => {};
      mockedSearch.mockReturnValueOnce(
        new Promise((r) => (resolveFirst = r)) as ReturnType<typeof mockedSearch>,
      );
      sidebarStore.setSearchQuery("stale");
      await vi.advanceTimersByTimeAsync(200);

      // A newer query supersedes the in-flight one before it resolves.
      mockedSearch.mockResolvedValueOnce({ hits: [hit("fresh")], total: 1 });
      sidebarStore.setSearchQuery("fresh");
      await vi.advanceTimersByTimeAsync(200);

      resolveFirst({ hits: [], total: 99 });
      await Promise.resolve();

      expect(sidebarStore.searchHits().map((h) => h.buffer_id)).toEqual(["fresh"]);
      expect(sidebarStore.searchTotal()).toBe(1);
    });

    it("flushes pending autosaves before issuing the search", async () => {
      const order: string[] = [];
      mockedFlush.mockImplementationOnce(async () => {
        order.push("flush");
      });
      mockedSearch.mockImplementationOnce(async () => {
        order.push("search");
        return { hits: [], total: 0 };
      });

      sidebarStore.setSearchQuery("foobar");
      await vi.advanceTimersByTimeAsync(200);

      expect(mockedFlush).toHaveBeenCalledOnce();
      expect(order).toEqual(["flush", "search"]);
    });
  });

  describe("per-window isolation", () => {
    it("two instances are independent", () => {
      const a = createSidebarStore();
      const b = createSidebarStore();
      a.show();
      expect(a.isOpen()).toBe(true);
      expect(b.isOpen()).toBe(false);
    });
  });
});
