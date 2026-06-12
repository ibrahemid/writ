import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  pickWorkspaceFolder: vi.fn(),
  clearWorkspaceRoot: vi.fn().mockResolvedValue(undefined),
  listWorkspaceDir: vi.fn(),
  getWorkspaceRoot: vi.fn(),
}));

import { workspaceStore } from "../../stores/global/workspace";
import {
  pickWorkspaceFolder,
  clearWorkspaceRoot,
  listWorkspaceDir,
  getWorkspaceRoot,
} from "../../services/tauri";
import type { WorkspaceEntry } from "../../types/workspace";

const mockedPick = vi.mocked(pickWorkspaceFolder);
const mockedClear = vi.mocked(clearWorkspaceRoot);
const mockedList = vi.mocked(listWorkspaceDir);
const mockedGetRoot = vi.mocked(getWorkspaceRoot);

function entry(name: string, dir: string, isDir = false): WorkspaceEntry {
  return { name, path: `${dir}/${name}`, is_dir: isDir };
}

describe("workspaceStore", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    mockedClear.mockResolvedValue(undefined);
    await workspaceStore.closeFolder();
  });

  it("hydrate restores the persisted root", async () => {
    mockedGetRoot.mockResolvedValue("/ws");
    await workspaceStore.hydrate();
    expect(workspaceStore.root()).toBe("/ws");
  });

  it("hydrate leaves root null when nothing persisted", async () => {
    mockedGetRoot.mockResolvedValue(null);
    await workspaceStore.hydrate();
    expect(workspaceStore.root()).toBeNull();
  });

  it("openFolder sets the root from the picker", async () => {
    mockedPick.mockResolvedValue("/picked");
    const root = await workspaceStore.openFolder();
    expect(root).toBe("/picked");
    expect(workspaceStore.root()).toBe("/picked");
  });

  it("openFolder keeps existing state when the picker is cancelled", async () => {
    mockedPick.mockResolvedValue(null);
    const root = await workspaceStore.openFolder();
    expect(root).toBeNull();
    expect(workspaceStore.root()).toBeNull();
  });

  it("loadDir caches entries reactively", async () => {
    mockedPick.mockResolvedValue("/ws");
    await workspaceStore.openFolder();

    mockedList.mockResolvedValue([entry("src", "/ws", true), entry("a.md", "/ws")]);
    await workspaceStore.loadDir("/ws");

    expect(workspaceStore.entriesFor("/ws")).toHaveLength(2);
    expect(mockedList).toHaveBeenCalledWith("/ws");
  });

  it("closeFolder clears root and cached listings", async () => {
    mockedPick.mockResolvedValue("/ws");
    await workspaceStore.openFolder();
    mockedList.mockResolvedValue([entry("a.md", "/ws")]);
    await workspaceStore.loadDir("/ws");

    await workspaceStore.closeFolder();

    expect(workspaceStore.root()).toBeNull();
    expect(workspaceStore.entriesFor("/ws")).toBeUndefined();
    expect(mockedClear).toHaveBeenCalled();
  });

  it("handleChanged reloads the parent dir when cached", async () => {
    mockedPick.mockResolvedValue("/ws");
    await workspaceStore.openFolder();
    mockedList.mockResolvedValue([entry("a.md", "/ws")]);
    await workspaceStore.loadDir("/ws");

    mockedList.mockResolvedValue([entry("a.md", "/ws"), entry("b.md", "/ws")]);
    workspaceStore.handleChanged("/ws/b.md", false);
    await vi.waitFor(() => {
      expect(workspaceStore.entriesFor("/ws")).toHaveLength(2);
    });
  });

  it("handleChanged ignores paths whose parent is not cached", async () => {
    mockedPick.mockResolvedValue("/ws");
    await workspaceStore.openFolder();

    workspaceStore.handleChanged("/ws/deep/dir/file.md", false);
    expect(mockedList).not.toHaveBeenCalled();
  });

  it("handleChanged drops the cached listing of a removed directory", async () => {
    mockedPick.mockResolvedValue("/ws");
    await workspaceStore.openFolder();
    mockedList.mockResolvedValue([entry("x.md", "/ws/sub")]);
    await workspaceStore.loadDir("/ws/sub");

    workspaceStore.handleChanged("/ws/sub", true);
    expect(workspaceStore.entriesFor("/ws/sub")).toBeUndefined();
  });
});
