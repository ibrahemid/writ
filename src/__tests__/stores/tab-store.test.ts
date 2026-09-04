import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import type { BufferDocument } from "../../types/buffer";

let tabIdCounter = 0;
const callLog: string[] = [];

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
    read_only: overrides.read_only ?? false,
    size_bytes: overrides.size_bytes ?? 0,
    line_ending: "lf",
  };
}

const confirmMock = vi.hoisted(() =>
  vi.fn((_request: ConfirmRequest) => Promise.resolve(false)),
);

vi.mock("../../components/ConfirmDialog/ConfirmDialog", () => ({
  requestConfirm: confirmMock,
}));

vi.mock("../../services/tauri", () => ({
  createBuffer: vi.fn().mockImplementation(() => Promise.resolve(mockBuffer())),
  listActiveBuffers: vi.fn().mockResolvedValue([]),
  listHistory: vi.fn().mockResolvedValue([]),
  closeBuffer: vi.fn().mockImplementation(async (id: string) => {
    callLog.push(`closeBuffer:${id}`);
  }),
  closeBuffers: vi.fn().mockImplementation(async (ids: string[]) => {
    callLog.push(`closeBuffers:${ids.join(",")}`);
  }),
  deleteBuffer: vi.fn().mockResolvedValue(undefined),
  restoreBuffer: vi.fn().mockResolvedValue(undefined),
  clearHistory: vi.fn().mockResolvedValue(undefined),
  renameBuffer: vi.fn().mockResolvedValue(undefined),
  openFile: vi.fn().mockImplementation((path: string) => {
    const doc = mockBuffer({ source_path: path, filename: path.split("/").pop() ?? "untitled" });
    return Promise.resolve({ doc, mode: { kind: "Normal" }, size_bytes: doc.size_bytes });
  }),
  showOpenFileDialog: vi.fn().mockResolvedValue(null),
  saveBufferContent: vi.fn().mockImplementation(async (id: string, content: string) => {
    callLog.push(`saveBufferContent:${id}:${content}`);
  }),
  previewClose: vi.fn().mockResolvedValue(undefined),
  materialiseNote: vi.fn().mockResolvedValue(undefined),
  cancelMaterialiseNote: vi.fn().mockResolvedValue(undefined),
}));

import type { ConfirmRequest } from "../../components/ConfirmDialog/ConfirmDialog";
import { createTabStore } from "../../stores/window/tab-store";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { debouncedSave, cancelAutosave } from "../../services/autosave";
import * as api from "../../services/tauri";
import { createDownloadStore } from "../../stores/window/download-store";

const mockedApi = vi.mocked(api);

function freshTabStore() {
  return createTabStore({ downloads: createDownloadStore(), registry: bufferRegistry });
}

