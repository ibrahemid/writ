import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  createBuffer: vi.fn().mockResolvedValue({
    id: "test-1",
    title: "test-buffer",
    filename: "test-1.md",
    status: "active",
    language: null,
    source_path: null,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    closed_at: null,
  }),
  listActiveBuffers: vi.fn().mockResolvedValue([]),
  listHistory: vi.fn().mockResolvedValue([]),
  closeBuffer: vi.fn().mockResolvedValue(undefined),
  deleteBuffer: vi.fn().mockResolvedValue(undefined),
  restoreBuffer: vi.fn().mockResolvedValue(undefined),
  clearHistory: vi.fn().mockResolvedValue(undefined),
  renameBuffer: vi.fn().mockResolvedValue(undefined),
}));

describe("bufferStore.renameTab", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("updates local state after rename", async () => {
    const { bufferStore } = await import("../../stores/buffers");

    await bufferStore.load();
    const doc = await bufferStore.createTab("original");
    expect(bufferStore.activeTabs().find(t => t.id === doc.id)?.title).toBe("test-buffer");

    await bufferStore.renameTab(doc.id, "renamed");
    expect(bufferStore.activeTabs().find(t => t.id === doc.id)?.title).toBe("renamed");
  });
});
