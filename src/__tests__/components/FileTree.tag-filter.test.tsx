import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import type { WorkspaceEntry } from "../../types/workspace";

vi.mock("../../services/tauri", () => ({
  noteFacts: vi.fn(),
  noteAllTags: vi.fn(async () => []),
  noteGraph: vi.fn(),
  notePathsForTag: vi.fn(async () => []),
}));

vi.mock("../../services/events", () => ({
  onEvent: vi.fn().mockResolvedValue(() => {}),
}));

const h = vi.hoisted(() => ({
  entries: new Map<string, WorkspaceEntry[]>(),
  root: "/notes" as string | null,
  sidebar: null as unknown,
}));

vi.mock("../../stores/global/workspace", () => ({
  workspaceStore: {
    root: () => h.root,
    entriesFor: (path: string) => h.entries.get(path),
    loadDir: vi.fn(),
  },
}));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({ tabs: { openFile: vi.fn(async () => undefined) }, sidebar: h.sidebar }),
}));

import FileTree from "../../components/Sidebar/FileTree";
import { createSidebarStore, type SidebarStore } from "../../stores/window/sidebar-store";
import { noteFactsStore } from "../../stores/global/note-facts";
import * as api from "../../services/tauri";

const mockedApi = vi.mocked(api);

function file(path: string): WorkspaceEntry {
  return { name: path.slice(path.lastIndexOf("/") + 1), path, is_dir: false, conflict_copy: null };
}

function dir(path: string): WorkspaceEntry {
  return { name: path.slice(path.lastIndexOf("/") + 1), path, is_dir: true, conflict_copy: null };
}

/** Lets the tag read settle. */
async function settle(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

function names(container: HTMLElement): string[] {
  return Array.from(container.querySelectorAll(".file-tree-item-name")).map(
    (name) => name.textContent!,
  );
}

let sidebar: SidebarStore;

beforeEach(async () => {
  await noteFactsStore.reset();
  vi.clearAllMocks();
  mockedApi.noteAllTags.mockResolvedValue([]);
  mockedApi.notePathsForTag.mockResolvedValue([]);
  h.entries = new Map();
  h.root = "/notes";
  sidebar = createSidebarStore();
  h.sidebar = sidebar;
});

afterEach(() => {
  cleanup();
});

describe("the file tree under a tag", () => {
  it("selecting a tag filters the note list and selecting it again restores the full list", async () => {
    h.entries.set("/notes", [file("/notes/Pricing.md"), file("/notes/Recipes.md")]);
    mockedApi.notePathsForTag.mockResolvedValue(["/notes/Pricing.md"]);

    const { container } = render(() => <FileTree />);
    expect(names(container)).toEqual(["Pricing.md", "Recipes.md"]);

    sidebar.selectTag("idea");
    await settle();
    expect(names(container)).toEqual(["Pricing.md"]);
    expect(mockedApi.notePathsForTag).toHaveBeenCalledWith("idea");

    sidebar.selectTag("idea");
    await settle();
    expect(names(container)).toEqual(["Pricing.md", "Recipes.md"]);
  });

  it("keeps the folders holding a tagged note and opens them to it", async () => {
    h.entries.set("/notes", [dir("/notes/Drafts"), dir("/notes/Archive")]);
    h.entries.set("/notes/Drafts", [file("/notes/Drafts/launch.md"), file("/notes/Drafts/old.md")]);
    mockedApi.notePathsForTag.mockResolvedValue(["/notes/Drafts/launch.md"]);

    const { container } = render(() => <FileTree />);
    sidebar.selectTag("idea");
    await settle();

    expect(names(container)).toEqual(["Drafts", "launch.md"]);
  });

  it("says so when no note in the folder carries the tag", async () => {
    h.entries.set("/notes", [file("/notes/Pricing.md")]);
    mockedApi.notePathsForTag.mockResolvedValue(["/elsewhere/Other.md"]);

    const { container } = render(() => <FileTree />);
    sidebar.selectTag("idea");
    await settle();

    expect(names(container)).toEqual([]);
    expect(container.querySelector(".file-tree-empty")!.textContent).toBe("No notes with this tag");
  });

  it("reads one tag's notes once however many rows the tree draws", async () => {
    h.entries.set("/notes", [dir("/notes/Drafts"), file("/notes/Pricing.md")]);
    h.entries.set("/notes/Drafts", [file("/notes/Drafts/launch.md")]);
    mockedApi.notePathsForTag.mockResolvedValue(["/notes/Drafts/launch.md", "/notes/Pricing.md"]);

    render(() => <FileTree />);
    sidebar.selectTag("idea");
    await settle();

    expect(mockedApi.notePathsForTag).toHaveBeenCalledTimes(1);
  });

  it("hands back the notes of a tag the selection has left", async () => {
    h.entries.set("/notes", [file("/notes/Pricing.md")]);
    mockedApi.notePathsForTag.mockResolvedValue(["/notes/Pricing.md"]);

    render(() => <FileTree />);
    sidebar.selectTag("idea");
    await settle();
    sidebar.selectTag("draft");
    await settle();

    // The first tag is no longer read: a session that tries ten tags does not
    // re-read ten lists on the next change.
    noteFactsStore.pathsForTag("idea");
    await settle();
    expect(mockedApi.notePathsForTag.mock.calls.map(([tag]) => tag)).toEqual([
      "idea",
      "draft",
      "idea",
    ]);
  });

  it("gives a folder its own expansion back when the tag clears", async () => {
    h.entries.set("/notes", [dir("/notes/Drafts")]);
    h.entries.set("/notes/Drafts", [file("/notes/Drafts/launch.md")]);
    mockedApi.notePathsForTag.mockResolvedValue(["/notes/Drafts/launch.md"]);

    const { container } = render(() => <FileTree />);
    sidebar.selectTag("idea");
    await settle();
    expect(names(container)).toEqual(["Drafts", "launch.md"]);

    sidebar.selectTag("idea");
    await settle();
    expect(names(container)).toEqual(["Drafts"]);

    fireEvent.click(container.querySelector<HTMLElement>('[aria-level="1"]')!);
    expect(names(container)).toEqual(["Drafts", "launch.md"]);
  });
});
