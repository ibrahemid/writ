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

function mockSourceBuffer(path: string, overrides: Partial<BufferDocument> = {}): BufferDocument {
  const filename = path.split("/").pop() ?? "untitled";
  return mockBuffer({
    title: filename,
    filename,
    source_path: path,
    language: "rust",
    ...overrides,
  });
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
  openFile: vi.fn().mockImplementation((path: string) =>
    Promise.resolve(mockSourceBuffer(path))
  ),
  showOpenFileDialog: vi.fn().mockResolvedValue(null),
  saveToSource: vi.fn().mockResolvedValue(undefined),
}));

import { bufferStore } from "../../stores/buffers";
import * as api from "../../services/tauri";

const mockedApi = vi.mocked(api);

describe("bufferStore file opening", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    tabIdCounter = 0;
    mockedApi.listActiveBuffers.mockResolvedValue([]);
    mockedApi.listHistory.mockResolvedValue([]);
    await bufferStore.load();
  });

  describe("openFile", () => {
    it("opens a file and adds it to active tabs", async () => {
      const doc = await bufferStore.openFile("/home/user/main.rs");

      expect(mockedApi.openFile).toHaveBeenCalledWith("/home/user/main.rs");
      expect(bufferStore.activeTabs()).toContainEqual(doc);
      expect(bufferStore.activeTabId()).toBe(doc.id);
    });

    it("sets the file as the active tab", async () => {
      await bufferStore.createTab();
      const doc = await bufferStore.openFile("/home/user/lib.rs");

      expect(bufferStore.activeTabId()).toBe(doc.id);
    });

    it("returns source_path and language from backend", async () => {
      const doc = await bufferStore.openFile("/home/user/app.rs");

      expect(doc.source_path).toBe("/home/user/app.rs");
      expect(doc.language).toBe("rust");
    });

    it("deduplicates by source_path in local state", async () => {
      const first = await bufferStore.openFile("/home/user/main.rs");
      await bufferStore.createTab();

      const second = await bufferStore.openFile("/home/user/main.rs");

      expect(second.id).toBe(first.id);
      expect(mockedApi.openFile).toHaveBeenCalledTimes(1);
      expect(bufferStore.activeTabId()).toBe(first.id);
    });

    it("deduplicates by id when backend returns existing buffer", async () => {
      const existingDoc = mockSourceBuffer("/home/user/readme.md", { id: "existing-1" });
      mockedApi.openFile.mockResolvedValueOnce(existingDoc);

      const doc1 = await bufferStore.openFile("/home/user/readme.md");
      expect(bufferStore.activeTabs().filter(t => t.id === "existing-1").length).toBe(1);

      mockedApi.openFile.mockResolvedValueOnce(existingDoc);
      await bufferStore.openFile("/home/user/readme.md");

      expect(bufferStore.activeTabs().filter(t => t.id === "existing-1").length).toBe(1);
    });
  });

  describe("openFileDialog", () => {
    it("returns null when user cancels dialog", async () => {
      mockedApi.showOpenFileDialog.mockResolvedValue(null);

      const result = await bufferStore.openFileDialog();

      expect(result).toBeNull();
      expect(mockedApi.showOpenFileDialog).toHaveBeenCalledOnce();
      expect(mockedApi.openFile).not.toHaveBeenCalled();
    });

    it("opens file when user selects a path", async () => {
      mockedApi.showOpenFileDialog.mockResolvedValue("/home/user/notes.md");

      const doc = await bufferStore.openFileDialog();

      expect(doc).not.toBeNull();
      expect(mockedApi.showOpenFileDialog).toHaveBeenCalledOnce();
      expect(mockedApi.openFile).toHaveBeenCalledWith("/home/user/notes.md");
      expect(bufferStore.activeTabId()).toBe(doc!.id);
    });

    it("adds opened file to active tabs", async () => {
      mockedApi.showOpenFileDialog.mockResolvedValue("/home/user/config.toml");

      const doc = await bufferStore.openFileDialog();

      expect(bufferStore.activeTabs()).toContainEqual(doc);
    });
  });

  describe("mixed scratch and source buffers", () => {
    it("maintains both scratch and source tabs", async () => {
      await bufferStore.createTab("scratch-1");
      await bufferStore.openFile("/home/user/file.rs");
      await bufferStore.createTab("scratch-2");

      expect(bufferStore.activeTabs().length).toBe(3);

      const sourceTab = bufferStore.activeTabs().find(t => t.source_path === "/home/user/file.rs");
      expect(sourceTab).toBeDefined();

      const scratchTabs = bufferStore.activeTabs().filter(t => t.source_path === null);
      expect(scratchTabs.length).toBe(2);
    });

    it("close works on source-backed tabs", async () => {
      const doc = await bufferStore.openFile("/home/user/file.rs");

      await bufferStore.closeTab(doc.id);

      expect(bufferStore.activeTabs().find(t => t.id === doc.id)).toBeUndefined();
      const historyEntry = bufferStore.historyList().find(h => h.id === doc.id);
      expect(historyEntry).toBeDefined();
      expect(historyEntry!.source_path).toBe("/home/user/file.rs");
    });
  });
});
