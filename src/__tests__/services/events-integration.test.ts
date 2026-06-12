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
    read_only: false,
    size_bytes: 0,
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
    const doc = {
      id: `open-${filename}`,
      title: filename,
      filename,
      status: "active" as const,
      language: "rust",
      source_path: path,
      cursor_pos: 0,
      scroll_pos: 0,
      tab_order: 1,
      created_at: new Date().toISOString(),
      updated_at: new Date().toISOString(),
      closed_at: null,
      read_only: false,
      size_bytes: 0,
    };
    return Promise.resolve({ doc, mode: { kind: "Normal" }, size_bytes: 0 });
  }),
  openFileConfirmed: vi.fn().mockResolvedValue(undefined),
  showOpenFileDialog: vi.fn().mockResolvedValue(null),
  saveToSource: vi.fn().mockResolvedValue(undefined),
  hideWindow: vi.fn().mockResolvedValue(undefined),
  minimizeWindow: vi.fn().mockResolvedValue(undefined),
}));

import { createTabStore } from "../../stores/window/tab-store";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import * as api from "../../services/tauri";
import { executeCommand, registerCommand } from "../../commands/registry";

const mockedApi = vi.mocked(api);

describe("OS file association integration", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    mockedApi.listActiveBuffers.mockResolvedValue([]);
    mockedApi.listHistory.mockResolvedValue([]);
    await bufferRegistry.load();
  });

  it("opens a file via the same path as drag-and-drop", async () => {
    const tabs = createTabStore({ registry: bufferRegistry });
    await tabs.openFile("/home/user/readme.md");

    expect(mockedApi.openFile).toHaveBeenCalledWith("/home/user/readme.md");
    expect(bufferRegistry.activeTabs().some((t) => t.source_path === "/home/user/readme.md")).toBe(true);
  });

  it("opens multiple files sequentially", async () => {
    const tabs = createTabStore({ registry: bufferRegistry });
    await tabs.openFile("/home/user/a.rs");
    await tabs.openFile("/home/user/b.ts");

    expect(mockedApi.openFile).toHaveBeenCalledTimes(2);
    expect(bufferRegistry.activeTabs().length).toBe(2);
  });

  it("activates the last opened file", async () => {
    const tabs = createTabStore({ registry: bufferRegistry });
    await tabs.openFile("/home/user/first.rs");
    const second = await tabs.openFile("/home/user/second.ts");

    expect(tabs.activeTabId()).toBe(second.id);
  });
});

describe("openFile error handling", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    mockedApi.listActiveBuffers.mockResolvedValue([]);
    mockedApi.listHistory.mockResolvedValue([]);
    await bufferRegistry.load();
  });

  it("propagates backend errors to caller", async () => {
    const tabs = createTabStore({ registry: bufferRegistry });
    mockedApi.openFile.mockRejectedValueOnce(new Error("file not found"));

    await expect(tabs.openFile("/nonexistent/file.txt")).rejects.toThrow("file not found");
  });

  it("does not add tab when backend fails", async () => {
    const tabs = createTabStore({ registry: bufferRegistry });
    mockedApi.openFile.mockRejectedValueOnce(new Error("permission denied"));

    try {
      await tabs.openFile("/restricted/file.txt");
    } catch {}

    expect(bufferRegistry.activeTabs().find((t) => t.source_path === "/restricted/file.txt")).toBeUndefined();
  });

  it("does not change activeTabId when backend fails", async () => {
    const tabs = createTabStore({ registry: bufferRegistry });
    const existing = await tabs.openFile("/home/user/good.rs");
    mockedApi.openFile.mockRejectedValueOnce(new Error("bad file"));

    try {
      await tabs.openFile("/home/user/bad.txt");
    } catch {}

    expect(tabs.activeTabId()).toBe(existing.id);
  });
});

describe("loadAndActivate", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("sets activeTabId when tabs exist but none is active", async () => {
    const tab = {
      id: "loaded-1", title: "Loaded", filename: "loaded.md",
      status: "active" as const, language: null, source_path: "/path/loaded.md",
      cursor_pos: 0, scroll_pos: 0, tab_order: 0,
      created_at: new Date().toISOString(), updated_at: new Date().toISOString(), closed_at: null, read_only: false, size_bytes: 0,
    };
    mockedApi.listActiveBuffers.mockResolvedValue([tab]);
    mockedApi.listHistory.mockResolvedValue([]);

    const tabs = createTabStore({ registry: bufferRegistry });
    await tabs.loadAndActivate();

    expect(tabs.activeTabId()).toBe("loaded-1");
  });

  it("clears activeTabId when previously active tab no longer exists", async () => {
    mockedApi.listActiveBuffers.mockResolvedValue([]);
    mockedApi.listHistory.mockResolvedValue([]);
    const tabs = createTabStore({ registry: bufferRegistry });
    tabs.setActiveTabId("removed-tab");

    await tabs.loadAndActivate();

    expect(tabs.activeTabId()).toBeNull();
  });

  it("preserves activeTabId when tab still exists", async () => {
    const tab = {
      id: "keep-me", title: "Keep", filename: "keep.md",
      status: "active" as const, language: null, source_path: null,
      cursor_pos: 0, scroll_pos: 0, tab_order: 0,
      created_at: new Date().toISOString(), updated_at: new Date().toISOString(), closed_at: null, read_only: false, size_bytes: 0,
    };
    mockedApi.listActiveBuffers.mockResolvedValue([tab]);
    mockedApi.listHistory.mockResolvedValue([]);
    const tabs = createTabStore({ registry: bufferRegistry });
    tabs.setActiveTabId("keep-me");

    await tabs.loadAndActivate();

    expect(tabs.activeTabId()).toBe("keep-me");
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
