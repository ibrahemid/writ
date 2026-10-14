import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import type { BufferDocument } from "../../types/buffer";
import type { WorkspaceEntry } from "../../types/workspace";

// The empty state belongs to the notes folder's tree: it says the folder is
// empty once the folder has been listed and found so, and nothing else in the
// sidebar stands in for it or hides it.

const NOTES = "/Users/me/Writ";

const histDoc: BufferDocument = {
  id: "h1",
  title: "notes.rs",
  filename: "notes.rs",
  status: "history",
  language: null,
  source_path: "/proj/src/notes.rs",
  cursor_pos: 0,
  scroll_pos: 0,
  tab_order: 0,
  created_at: new Date().toISOString(),
  updated_at: new Date().toISOString(),
  closed_at: new Date().toISOString(),
  read_only: false,
  size_bytes: 0,
  line_ending: "lf",
  encoding: "UTF-8",
};

const h = vi.hoisted(() => ({
  history: [] as BufferDocument[],
  workspaceRoot: null as string | null,
}));

vi.mock("../../services/autosave", () => ({
  flushAutosave: vi.fn(),
  onAutosaveStart: vi.fn(() => () => {}),
  onAutosaveSuccess: vi.fn(() => () => {}),
  onAutosaveError: vi.fn(() => () => {}),
}));
// The sidebar head reads the window's focus state for the macOS lights.
vi.mock("../../stores/global/os-window", () => ({
  osWindowStore: {
    focused: () => true,
    hide: vi.fn(),
    minimize: vi.fn(),
    toggleFullscreen: vi.fn(),
  },
}));
vi.mock("../../components/ContextMenu/ContextMenu", () => ({
  showContextMenu: vi.fn(),
}));
vi.mock("../../services/tauri", () => ({
  listActiveBuffers: vi.fn(async () => []),
  listHistory: vi.fn(async () => h.history),
  restoreBuffer: vi.fn(async () => {}),
  deleteBuffer: vi.fn(async () => {}),
  closeBuffer: vi.fn(async () => {}),
  clearHistory: vi.fn(async () => {}),
  renameBuffer: vi.fn(async () => {}),
  createBuffer: vi.fn(),
  searchBuffers: vi.fn(async () => ({ hits: [], total: 0 })),
  noteAllTags: vi.fn(async () => []),
  notePathsForTag: vi.fn(async () => []),
  updateConfig: vi.fn(async () => {}),
  getNotesRoot: vi.fn(async () => NOTES),
  listNotesDir: vi.fn(async () => []),
  getWorkspaceRoot: vi.fn(async () => h.workspaceRoot),
  clearWorkspaceRoot: vi.fn(async () => {}),
  listWorkspaceDir: vi.fn(async () => []),
}));
vi.mock("../../services/events", () => ({
  onEvent: vi.fn().mockResolvedValue(() => {}),
}));

import WindowProvider from "../../components/WindowProvider/WindowProvider";
import Sidebar from "../../components/Sidebar/Sidebar";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { configStore } from "../../stores/global/config";
import { notesStore } from "../../stores/global/notes";
import { notesTreeStore } from "../../stores/global/notes-tree";
import { workspaceStore } from "../../stores/global/workspace";
import { windowRegistry } from "../../stores/global/window-registry";
import * as api from "../../services/tauri";

const mockedApi = vi.mocked(api);

async function settle(): Promise<void> {
  for (let i = 0; i < 6; i += 1) await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
}

let windowId = 9500;

async function renderSidebar() {
  configStore.setSidebarSectionHidden("recent", false);
  await bufferRegistry.load();
  await workspaceStore.hydrate();
  await notesStore.load();
  windowId += 1;
  const view = render(() => (
    <WindowProvider windowId={windowId}>
      <Sidebar />
    </WindowProvider>
  ));
  await settle();
  return view;
}

beforeEach(async () => {
  await notesTreeStore.reset();
  vi.clearAllMocks();
  h.history = [];
  h.workspaceRoot = null;
  mockedApi.listNotesDir.mockResolvedValue([]);
  mockedApi.listWorkspaceDir.mockResolvedValue([]);
  await workspaceStore.closeFolder();
});

afterEach(() => {
  cleanup();
});

describe("the notes folder's empty state", () => {
  it("says the folder is empty once it has been listed, and offers the two ways to fill it", async () => {
    let finish!: (entries: WorkspaceEntry[]) => void;
    mockedApi.listNotesDir.mockReturnValueOnce(
      new Promise<WorkspaceEntry[]>((resolve) => {
        finish = resolve;
      }),
    );
    const { container } = await renderSidebar();
    expect(container.querySelector(".sidebar-empty")).toBeNull();
    expect(container.textContent).not.toContain("Empty folder");

    finish([]);
    await settle();

    const empty = container.querySelector(".sidebar-empty");
    expect(empty?.textContent).toContain("This folder is empty.");
    expect(container.querySelectorAll(".sidebar-empty .writ-btn")).toHaveLength(2);
    expect(container.querySelectorAll(".sidebar-empty .kbd-chord")).toHaveLength(1);
  });

  it("says a folder that cannot be listed could not be read rather than calling it empty", async () => {
    mockedApi.listNotesDir.mockRejectedValue(new Error("gone"));

    const { container } = await renderSidebar();

    expect(container.querySelector(".sidebar-empty")).toBeNull();
    expect(container.textContent).toContain("Could not read this folder.");
  });

  it("stays beside Recent, which does not stand in for the folder", async () => {
    h.history = [histDoc];

    const { container } = await renderSidebar();

    expect(container.querySelector(".sidebar-empty")).toBeTruthy();
    expect(container.querySelector(".history-section")).toBeTruthy();
  });

  it("is the notes folder's alone: an empty folder opened by hand says so in its tree", async () => {
    h.workspaceRoot = "/proj";

    const { container } = await renderSidebar();

    expect(container.querySelector(".sidebar-empty")).toBeNull();
    expect(container.querySelector(".files-section .sidebar-section-name")?.textContent).toBe(
      "proj",
    );
    expect(container.textContent).toContain("Empty folder");
  });

  it("goes with the rest of the sections while searching", async () => {
    const { container } = await renderSidebar();
    expect(container.querySelector(".sidebar-empty")).toBeTruthy();

    windowRegistry.getActive()!.sidebar.setSearchQuery("anything");

    expect(container.querySelector(".sidebar-empty")).toBeNull();
  });
});
