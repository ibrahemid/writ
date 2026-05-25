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
  closeBuffers: vi.fn().mockResolvedValue(undefined),
  deleteBuffer: vi.fn().mockResolvedValue(undefined),
  restoreBuffer: vi.fn().mockResolvedValue(undefined),
  clearHistory: vi.fn().mockResolvedValue(undefined),
  renameBuffer: vi.fn().mockResolvedValue(undefined),
  openFile: vi.fn().mockImplementation((path: string) =>
    Promise.resolve(mockSourceBuffer(path)),
  ),
  showOpenFileDialog: vi.fn().mockResolvedValue(null),
  readBufferContent: vi.fn().mockResolvedValue(""),
}));

vi.mock("../../services/autosave", () => ({
  flushAutosave: vi.fn().mockResolvedValue(undefined),
}));

import { bufferRegistry } from "../../stores/global/buffer-registry";
import * as api from "../../services/tauri";

const mockedApi = vi.mocked(api);

describe("bufferRegistry (app-global)", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    tabIdCounter = 0;
    mockedApi.listActiveBuffers.mockResolvedValue([]);
    mockedApi.listHistory.mockResolvedValue([]);
    await bufferRegistry.load();
  });

  describe("derivations", () => {
    it("derives activeTabs by filtering on status === 'active'", async () => {
      const active = mockBuffer({ id: "a-1", status: "active" });
      const history = mockBuffer({ id: "h-1", status: "history" });
      mockedApi.listActiveBuffers.mockResolvedValueOnce([active]);
      mockedApi.listHistory.mockResolvedValueOnce([history]);

      await bufferRegistry.load();

      expect(bufferRegistry.activeTabs().map((b) => b.id)).toEqual(["a-1"]);
      expect(bufferRegistry.historyList().map((b) => b.id)).toEqual(["h-1"]);
    });
  });

  describe("createBuffer", () => {
    it("returns the new doc and adds it to active tabs", async () => {
      const doc = await bufferRegistry.createBuffer("Notes");
      expect(mockedApi.createBuffer).toHaveBeenCalledWith("Notes");
      expect(bufferRegistry.activeTabs()).toContainEqual(doc);
    });

    it("is idempotent when backend returns an already-listed buffer", async () => {
      const existing = mockBuffer({ id: "existing", status: "active" });
      mockedApi.listActiveBuffers.mockResolvedValueOnce([existing]);
      await bufferRegistry.load();

      mockedApi.createBuffer.mockResolvedValueOnce(existing);
      await bufferRegistry.createBuffer();

      expect(bufferRegistry.activeTabs().filter((b) => b.id === "existing").length).toBe(1);
    });
  });

  describe("closeBuffer", () => {
    it("moves the buffer to history", async () => {
      const doc = await bufferRegistry.createBuffer();
      await bufferRegistry.closeBuffer(doc.id);

      const entry = bufferRegistry.historyList().find((h) => h.id === doc.id);
      expect(entry).toBeDefined();
      expect(entry!.status).toBe("history");
      expect(entry!.closed_at).toBeTruthy();
    });

    it("does not drift when the IPC rejects", async () => {
      const doc = await bufferRegistry.createBuffer();
      mockedApi.closeBuffer.mockRejectedValueOnce(new Error("ipc failed"));

      await expect(bufferRegistry.closeBuffer(doc.id)).rejects.toThrow("ipc failed");

      expect(bufferRegistry.activeTabs().find((t) => t.id === doc.id)).toBeDefined();
      expect(bufferRegistry.historyList().find((h) => h.id === doc.id)).toBeUndefined();
    });
  });

  describe("closeBuffers (bulk)", () => {
    it("uses a single IPC for many buffers", async () => {
      const a = await bufferRegistry.createBuffer();
      const b = await bufferRegistry.createBuffer();

      await bufferRegistry.closeBuffers([a.id, b.id]);

      expect(mockedApi.closeBuffers).toHaveBeenCalledOnce();
      expect(mockedApi.closeBuffers).toHaveBeenCalledWith([a.id, b.id]);
      expect(mockedApi.closeBuffer).not.toHaveBeenCalled();
    });

    it("is a no-op for an empty list", async () => {
      await bufferRegistry.closeBuffers([]);
      expect(mockedApi.closeBuffers).not.toHaveBeenCalled();
    });
  });

  describe("restoreBuffer", () => {
    it("invokes restore IPC and reloads", async () => {
      const doc = await bufferRegistry.createBuffer();
      await bufferRegistry.closeBuffer(doc.id);

      mockedApi.listActiveBuffers.mockResolvedValueOnce([{ ...doc, status: "active" as const }]);
      mockedApi.listHistory.mockResolvedValueOnce([]);

      await bufferRegistry.restoreBuffer(doc.id);

      expect(mockedApi.restoreBuffer).toHaveBeenCalledWith(doc.id);
      expect(bufferRegistry.activeTabs().find((b) => b.id === doc.id)).toBeDefined();
    });
  });

  describe("deleteFromHistory", () => {
    it("permanently removes a buffer", async () => {
      const doc = await bufferRegistry.createBuffer();
      await bufferRegistry.closeBuffer(doc.id);
      await bufferRegistry.deleteFromHistory(doc.id);

      expect(mockedApi.deleteBuffer).toHaveBeenCalledWith(doc.id);
      expect(bufferRegistry.historyList().find((h) => h.id === doc.id)).toBeUndefined();
    });
  });

  describe("clearAllHistory", () => {
    it("clears every history entry", async () => {
      const doc = await bufferRegistry.createBuffer();
      await bufferRegistry.closeBuffer(doc.id);

      await bufferRegistry.clearAllHistory();

      expect(mockedApi.clearHistory).toHaveBeenCalledOnce();
      expect(bufferRegistry.historyList()).toEqual([]);
    });
  });

  describe("renameBuffer", () => {
    it("updates the title in both derivations", async () => {
      const active = await bufferRegistry.createBuffer();
      await bufferRegistry.closeBuffer(active.id);
      const fresh = await bufferRegistry.createBuffer();

      await bufferRegistry.renameBuffer(fresh.id, "renamed-active");
      await bufferRegistry.renameBuffer(active.id, "renamed-history");

      expect(
        bufferRegistry.activeTabs().find((b) => b.id === fresh.id)?.title,
      ).toBe("renamed-active");
      expect(
        bufferRegistry.historyList().find((b) => b.id === active.id)?.title,
      ).toBe("renamed-history");
    });
  });

  describe("openFile", () => {
    it("opens a file and reports it as new", async () => {
      const result = await bufferRegistry.openFile("/home/user/main.rs");
      expect(mockedApi.openFile).toHaveBeenCalledWith("/home/user/main.rs");
      expect(result.existed).toBe(false);
      expect(bufferRegistry.activeTabs()).toContainEqual(result.doc);
    });

    it("dedupes by source_path on subsequent open", async () => {
      const first = await bufferRegistry.openFile("/home/user/main.rs");
      const again = await bufferRegistry.openFile("/home/user/main.rs");

      expect(again.existed).toBe(true);
      expect(again.doc.id).toBe(first.doc.id);
      expect(mockedApi.openFile).toHaveBeenCalledTimes(1);
    });
  });

  describe("readContent", () => {
    it("delegates to api.readBufferContent", async () => {
      mockedApi.readBufferContent.mockResolvedValueOnce("hello");
      const text = await bufferRegistry.readContent("buf-1");
      expect(mockedApi.readBufferContent).toHaveBeenCalledWith("buf-1");
      expect(text).toBe("hello");
    });

    it("propagates rejection", async () => {
      mockedApi.readBufferContent.mockRejectedValueOnce(new Error("not found"));
      await expect(bufferRegistry.readContent("missing")).rejects.toThrow("not found");
    });
  });
});