describe("tabStore (per-window factory)", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    tabIdCounter = 0;
    callLog.length = 0;
    mockedApi.listActiveBuffers.mockResolvedValue([]);
    mockedApi.listHistory.mockResolvedValue([]);
    confirmMock.mockImplementation(() => Promise.resolve(false));
    for (const b of bufferRegistry.activeTabs()) cancelAutosave(b.id);
    for (const b of bufferRegistry.historyList()) cancelAutosave(b.id);
    await bufferRegistry.load();
  });

  describe("createTab", () => {
    it("creates a buffer and sets it active in this window", async () => {
      const tabs = freshTabStore();
      const doc = await tabs.createTab("Notes");
      expect(mockedApi.createBuffer).toHaveBeenCalledWith("Notes");
      expect(tabs.activeTabId()).toBe(doc!.id);
      expect(bufferRegistry.activeTabs()).toContainEqual(doc);
    });

    it("two window instances have independent activeTabId", async () => {
      const a = freshTabStore();
      const b = freshTabStore();
      const docA = await a.createTab();
      const docB = await b.createTab();
      expect(a.activeTabId()).toBe(docA.id);
      expect(b.activeTabId()).toBe(docB.id);
    });
  });

  describe("closeTab", () => {
    it("removes the tab from the registry and clears activeTabId", async () => {
      const tabs = freshTabStore();
      const doc = await tabs.createTab();

      await tabs.closeTab(doc.id);

      expect(mockedApi.closeBuffer).toHaveBeenCalledWith(doc.id);
      expect(bufferRegistry.activeTabs().find((t) => t.id === doc.id)).toBeUndefined();
      expect(tabs.activeTabId()).toBeNull();
    });

    it("switches to the most recent remaining tab", async () => {
      const tabs = freshTabStore();
      const first = await tabs.createTab();
      const second = await tabs.createTab();
      await tabs.closeTab(second.id);

      expect(tabs.activeTabId()).toBe(first!.id);
    });

    // Regression: closing the active tab must reselect the surviving tab
    // BEFORE flipping the closed buffer to history. The status flip
    // synchronously re-runs the active-buffer memo; if activeTabId still
    // pointed at the closed id, the memo resolves to null for one flush,
    // which disposes and recreates the preview iframe element (a
    // destroy-then-recreate that hard-freezes the macOS webview) instead of
    // navigating its src. Proven by the close IPC observing the survivor
    // already selected.
    it("reselects the surviving tab before the close IPC fires (preview freeze)", async () => {
      const tabs = freshTabStore();
      const first = await tabs.createTab();
      const second = await tabs.createTab();
      expect(tabs.activeTabId()).toBe(second.id);

      let activeWhenClosed: string | null = "UNSET" as unknown as string;
      mockedApi.closeBuffer.mockImplementationOnce(async () => {
        activeWhenClosed = tabs.activeTabId();
      });

      await tabs.closeTab(second.id);

      expect(activeWhenClosed).toBe(first.id);
      expect(tabs.activeTabId()).toBe(first!.id);
    });

    it("closing the only tab reselects null before the close IPC fires", async () => {
      const tabs = freshTabStore();
      const only = await tabs.createTab();

      let activeWhenClosed: string | null = "UNSET" as unknown as string;
      mockedApi.closeBuffer.mockImplementationOnce(async () => {
        activeWhenClosed = tabs.activeTabId();
      });

      await tabs.closeTab(only.id);

      expect(activeWhenClosed).toBeNull();
      expect(tabs.activeTabId()).toBeNull();
    });

    it("stays on the tab when its text could not be written", async () => {
      const tabs = freshTabStore();
      const first = await tabs.createTab();
      const unsaved = await tabs.createTab();
      mockedApi.saveBufferContent.mockRejectedValueOnce(new Error("disk full"));
      debouncedSave(unsaved.id, "work in progress", 0);

      await tabs.closeTab(unsaved.id);

      expect(confirmMock).toHaveBeenCalledOnce();
      expect(confirmMock.mock.calls[0][0]).toMatchObject({
        title: `Couldn't save ${unsaved.title}`,
        confirmLabel: "Close and lose changes",
        cancelLabel: "Keep open",
        defaultAction: "cancel",
      });
      expect(mockedApi.closeBuffer).not.toHaveBeenCalled();
      expect(bufferRegistry.activeTabs().map((b) => b.id)).toEqual([first.id, unsaved.id]);
      expect(tabs.activeTabId()).toBe(unsaved.id);
      cancelAutosave(unsaved.id);
    });

    it("keeps the tab open while a failing write is still in flight", async () => {
      // The write has already left the queue, so a close that only looked at
      // the queue would report success and take the text with it.
      const tabs = freshTabStore();
      await tabs.createTab();
      const unsaved = await tabs.createTab();
      let rejectWrite: ((reason: Error) => void) | undefined;
      mockedApi.saveBufferContent.mockImplementationOnce(
        () => new Promise((_resolve, reject) => { rejectWrite = reject; }),
      );
      mockedApi.saveBufferContent.mockImplementation(() =>
        Promise.reject(new Error("disk full")),
      );
      debouncedSave(unsaved.id, "work in progress", 0);
      await new Promise((resolve) => setTimeout(resolve, 1));
      expect(rejectWrite).toBeDefined();

      const closing = tabs.closeTab(unsaved.id);
      await Promise.resolve();
      rejectWrite!(new Error("disk full"));
      await closing;

      expect(mockedApi.closeBuffer).not.toHaveBeenCalled();
      expect(bufferRegistry.activeTabs().map((b) => b.id)).toContain(unsaved.id);
      expect(tabs.activeTabId()).toBe(unsaved.id);
      cancelAutosave(unsaved.id);
      mockedApi.saveBufferContent.mockReset();
      mockedApi.saveBufferContent.mockImplementation(async (id: string, content: string) => {
        callLog.push(`saveBufferContent:${id}:${content}`);
      });
    });

    it("closes the tab when the user chooses to lose the changes", async () => {
      // Without this the tab is stuck for the rest of the session on a volume
      // that never accepts a write.
      const tabs = freshTabStore();
      const first = await tabs.createTab();
      const unsaved = await tabs.createTab();
      mockedApi.saveBufferContent.mockRejectedValue(new Error("disk full"));
      debouncedSave(unsaved.id, "work in progress", 0);
      confirmMock.mockImplementation(() => Promise.resolve(true));

      await tabs.closeTab(unsaved.id);

      expect(mockedApi.closeBuffer).toHaveBeenCalledWith(unsaved.id);
      expect(bufferRegistry.activeTabs().map((b) => b.id)).toEqual([first.id]);
      expect(tabs.activeTabId()).toBe(first!.id);
      expect(mockedApi.saveBufferContent).toHaveBeenCalledTimes(1);
      mockedApi.saveBufferContent.mockReset();
      mockedApi.saveBufferContent.mockImplementation(async (id: string, content: string) => {
        callLog.push(`saveBufferContent:${id}:${content}`);
      });
    });
  });

  describe("closeOtherTabs", () => {
    it("keeps only the specified tab via a single bulk IPC", async () => {
      const tabs = freshTabStore();
      const keep = await tabs.createTab();
      const second = await tabs.createTab();
      const third = await tabs.createTab();

      await tabs.closeOtherTabs(keep.id);

      expect(mockedApi.closeBuffers).toHaveBeenCalledOnce();
      expect(mockedApi.closeBuffers).toHaveBeenCalledWith([second.id, third.id]);
      expect(mockedApi.closeBuffer).not.toHaveBeenCalled();
      expect(tabs.activeTabId()).toBe(keep.id);
    });
  });

  describe("closeAllTabs", () => {
    it("closes every tab via a single bulk IPC and clears activeTabId", async () => {
      const tabs = freshTabStore();
      const first = await tabs.createTab();
      const second = await tabs.createTab();

      await tabs.closeAllTabs();

      expect(mockedApi.closeBuffers).toHaveBeenCalledOnce();
      expect(mockedApi.closeBuffers).toHaveBeenCalledWith([first.id, second.id]);
      expect(tabs.activeTabId()).toBeNull();
    });

    it("keeps a tab whose text could not be written, and selects it", async () => {
      const tabs = freshTabStore();
      const first = await tabs.createTab();
      const unsaved = await tabs.createTab();
      mockedApi.saveBufferContent.mockRejectedValueOnce(new Error("disk full"));
      debouncedSave(unsaved.id, "work in progress", 0);

      await tabs.closeAllTabs();

      expect(mockedApi.closeBuffers).toHaveBeenCalledWith([first.id]);
      expect(confirmMock).toHaveBeenCalledOnce();
      expect(bufferRegistry.activeTabs().map((b) => b.id)).toEqual([unsaved.id]);
      expect(tabs.activeTabId()).toBe(unsaved.id);
      cancelAutosave(unsaved.id);
    });

    it("closes the rest and offers one choice for the tabs that failed", async () => {
      const tabs = freshTabStore();
      const clean = await tabs.createTab();
      const stuckA = await tabs.createTab();
      const stuckB = await tabs.createTab();
      mockedApi.saveBufferContent.mockImplementation(async (id: string, content: string) => {
        if (id === stuckA.id || id === stuckB.id) throw new Error("disk full");
        callLog.push(`saveBufferContent:${id}:${content}`);
      });
      debouncedSave(stuckA.id, "a", 0);
      debouncedSave(stuckB.id, "b", 0);
      confirmMock.mockImplementation(() => Promise.resolve(true));

      await tabs.closeAllTabs();

      expect(mockedApi.closeBuffers).toHaveBeenNthCalledWith(1, [clean.id]);
      expect(confirmMock).toHaveBeenCalledOnce();
      expect(confirmMock.mock.calls[0][0]).toMatchObject({
        title: "Couldn't save 2 tabs",
        confirmLabel: "Close and lose changes",
        cancelLabel: "Keep open",
      });
      expect(confirmMock.mock.calls[0][0].message).toContain(stuckA.title);
      expect(confirmMock.mock.calls[0][0].message).toContain(stuckB.title);
      expect(mockedApi.closeBuffers).toHaveBeenNthCalledWith(2, [stuckA.id, stuckB.id]);
      expect(bufferRegistry.activeTabs()).toEqual([]);
      expect(tabs.activeTabId()).toBeNull();
      mockedApi.saveBufferContent.mockReset();
      mockedApi.saveBufferContent.mockImplementation(async (id: string, content: string) => {
        callLog.push(`saveBufferContent:${id}:${content}`);
      });
    });
  });

  describe("restoreFromHistory", () => {
    it("restores the buffer and sets it active in this window", async () => {
      const tabs = freshTabStore();
      const doc = await tabs.createTab();
      await tabs.closeTab(doc.id);

      mockedApi.listActiveBuffers.mockResolvedValueOnce([{ ...doc, status: "active" as const }]);
      mockedApi.listHistory.mockResolvedValueOnce([]);

      await tabs.restoreFromHistory(doc.id);

      expect(mockedApi.restoreBuffer).toHaveBeenCalledWith(doc.id);
      expect(tabs.activeTabId()).toBe(doc!.id);
    });
  });

  describe("openFile", () => {
    it("opens the file and sets it active", async () => {
      const tabs = freshTabStore();
      const doc = await tabs.openFile("/home/user/main.rs");
      expect(mockedApi.openFile).toHaveBeenCalledWith("/home/user/main.rs");
      expect(tabs.activeTabId()).toBe(doc!.id);
    });

    it("sends a note whose bytes are not here to the downloads, opening nothing", async () => {
      const downloads = createDownloadStore();
      const tabs = createTabStore({ downloads, registry: bufferRegistry });
      const path = "/home/user/Writ/away.md";
      mockedApi.openFile.mockResolvedValueOnce({
        doc: null,
        mode: { kind: "NotDownloaded", path, provider: "iCloud Drive" },
        size_bytes: 12,
      });

      const doc = await tabs.openFile(path);

      expect(doc).toBeNull();
      expect(tabs.activeTabId()).toBeNull();
      expect(mockedApi.materialiseNote).toHaveBeenCalledWith(path);
      expect(downloads.pending()).toEqual([
        {
          path,
          title: "away.md",
          provider: "iCloud Drive",
          state: "downloading",
          message: null,
          reason: "download",
        },
      ]);
      expect(downloads.selected()?.path).toBe(path);
    });

    it("selecting a note's tab takes the pane back from a pending download", async () => {
      const downloads = createDownloadStore();
      const tabs = createTabStore({ downloads, registry: bufferRegistry });
      const path = "/home/user/Writ/away.md";
      mockedApi.openFile.mockResolvedValueOnce({
        doc: null,
        mode: { kind: "NotDownloaded", path, provider: null },
        size_bytes: 0,
      });
      await tabs.openFile(path);

      const note = await tabs.createTab("Elsewhere");

      expect(tabs.activeTabId()).toBe(note!.id);
      expect(downloads.selected()).toBeNull();
      expect(downloads.pending()).toHaveLength(1);
    });

    it("dedupes by source_path", async () => {
      const tabs = freshTabStore();
      const first = await tabs.openFile("/home/user/main.rs");
      await tabs.createTab();
      const again = await tabs.openFile("/home/user/main.rs");

      expect(again!.id).toBe(first!.id);
      expect(mockedApi.openFile).toHaveBeenCalledTimes(1);
      expect(tabs.activeTabId()).toBe(first!.id);
    });
  });

  describe("openFileDialog", () => {
    it("returns null when user cancels", async () => {
      const tabs = freshTabStore();
      mockedApi.showOpenFileDialog.mockResolvedValueOnce(null);
      const result = await tabs.openFileDialog();
      expect(result).toBeNull();
      expect(mockedApi.openFile).not.toHaveBeenCalled();
    });

    it("opens the chosen file and sets it active", async () => {
      const tabs = freshTabStore();
      mockedApi.showOpenFileDialog.mockResolvedValueOnce("/home/user/notes.md");
      const doc = await tabs.openFileDialog();
      expect(doc).not.toBeNull();
      expect(tabs.activeTabId()).toBe(doc!.id);
    });
  });

  describe("loadAndActivate", () => {
    it("activates the most recent tab when none is currently active", async () => {
      const persisted = [
        mockBuffer({ id: "p-1", status: "active" }),
        mockBuffer({ id: "p-2", status: "active" }),
      ];
      mockedApi.listActiveBuffers.mockResolvedValueOnce(persisted);
      mockedApi.listHistory.mockResolvedValueOnce([]);

      const tabs = freshTabStore();
      await tabs.loadAndActivate();

      expect(tabs.activeTabId()).toBe("p-2");
    });

    it("clears activeTabId if the prior active id is no longer present", async () => {
      const tabs = freshTabStore();
      const doc = await tabs.createTab();
      expect(tabs.activeTabId()).toBe(doc!.id);

      mockedApi.listActiveBuffers.mockResolvedValueOnce([]);
      mockedApi.listHistory.mockResolvedValueOnce([]);
      await tabs.loadAndActivate();

      expect(tabs.activeTabId()).toBeNull();
    });
  });

  describe("autosave flush on close", () => {
    beforeEach(() => {
      vi.useFakeTimers();
    });

    afterEach(() => {
      vi.useRealTimers();
    });

    it("flushes pending autosave before closeTab triggers the close IPC", async () => {
      const tabs = freshTabStore();
      const doc = await tabs.createTab();

      debouncedSave(doc.id, "latest typed content", 300);
      await vi.advanceTimersByTimeAsync(200);
      expect(mockedApi.saveBufferContent).not.toHaveBeenCalled();

      await tabs.closeTab(doc.id);

      expect(mockedApi.saveBufferContent).toHaveBeenCalledWith(doc.id, "latest typed content");
      const saveIdx = callLog.findIndex((e) => e.startsWith(`saveBufferContent:${doc.id}`));
      const closeIdx = callLog.findIndex((e) => e === `closeBuffer:${doc.id}`);
      expect(saveIdx).toBeGreaterThanOrEqual(0);
      expect(closeIdx).toBeGreaterThanOrEqual(0);
      expect(saveIdx).toBeLessThan(closeIdx);
    });

    it("persists an intentional empty buffer via close flush", async () => {
      const tabs = freshTabStore();
      const doc = await tabs.createTab();

      debouncedSave(doc.id, "", 300);
      await vi.advanceTimersByTimeAsync(50);
      await tabs.closeTab(doc.id);

      expect(mockedApi.saveBufferContent).toHaveBeenCalledWith(doc.id, "");
    });

    it("flushes pending autosaves before closeOtherTabs", async () => {
      const tabs = freshTabStore();
      const keep = await tabs.createTab();
      const a = await tabs.createTab();
      const b = await tabs.createTab();

      debouncedSave(a.id, "a-pending", 300);
      debouncedSave(b.id, "b-pending", 300);
      await vi.advanceTimersByTimeAsync(50);

      await tabs.closeOtherTabs(keep.id);

      expect(mockedApi.saveBufferContent).toHaveBeenCalledWith(a.id, "a-pending");
      expect(mockedApi.saveBufferContent).toHaveBeenCalledWith(b.id, "b-pending");
      const closeIdx = callLog.findIndex((e) => e.startsWith("closeBuffers:"));
      const aIdx = callLog.findIndex((e) => e === `saveBufferContent:${a.id}:a-pending`);
      const bIdx = callLog.findIndex((e) => e === `saveBufferContent:${b.id}:b-pending`);
      expect(aIdx).toBeLessThan(closeIdx);
      expect(bIdx).toBeLessThan(closeIdx);
    });

    it("flushes pending autosaves before closeAllTabs", async () => {
      const tabs = freshTabStore();
      const a = await tabs.createTab();
      const b = await tabs.createTab();

      debouncedSave(a.id, "a-final", 300);
      debouncedSave(b.id, "b-final", 300);
      await vi.advanceTimersByTimeAsync(50);

      await tabs.closeAllTabs();

      expect(mockedApi.saveBufferContent).toHaveBeenCalledWith(a.id, "a-final");
      expect(mockedApi.saveBufferContent).toHaveBeenCalledWith(b.id, "b-final");
    });
  });
});
