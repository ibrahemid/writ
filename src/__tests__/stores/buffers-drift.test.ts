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

describe("bufferStore single-source-of-truth invariants", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    tabIdCounter = 0;
    mockedApi.listActiveBuffers.mockResolvedValue([]);
    mockedApi.listHistory.mockResolvedValue([]);
    await bufferStore.load();
  });

  it("does not drift when closeBuffer IPC rejects", async () => {
    const doc = await bufferStore.createTab();
    mockedApi.closeBuffer.mockRejectedValueOnce(new Error("ipc failed"));

    await expect(bufferStore.closeTab(doc.id)).rejects.toThrow("ipc failed");

    expect(bufferStore.activeTabs().find((t) => t.id === doc.id)).toBeDefined();
    expect(bufferStore.historyList().find((h) => h.id === doc.id)).toBeUndefined();
  });

  it("derives activeTabs by filtering buffers on status === 'active'", async () => {
    const active = mockBuffer({ id: "a-1", status: "active" });
    const history = mockBuffer({ id: "h-1", status: "history" });
    mockedApi.listActiveBuffers.mockResolvedValueOnce([active]);
    mockedApi.listHistory.mockResolvedValueOnce([history]);

    await bufferStore.load();

    expect(bufferStore.activeTabs().map((b) => b.id)).toEqual(["a-1"]);
    expect(bufferStore.historyList().map((b) => b.id)).toEqual(["h-1"]);
  });

  it("derives historyList by filtering buffers on status === 'history'", async () => {
    const a = mockBuffer({ id: "x-1", status: "active" });
    const h1 = mockBuffer({ id: "x-2", status: "history" });
    const h2 = mockBuffer({ id: "x-3", status: "history" });
    mockedApi.listActiveBuffers.mockResolvedValueOnce([a]);
    mockedApi.listHistory.mockResolvedValueOnce([h1, h2]);

    await bufferStore.load();

    expect(bufferStore.historyList().map((b) => b.id).sort()).toEqual([
      "x-2",
      "x-3",
    ]);
  });

  it("rename propagates to both active and history derivations from one mutation", async () => {
    const active = await bufferStore.createTab();
    await bufferStore.closeTab(active.id);
    const restored = await bufferStore.createTab();

    await bufferStore.renameTab(restored.id, "renamed-active");
    await bufferStore.renameTab(active.id, "renamed-history");

    expect(
      bufferStore.activeTabs().find((b) => b.id === restored.id)?.title,
    ).toBe("renamed-active");
    expect(
      bufferStore.historyList().find((b) => b.id === active.id)?.title,
    ).toBe("renamed-history");
  });
});
