import { describe, it, expect, vi, beforeEach } from "vitest";
import type { BufferDocument } from "../../types/buffer";

let tabIdCounter = 0;

function mockBuffer(overrides: Partial<BufferDocument> = {}): BufferDocument {
  tabIdCounter++;
  return {
    id: overrides.id ?? `buf-${tabIdCounter}`,
    title: overrides.title ?? `Buffer ${tabIdCounter}`,
    filename: overrides.filename ?? `buf-${tabIdCounter}.md`,
    status: overrides.status ?? "active",
    language: overrides.language ?? null,
    source_path: overrides.source_path ?? null,
    cursor_pos: overrides.cursor_pos ?? 0,
    scroll_pos: overrides.scroll_pos ?? 0,
    tab_order: overrides.tab_order ?? tabIdCounter,
    created_at: overrides.created_at ?? new Date().toISOString(),
    updated_at: overrides.updated_at ?? new Date().toISOString(),
    closed_at: overrides.closed_at ?? null,
  };
}

vi.mock("../../services/tauri", () => ({
  createBuffer: vi.fn().mockImplementation(() => Promise.resolve(mockBuffer())),
  listActiveBuffers: vi.fn().mockResolvedValue([]),
  listHistory: vi.fn().mockResolvedValue([]),
  closeBuffer: vi.fn().mockResolvedValue(undefined),
  deleteBuffer: vi.fn().mockResolvedValue(undefined),
  restoreBuffer: vi.fn().mockResolvedValue(undefined),
  clearHistory: vi.fn().mockResolvedValue(undefined),
  renameBuffer: vi.fn().mockResolvedValue(undefined),
}));

import { bufferStore } from "../../stores/buffers";
import * as api from "../../services/tauri";

const mockedApi = vi.mocked(api);

describe("bufferStore operations", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    tabIdCounter = 0;
    mockedApi.listActiveBuffers.mockResolvedValue([]);
    mockedApi.listHistory.mockResolvedValue([]);
    await bufferStore.load();
  });

  describe("createTab", () => {
    it("adds new tab to active tabs", async () => {
      const doc = await bufferStore.createTab("Notes");

      expect(mockedApi.createBuffer).toHaveBeenCalledWith("Notes");
      expect(bufferStore.activeTabs()).toContainEqual(doc);
    });

    it("sets new tab as active", async () => {
      const doc = await bufferStore.createTab();

      expect(bufferStore.activeTabId()).toBe(doc.id);
    });
  });

  describe("closeTab", () => {
    it("removes tab from active list", async () => {
      const doc = await bufferStore.createTab();

      await bufferStore.closeTab(doc.id);

      expect(mockedApi.closeBuffer).toHaveBeenCalledWith(doc.id);
      expect(bufferStore.activeTabs().find(t => t.id === doc.id)).toBeUndefined();
    });

    it("moves closed tab to history", async () => {
      const doc = await bufferStore.createTab();

      await bufferStore.closeTab(doc.id);

      const historyEntry = bufferStore.historyList().find(h => h.id === doc.id);
      expect(historyEntry).toBeDefined();
      expect(historyEntry!.status).toBe("history");
      expect(historyEntry!.closed_at).toBeTruthy();
    });

    it("switches active tab to remaining tab", async () => {
      const first = await bufferStore.createTab();
      const second = await bufferStore.createTab();

      await bufferStore.closeTab(second.id);

      expect(bufferStore.activeTabId()).toBe(first.id);
    });

    it("sets activeTabId to null when last tab is closed", async () => {
      const doc = await bufferStore.createTab();

      await bufferStore.closeTab(doc.id);

      expect(bufferStore.activeTabId()).toBeNull();
    });
  });

  describe("closeOtherTabs", () => {
    it("keeps only the specified tab", async () => {
      const keep = await bufferStore.createTab();
      await bufferStore.createTab();
      await bufferStore.createTab();

      await bufferStore.closeOtherTabs(keep.id);

      expect(mockedApi.closeBuffer).toHaveBeenCalledTimes(2);
      expect(bufferStore.activeTabId()).toBe(keep.id);
    });
  });

  describe("closeAllTabs", () => {
    it("closes every tab and sets activeTabId to null", async () => {
      await bufferStore.createTab();
      await bufferStore.createTab();

      await bufferStore.closeAllTabs();

      expect(mockedApi.closeBuffer).toHaveBeenCalledTimes(2);
      expect(bufferStore.activeTabId()).toBeNull();
    });
  });

  describe("restoreFromHistory", () => {
    it("calls restore API and sets tab as active", async () => {
      const doc = await bufferStore.createTab();
      await bufferStore.closeTab(doc.id);

      mockedApi.listActiveBuffers.mockResolvedValueOnce([{ ...doc, status: "active" as const }]);
      mockedApi.listHistory.mockResolvedValueOnce([]);

      await bufferStore.restoreFromHistory(doc.id);

      expect(mockedApi.restoreBuffer).toHaveBeenCalledWith(doc.id);
      expect(bufferStore.activeTabId()).toBe(doc.id);
    });
  });

  describe("deleteFromHistory", () => {
    it("permanently removes buffer from history", async () => {
      const doc = await bufferStore.createTab();
      await bufferStore.closeTab(doc.id);

      await bufferStore.deleteFromHistory(doc.id);

      expect(mockedApi.deleteBuffer).toHaveBeenCalledWith(doc.id);
      expect(bufferStore.historyList().find(h => h.id === doc.id)).toBeUndefined();
    });
  });

  describe("clearAllHistory", () => {
    it("clears all history entries", async () => {
      const doc = await bufferStore.createTab();
      await bufferStore.closeTab(doc.id);

      await bufferStore.clearAllHistory();

      expect(mockedApi.clearHistory).toHaveBeenCalledOnce();
      expect(bufferStore.historyList()).toEqual([]);
    });
  });

  describe("renameTab", () => {
    it("updates title in active tabs", async () => {
      const doc = await bufferStore.createTab();

      await bufferStore.renameTab(doc.id, "New Name");

      expect(mockedApi.renameBuffer).toHaveBeenCalledWith(doc.id, "New Name");
      expect(bufferStore.activeTabs().find(t => t.id === doc.id)?.title).toBe("New Name");
    });

    it("updates title in history list", async () => {
      const doc = await bufferStore.createTab();
      await bufferStore.closeTab(doc.id);

      await bufferStore.renameTab(doc.id, "Renamed History");

      expect(bufferStore.historyList().find(h => h.id === doc.id)?.title).toBe("Renamed History");
    });
  });

  describe("load", () => {
    it("fetches active and history lists from backend", async () => {
      const active = [mockBuffer({ id: "a-1", status: "active" })];
      const history = [mockBuffer({ id: "h-1", status: "history" })];
      mockedApi.listActiveBuffers.mockResolvedValueOnce(active);
      mockedApi.listHistory.mockResolvedValueOnce(history);

      await bufferStore.load();

      expect(bufferStore.activeTabs()).toEqual(active);
      expect(bufferStore.historyList()).toEqual(history);
    });
  });
});
