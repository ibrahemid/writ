import { describe, it, expect, vi, beforeEach } from "vitest";
import type { BufferDocument } from "../../types/buffer";

let tabIdCounter = 0;

function mockSourceBuffer(path: string): BufferDocument {
  tabIdCounter++;
  const filename = path.split("/").pop() ?? "untitled";
  return {
    id: `buf-${tabIdCounter}`,
    title: filename,
    filename,
    status: "active",
    language: "rust",
    source_path: path,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: tabIdCounter,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    closed_at: null,
  };
}

vi.mock("../../services/tauri", () => ({
  createBuffer: vi.fn().mockImplementation(() => {
    tabIdCounter++;
    return Promise.resolve({
      id: `buf-${tabIdCounter}`,
      title: `Buffer ${tabIdCounter}`,
      filename: `buf-${tabIdCounter}.md`,
      status: "active",
      language: null,
      source_path: null,
      cursor_pos: 0,
      scroll_pos: 0,
      tab_order: tabIdCounter,
      created_at: new Date().toISOString(),
      updated_at: new Date().toISOString(),
      closed_at: null,
    });
  }),
  listActiveBuffers: vi.fn().mockResolvedValue([]),
  listHistory: vi.fn().mockResolvedValue([]),
  closeBuffer: vi.fn().mockResolvedValue(undefined),
  deleteBuffer: vi.fn().mockResolvedValue(undefined),
  restoreBuffer: vi.fn().mockResolvedValue(undefined),
  clearHistory: vi.fn().mockResolvedValue(undefined),
  renameBuffer: vi.fn().mockResolvedValue(undefined),
  openFile: vi.fn().mockImplementation((path: string) =>
    Promise.resolve(mockSourceBuffer(path)),
  ),
  showOpenFileDialog: vi.fn().mockResolvedValue(null),
  saveToSource: vi.fn().mockResolvedValue(undefined),
  hideWindow: vi.fn().mockResolvedValue(undefined),
  minimizeWindow: vi.fn().mockResolvedValue(undefined),
}));

import { createTabStore } from "../../stores/window/tab-store";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import * as api from "../../services/tauri";

const mockedApi = vi.mocked(api);

describe("drag-and-drop file handling (per-window tabStore)", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    tabIdCounter = 0;
    mockedApi.listActiveBuffers.mockResolvedValue([]);
    mockedApi.listHistory.mockResolvedValue([]);
    await bufferRegistry.load();
  });

  it("opening files from drop paths calls openFile for each path", async () => {
    const tabs = createTabStore({ registry: bufferRegistry });
    const paths = ["/home/user/file1.rs", "/home/user/file2.ts"];

    for (const path of paths) {
      await tabs.openFile(path);
    }

    expect(mockedApi.openFile).toHaveBeenCalledTimes(2);
    expect(mockedApi.openFile).toHaveBeenCalledWith("/home/user/file1.rs");
    expect(mockedApi.openFile).toHaveBeenCalledWith("/home/user/file2.ts");
  });

  it("sets the last dropped file as active tab", async () => {
    const tabs = createTabStore({ registry: bufferRegistry });
    await tabs.openFile("/home/user/first.rs");
    const second = await tabs.openFile("/home/user/second.ts");

    expect(tabs.activeTabId()).toBe(second.id);
  });

  it("adds all dropped files to active tabs", async () => {
    const tabs = createTabStore({ registry: bufferRegistry });
    await tabs.openFile("/home/user/a.rs");
    await tabs.openFile("/home/user/b.ts");

    const list = bufferRegistry.activeTabs();
    expect(list.length).toBe(2);
    expect(list.some((t) => t.source_path === "/home/user/a.rs")).toBe(true);
    expect(list.some((t) => t.source_path === "/home/user/b.ts")).toBe(true);
  });
});
