import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  createBuffer: vi.fn().mockResolvedValue({
    id: "buf-1",
    title: "Buffer 1",
    filename: "buf-1.md",
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
  openFile: vi.fn().mockImplementation((path: string) => {
    const filename = path.split("/").pop() ?? "untitled";
    return Promise.resolve({
      id: `open-${filename}`,
      title: filename,
      filename,
      status: "active",
      language: "rust",
      source_path: path,
      cursor_pos: 0,
      scroll_pos: 0,
      tab_order: 1,
      created_at: new Date().toISOString(),
      updated_at: new Date().toISOString(),
      closed_at: null,
    });
  }),
  showOpenFileDialog: vi.fn().mockResolvedValue(null),
  saveToSource: vi.fn().mockResolvedValue(undefined),
  hideWindow: vi.fn().mockResolvedValue(undefined),
  minimizeWindow: vi.fn().mockResolvedValue(undefined),
  onDragDrop: vi.fn().mockResolvedValue(() => {}),
}));

import { bufferStore } from "../../stores/buffers";
import * as api from "../../services/tauri";
import { executeCommand, registerCommand } from "../../commands/registry";

const mockedApi = vi.mocked(api);

describe("OS file association integration", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    mockedApi.listActiveBuffers.mockResolvedValue([]);
    mockedApi.listHistory.mockResolvedValue([]);
    await bufferStore.load();
  });

  it("opens a file via the same path as drag-and-drop", async () => {
    await bufferStore.openFile("/home/user/readme.md");

    expect(mockedApi.openFile).toHaveBeenCalledWith("/home/user/readme.md");
    expect(bufferStore.activeTabs().some(t => t.source_path === "/home/user/readme.md")).toBe(true);
  });

  it("opens multiple files sequentially", async () => {
    await bufferStore.openFile("/home/user/a.rs");
    await bufferStore.openFile("/home/user/b.ts");

    expect(mockedApi.openFile).toHaveBeenCalledTimes(2);
    expect(bufferStore.activeTabs().length).toBe(2);
  });

  it("activates the last opened file", async () => {
    await bufferStore.openFile("/home/user/first.rs");
    const second = await bufferStore.openFile("/home/user/second.ts");

    expect(bufferStore.activeTabId()).toBe(second.id);
  });
});

describe("openFile error handling", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    mockedApi.listActiveBuffers.mockResolvedValue([]);
    mockedApi.listHistory.mockResolvedValue([]);
    await bufferStore.load();
  });

  it("propagates backend errors to caller", async () => {
    mockedApi.openFile.mockRejectedValueOnce(new Error("file not found"));

    await expect(bufferStore.openFile("/nonexistent/file.txt")).rejects.toThrow("file not found");
  });

  it("does not add tab when backend fails", async () => {
    mockedApi.openFile.mockRejectedValueOnce(new Error("permission denied"));

    try {
      await bufferStore.openFile("/restricted/file.txt");
    } catch {}

    expect(bufferStore.activeTabs().find(t => t.source_path === "/restricted/file.txt")).toBeUndefined();
  });

  it("does not change activeTabId when backend fails", async () => {
    const existing = await bufferStore.openFile("/home/user/good.rs");
    mockedApi.openFile.mockRejectedValueOnce(new Error("bad file"));

    try {
      await bufferStore.openFile("/home/user/bad.txt");
    } catch {}

    expect(bufferStore.activeTabId()).toBe(existing.id);
  });
});

describe("loadAndActivate", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
  });

  it("sets activeTabId when tabs exist but none is active", async () => {
    const tab = {
      id: "loaded-1", title: "Loaded", filename: "loaded.md",
      status: "active" as const, language: null, source_path: "/path/loaded.md",
      cursor_pos: 0, scroll_pos: 0, tab_order: 0,
      created_at: new Date().toISOString(), updated_at: new Date().toISOString(), closed_at: null,
    };
    mockedApi.listActiveBuffers.mockResolvedValue([tab]);
    mockedApi.listHistory.mockResolvedValue([]);

    await bufferStore.loadAndActivate();

    expect(bufferStore.activeTabId()).toBe("loaded-1");
  });

  it("clears activeTabId when previously active tab no longer exists", async () => {
    mockedApi.listActiveBuffers.mockResolvedValue([]);
    mockedApi.listHistory.mockResolvedValue([]);
    bufferStore.setActiveTabId("removed-tab");

    await bufferStore.loadAndActivate();

    expect(bufferStore.activeTabId()).toBeNull();
  });

  it("preserves activeTabId when tab still exists", async () => {
    const tab = {
      id: "keep-me", title: "Keep", filename: "keep.md",
      status: "active" as const, language: null, source_path: null,
      cursor_pos: 0, scroll_pos: 0, tab_order: 0,
      created_at: new Date().toISOString(), updated_at: new Date().toISOString(), closed_at: null,
    };
    mockedApi.listActiveBuffers.mockResolvedValue([tab]);
    mockedApi.listHistory.mockResolvedValue([]);
    bufferStore.setActiveTabId("keep-me");

    await bufferStore.loadAndActivate();

    expect(bufferStore.activeTabId()).toBe("keep-me");
  });
});

describe("menu:action event integration", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("executes registered command by action id", () => {
    const handler = vi.fn();
    registerCommand({
      id: "test.menu.action",
      label: "Test Action",
      scope: "app",
      execute: handler,
    });

    executeCommand("test.menu.action");

    expect(handler).toHaveBeenCalledOnce();
  });

  it("does not throw for unknown action id", () => {
    expect(() => executeCommand("nonexistent.action")).not.toThrow();
  });
});
