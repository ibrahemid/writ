import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import type { BufferDocument } from "../../types/buffer";
import type { WorkspaceEntry } from "../../types/workspace";

// The sidebar's folder section on a fresh install: the notes folder is the
// tree whenever no other folder is open, the tag filter filters it, and a
// write Writ makes itself reaches it through `notes:indexed`.

const NOTES = "/Users/me/Writ";

const h = vi.hoisted(() => ({
  active: [] as BufferDocument[],
  history: [] as BufferDocument[],
  workspaceRoot: null as string | null,
  workspaceDirs: new Map<string, WorkspaceEntry[]>(),
  notesDirs: new Map<string, WorkspaceEntry[]>(),
  handlers: new Map<string, ((payload: unknown) => void)[]>(),
}));

vi.mock("../../services/autosave", () => ({
  flushAutosave: vi.fn().mockResolvedValue({ ok: true, failures: [] }),
  onAutosaveStart: vi.fn(() => () => {}),
  onAutosaveSuccess: vi.fn(() => () => {}),
  onAutosaveError: vi.fn(() => () => {}),
}));
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
  listActiveBuffers: vi.fn(async () => h.active),
  listHistory: vi.fn(async () => h.history),
  restoreBuffer: vi.fn(async () => {}),
  deleteBuffer: vi.fn(async () => {}),
  closeBuffer: vi.fn(async () => {}),
  clearHistory: vi.fn(async () => {}),
  renameBuffer: vi.fn(async () => {}),
  createBuffer: vi.fn(),
  searchBuffers: vi.fn(async () => ({ hits: [], total: 0 })),
  updateConfig: vi.fn(async () => {}),
  getNotesRoot: vi.fn(async () => NOTES),
  getWorkspaceRoot: vi.fn(async () => h.workspaceRoot),
  clearWorkspaceRoot: vi.fn(async () => {}),
  listWorkspaceDir: vi.fn(async (dir: string) => h.workspaceDirs.get(dir) ?? []),
  listNotesDir: vi.fn(async (dir: string) => h.notesDirs.get(dir) ?? []),
  noteAllTags: vi.fn(async () => []),
  notePathsForTag: vi.fn(async () => []),
  noteFacts: vi.fn(),
  noteGraph: vi.fn(),
  noteBacklinks: vi.fn(),
  openFile: vi.fn(),
}));
vi.mock("../../services/events", () => ({
  onEvent: vi.fn(async (kind: string, handler: (payload: unknown) => void) => {
    const list = h.handlers.get(kind) ?? [];
    list.push(handler);
    h.handlers.set(kind, list);
    return () => {
      const held = h.handlers.get(kind) ?? [];
      h.handlers.set(
        kind,
        held.filter((candidate) => candidate !== handler),
      );
    };
  }),
}));

import WindowProvider from "../../components/WindowProvider/WindowProvider";
import Sidebar from "../../components/Sidebar/Sidebar";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { configStore } from "../../stores/global/config";
import { notesStore } from "../../stores/global/notes";
import { workspaceStore } from "../../stores/global/workspace";
import { windowRegistry } from "../../stores/global/window-registry";
import { registerCommand, unregisterCommand } from "../../commands/registry";
import { indexCacheStore } from "../../stores/global/index-cache";
import { notesTreeStore } from "../../stores/global/notes-tree";
import * as api from "../../services/tauri";

const mockedApi = vi.mocked(api);

function file(path: string): WorkspaceEntry {
  return { name: path.slice(path.lastIndexOf("/") + 1), path, is_dir: false, conflict_copy: null };
}

function closed(path: string): BufferDocument {
  const now = new Date().toISOString();
  return {
    id: `h-${path}`,
    title: path.slice(path.lastIndexOf("/") + 1),
    filename: path.slice(path.lastIndexOf("/") + 1),
    status: "history",
    language: null,
    source_path: path,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: now,
    updated_at: now,
    closed_at: now,
    read_only: false,
    size_bytes: 0,
    line_ending: "lf",
  };
}

