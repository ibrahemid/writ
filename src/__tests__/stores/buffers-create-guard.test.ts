import { describe, it, expect, vi, beforeEach } from "vitest";

const callOrder: string[] = [];

const existingDoc = {
  id: "dup-1",
  title: "writ-1",
  filename: "writ-1",
  status: "active" as const,
  language: null,
  source_path: null,
  cursor_pos: 0,
  scroll_pos: 0,
  tab_order: 0,
  created_at: new Date().toISOString(),
  updated_at: new Date().toISOString(),
  closed_at: null,
};

vi.mock("../../services/autosave", () => ({
  flushAutosave: vi.fn(async () => {
    callOrder.push("flush");
  }),
}));

vi.mock("../../services/tauri", () => ({
  createBuffer: vi.fn(async () => {
    callOrder.push("create");
    return existingDoc;
  }),
  listActiveBuffers: vi.fn().mockResolvedValue([]),
  listHistory: vi.fn().mockResolvedValue([]),
}));

describe("bufferStore.createTab guard", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    callOrder.length = 0;
    vi.resetModules();
  });

  it("flushes autosave before creating", async () => {
    const { bufferStore } = await import("../../stores/buffers");
    await bufferStore.createTab();
    expect(callOrder).toEqual(["flush", "create"]);
  });

  it("is idempotent when backend returns an existing buffer", async () => {
    const tauri = await import("../../services/tauri");
    (tauri.listActiveBuffers as ReturnType<typeof vi.fn>).mockResolvedValueOnce([
      existingDoc,
    ]);
    const { bufferStore } = await import("../../stores/buffers");

    await bufferStore.load();
    expect(bufferStore.activeTabs()).toHaveLength(1);

    await bufferStore.createTab();

    expect(bufferStore.activeTabs()).toHaveLength(1);
    expect(bufferStore.activeTabId()).toBe("dup-1");
  });
});