/** Delivers `payload` to every listener the stores opened for `kind`. */
function deliver(kind: string, payload: unknown): void {
  const listeners = h.handlers.get(kind) ?? [];
  expect(listeners.length, `nothing listens for ${kind}`).toBeGreaterThan(0);
  for (const handler of listeners) handler(payload);
}

async function settle(): Promise<void> {
  for (let i = 0; i < 6; i += 1) await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  for (let i = 0; i < 6; i += 1) await Promise.resolve();
}

function treeNames(container: HTMLElement): string[] {
  return Array.from(container.querySelectorAll(".files-section .file-tree-item-name")).map(
    (name) => name.textContent!,
  );
}

function listedUnderTag(container: HTMLElement): string[] {
  return Array.from(container.querySelectorAll(".tags-section .tags-file-name")).map(
    (name) => name.textContent!,
  );
}

function rowsUnderTag(container: HTMLElement): string[][] {
  return Array.from(container.querySelectorAll(".tags-section .tags-file")).map((row) =>
    Array.from(row.querySelectorAll(".tags-file-name, .tags-file-folder")).map(
      (part) => part.textContent!,
    ),
  );
}

let windowId = 9700;

async function mount() {
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

function tagRow(container: HTMLElement, name: string): HTMLElement {
  const row = Array.from(container.querySelectorAll<HTMLElement>(".tags-row")).find(
    (candidate) => candidate.querySelector(".tags-row-name")?.textContent === name,
  );
  expect(row, `no tag row named ${name}`).toBeDefined();
  return row!;
}

beforeEach(async () => {
  await indexCacheStore.reset();
  await notesTreeStore.reset();
  vi.clearAllMocks();
  h.active = [];
  h.history = [];
  h.workspaceRoot = null;
  h.workspaceDirs = new Map();
  h.notesDirs = new Map();
  h.handlers = new Map();
  mockedApi.noteAllTags.mockResolvedValue([]);
  mockedApi.notePathsForTag.mockResolvedValue([]);
  await workspaceStore.closeFolder();
  // A fresh config: Recent hidden, every app off, no folder opened by hand.
  await configStore.load();
});

afterEach(() => {
  cleanup();
});

describe("the folder section on a fresh install", () => {
  it("shows the notes folder beside one closed file rather than an empty column", async () => {
    h.history = [closed(`${NOTES}/2026-10-13.txt`)];
    h.notesDirs.set(NOTES, [file(`${NOTES}/2026-10-13.txt`)]);

    const { container } = await mount();

    expect(container.querySelector(".files-section .sidebar-section-name")?.textContent).toBe(
      "Writ",
    );
    expect(treeNames(container)).toEqual(["2026-10-13"]);
    expect(container.textContent).not.toContain("No files yet.");
  });

  it("lists the notes folder beside an open file, never saying it holds nothing", async () => {
    h.notesDirs.set(NOTES, [file(`${NOTES}/Plan.md`), file(`${NOTES}/Shopping.txt`)]);

    const { container } = await mount();

    expect(treeNames(container)).toEqual(["Plan", "Shopping"]);
    expect(container.querySelector(".sidebar-empty")).toBeNull();
  });

  it("says an empty notes folder is empty and offers a new file and the folder", async () => {
    const showFolder = vi.fn();
    registerCommand({
      id: "notes.showFolder",
      label: "Show folder in Finder",
      scope: "app",
      execute: showFolder,
    });

    const { container } = await mount();

    const empty = container.querySelector<HTMLElement>(".sidebar-empty");
    expect(empty, "an empty folder has an empty state").not.toBeNull();
    expect(empty!.textContent).toContain("This folder is empty.");
    const buttons = Array.from(empty!.querySelectorAll<HTMLButtonElement>("button"));
    expect(buttons.map((button) => button.textContent?.trim())).toEqual([
      "New file",
      "Show folder in Finder",
    ]);
    fireEvent.click(buttons[1]);
    expect(showFolder).toHaveBeenCalledTimes(1);
    unregisterCommand("notes.showFolder");
  });

  it("shows a folder opened by hand in place of the notes folder", async () => {
    h.workspaceRoot = "/proj";
    h.workspaceDirs.set("/proj", [file("/proj/main.rs")]);
    h.notesDirs.set(NOTES, [file(`${NOTES}/Plan.md`)]);

    const { container } = await mount();

    expect(container.querySelector(".files-section .sidebar-section-name")?.textContent).toBe(
      "proj",
    );
    expect(treeNames(container)).toEqual(["main.rs"]);
    await workspaceStore.closeFolder();
    await settle();
    expect(treeNames(container)).toEqual(["Plan"]);
  });
});

describe("Writ's own writes reach the notes folder", () => {
  it("lists a file Writ created once its index event arrives", async () => {
    h.notesDirs.set(NOTES, [file(`${NOTES}/A.md`)]);
    const { container } = await mount();
    expect(treeNames(container)).toEqual(["A"]);

    h.notesDirs.set(NOTES, [file(`${NOTES}/A.md`), file(`${NOTES}/B.md`)]);
    deliver("notes:indexed", { paths: [`${NOTES}/B.md`] });
    await settle();

    expect(treeNames(container)).toEqual(["A", "B"]);
  });

  it("drops a file Writ deleted and follows one it renamed", async () => {
    h.notesDirs.set(NOTES, [file(`${NOTES}/A.md`), file(`${NOTES}/B.md`)]);
    const { container } = await mount();

    h.notesDirs.set(NOTES, [file(`${NOTES}/B.md`)]);
    deliver("notes:indexed", { paths: [`${NOTES}/A.md`] });
    await settle();
    expect(treeNames(container)).toEqual(["B"]);

    h.notesDirs.set(NOTES, [file(`${NOTES}/C.md`)]);
    deliver("notes:indexed", { paths: [`${NOTES}/B.md`, `${NOTES}/C.md`] });
    await settle();
    expect(treeNames(container)).toEqual(["C"]);
  });
});

describe("tags over the notes folder", () => {
  beforeEach(() => {
    configStore.noteAppsOn(["tags"]);
  });

  it("says how to make a tag while there is none", async () => {
    h.notesDirs.set(NOTES, [file(`${NOTES}/Plan.md`)]);

    const { container } = await mount();

    expect(container.querySelector(".tags-section")?.textContent).toContain(
      "Type #tag in a file to list it here.",
    );
  });

  it("lists a tag a save added once the index event arrives", async () => {
    h.notesDirs.set(NOTES, [file(`${NOTES}/Plan.md`)]);
    const { container } = await mount();

    mockedApi.noteAllTags.mockResolvedValue([{ tag: "plan", count: 1 }]);
    deliver("notes:indexed", { paths: [`${NOTES}/Plan.md`] });
    await settle();

    expect(tagRow(container, "plan")).toBeTruthy();
    expect(container.textContent).not.toContain("Type #tag in a file to list it here.");
  });

  it("filters the notes folder with no folder open, and the heading says so until cleared", async () => {
    h.notesDirs.set(NOTES, [file(`${NOTES}/Plan.md`), file(`${NOTES}/Shopping.md`)]);
    mockedApi.noteAllTags.mockResolvedValue([{ tag: "plan", count: 1 }]);
    mockedApi.notePathsForTag.mockResolvedValue([`${NOTES}/Plan.md`]);
    const { container } = await mount();
    expect(treeNames(container)).toEqual(["Plan", "Shopping"]);

    fireEvent.click(tagRow(container, "plan"));
    await settle();

    expect(treeNames(container)).toEqual(["Plan"]);
    const chip = container.querySelector<HTMLElement>(".files-section .files-tag-chip");
    expect(chip, "a filtered tree says it is filtered").not.toBeNull();
    expect(chip!.textContent).toContain("#plan");

    fireEvent.click(chip!.querySelector<HTMLButtonElement>("button")!);
    await settle();
    expect(windowRegistry.getActive()!.sidebar.selectedTag()).toBeNull();
    expect(treeNames(container)).toEqual(["Plan", "Shopping"]);
    expect(container.querySelector(".files-tag-chip")).toBeNull();
  });

  it("lists the tagged files under the tag when the open folder is not the notes folder", async () => {
    h.workspaceRoot = "/proj";
    h.workspaceDirs.set("/proj", [file("/proj/main.rs"), file("/proj/README.md")]);
    mockedApi.noteAllTags.mockResolvedValue([{ tag: "plan", count: 1 }]);
    mockedApi.notePathsForTag.mockResolvedValue([`${NOTES}/Plan.md`]);
    const { container } = await mount();

    fireEvent.click(tagRow(container, "plan"));
    await settle();

    expect(treeNames(container)).toEqual(["main.rs", "README"]);
    expect(container.textContent).not.toContain("No files with this tag");
    expect(container.querySelector(".files-tag-chip")).toBeNull();
    expect(listedUnderTag(container)).toEqual(["Plan"]);
  });

  it("lists the tagged files under the tag when the notes folder itself is opened by hand", async () => {
    h.workspaceRoot = NOTES;
    h.workspaceDirs.set(NOTES, [file(`${NOTES}/Plan.md`), file(`${NOTES}/Shopping.md`)]);
    mockedApi.noteAllTags.mockResolvedValue([{ tag: "plan", count: 2 }]);
    mockedApi.notePathsForTag.mockResolvedValue([`${NOTES}/Plan.md`, `${NOTES}/build/Plan.md`]);
    const { container } = await mount();

    fireEvent.click(tagRow(container, "plan"));
    await settle();

    expect(treeNames(container)).toEqual(["Plan", "Shopping"]);
    expect(listedUnderTag(container)).toEqual(["Plan", "Plan"]);
    expect(container.querySelector(".files-tag-chip")).toBeNull();
  });

  it("never says no file carries a tag whose only file the opened folder's tree leaves out", async () => {
    h.workspaceRoot = NOTES;
    h.workspaceDirs.set(NOTES, [file(`${NOTES}/Shopping.md`)]);
    mockedApi.noteAllTags.mockResolvedValue([{ tag: "plan", count: 1 }]);
    mockedApi.notePathsForTag.mockResolvedValue([`${NOTES}/build/Plan.md`]);
    const { container } = await mount();

    fireEvent.click(tagRow(container, "plan"));
    await settle();

    expect(container.textContent).not.toContain("No files with this tag");
    expect(listedUnderTag(container)).toEqual(["Plan"]);
  });

  it("lists the tagged files under the tag when the folder opened by hand holds the notes folder", async () => {
    // The way down to the notes folder passes a name a code folder's tree
    // leaves out, so the tree of the folder above cannot reach any note.
    mockedApi.getNotesRoot.mockResolvedValueOnce("/Users/me/vendor/Writ");
    h.workspaceRoot = "/Users/me";
    h.workspaceDirs.set("/Users/me", [file("/Users/me/todo.md")]);
    mockedApi.noteAllTags.mockResolvedValue([{ tag: "plan", count: 1 }]);
    mockedApi.notePathsForTag.mockResolvedValue(["/Users/me/vendor/Writ/Plan.md"]);
    const { container } = await mount();

    fireEvent.click(tagRow(container, "plan"));
    await settle();

    expect(treeNames(container)).toEqual(["todo"]);
    expect(listedUnderTag(container)).toEqual(["Plan"]);
    expect(container.textContent).not.toContain("No files with this tag");
  });

  it("lists the tagged files under the tag while the Files section is folded", async () => {
    h.notesDirs.set(NOTES, [file(`${NOTES}/Plan.md`), file(`${NOTES}/Shopping.md`)]);
    mockedApi.noteAllTags.mockResolvedValue([{ tag: "plan", count: 1 }]);
    mockedApi.notePathsForTag.mockResolvedValue([`${NOTES}/Plan.md`]);
    const { container } = await mount();
    configStore.setSidebarSectionCollapsed("folder", true);
    await settle();

    fireEvent.click(tagRow(container, "plan"));
    await settle();

    expect(treeNames(container)).toEqual([]);
    expect(listedUnderTag(container)).toEqual(["Plan"]);
    expect(container.querySelector(".files-tag-chip")).toBeNull();
  });

  it("moves the tagged files under the tag when the filtered Files section folds, and back into the tree when it opens", async () => {
    h.notesDirs.set(NOTES, [file(`${NOTES}/Plan.md`), file(`${NOTES}/Shopping.md`)]);
    mockedApi.noteAllTags.mockResolvedValue([{ tag: "plan", count: 1 }]);
    mockedApi.notePathsForTag.mockResolvedValue([`${NOTES}/Plan.md`]);
    const { container } = await mount();
    fireEvent.click(tagRow(container, "plan"));
    await settle();
    expect(treeNames(container)).toEqual(["Plan"]);
    expect(listedUnderTag(container)).toEqual([]);

    configStore.setSidebarSectionCollapsed("folder", true);
    await settle();

    expect(treeNames(container)).toEqual([]);
    expect(listedUnderTag(container)).toEqual(["Plan"]);
    expect(container.querySelector(".files-tag-chip")).toBeNull();

    configStore.setSidebarSectionCollapsed("folder", false);
    await settle();

    expect(treeNames(container)).toEqual(["Plan"]);
    expect(listedUnderTag(container)).toEqual([]);
    expect(container.querySelector(".files-section .files-tag-chip")?.textContent).toContain(
      "#plan",
    );
  });

  it("shows the folder beside a name two files under the tag share", async () => {
    h.workspaceRoot = "/proj";
    h.workspaceDirs.set("/proj", [file("/proj/main.rs")]);
    mockedApi.noteAllTags.mockResolvedValue([{ tag: "plan", count: 3 }]);
    mockedApi.notePathsForTag.mockResolvedValue([
      `${NOTES}/A/Plan.md`,
      `${NOTES}/B/Plan.md`,
      `${NOTES}/Shopping.md`,
    ]);
    const { container } = await mount();

    fireEvent.click(tagRow(container, "plan"));
    await settle();

    expect(rowsUnderTag(container)).toEqual([["Plan", "A"], ["Plan", "B"], ["Shopping"]]);
  });

  it("lists the tagged files under the tag while the Files section is switched off", async () => {
    h.notesDirs.set(NOTES, [file(`${NOTES}/Plan.md`), file(`${NOTES}/Shopping.md`)]);
    mockedApi.noteAllTags.mockResolvedValue([{ tag: "plan", count: 1 }]);
    mockedApi.notePathsForTag.mockResolvedValue([`${NOTES}/Plan.md`]);
    const { container } = await mount();
    configStore.setSidebarSectionHidden("folder", true);
    await settle();

    fireEvent.click(tagRow(container, "plan"));
    await settle();

    expect(container.querySelector(".files-section")).toBeNull();
    expect(listedUnderTag(container)).toEqual(["Plan"]);
  });

  it("shows a failed tag read rather than hiding the section", async () => {
    mockedApi.noteAllTags.mockRejectedValue(new Error("index unreadable"));

    const { container } = await mount();

    expect(container.querySelector(".tags-section")?.textContent).toContain(
      "Could not read the tags.",
    );
  });

  it("shows a failed read of a tag's files rather than saying none carry it", async () => {
    h.notesDirs.set(NOTES, [file(`${NOTES}/Plan.md`)]);
    mockedApi.noteAllTags.mockResolvedValue([{ tag: "plan", count: 1 }]);
    mockedApi.notePathsForTag.mockRejectedValue(new Error("index unreadable"));
    const { container } = await mount();

    fireEvent.click(tagRow(container, "plan"));
    await settle();

    expect(container.textContent).toContain("Could not read the files with this tag.");
    expect(container.textContent).not.toContain("No files with this tag");
  });
});
