import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import type { WorkspaceEntry } from "../../types/workspace";
import type { FolderTree } from "../../stores/global/sidebar-folder";
import type { MenuItem } from "../../components/ContextMenu/ContextMenu";

const h = await vi.hoisted(async () => {
  const { createStore } = await import("solid-js/store");
  const [state, setState] = createStore<{
    entries: Record<string, WorkspaceEntry[] | undefined>;
    errors: Record<string, string | undefined>;
    unreadable: Record<string, number | undefined>;
    loading: Record<string, boolean | undefined>;
  }>({ entries: {}, errors: {}, unreadable: {}, loading: {} });
  return {
    state,
    setState,
    openFile: vi.fn(),
    newNoteInFolder: vi.fn(async () => ({ id: "new" })),
    menu: [] as MenuItem[],
    copyText: vi.fn(async () => {}),
    renameNoteAndLinks: vi.fn(async () => {}),
    confirmAndDeleteNote: vi.fn(async () => {}),
    showToast: vi.fn(),
  };
});

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    tabs: { openFile: h.openFile, newNoteInFolder: h.newNoteInFolder, activeTabId: () => null },
    sidebar: { selectedTag: () => null },
  }),
}));
vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: { activeTabs: () => [] },
}));
vi.mock("../../components/ContextMenu/ContextMenu", () => ({
  showContextMenu: vi.fn((_x: number, _y: number, items: MenuItem[]) => {
    h.menu = items;
  }),
}));
vi.mock("../../stores/global/clipboard", () => ({
  clipboardStore: { copyText: h.copyText },
}));
vi.mock("../../stores/global/notifications", () => ({ showToast: h.showToast }));
vi.mock("../../lib/note-actions", () => ({
  renameNoteAndLinks: h.renameNoteAndLinks,
  confirmAndDeleteNote: h.confirmAndDeleteNote,
  showInFileManagerLabel: () => "Show in Finder",
}));
vi.mock("../../stores/global/notes", () => ({
  notesStore: {
    root: () => "/notes",
    folder: () => null,
    contains: (path: string) => path.startsWith("/w/notes/"),
  },
}));

import FileTree from "../../components/Sidebar/FileTree";
import { treeExpansionStore } from "../../stores/global/tree-expansion";

const ROOT = "/w";
const loadDir = vi.fn(async (_path: string) => {});
const revealEntry = vi.fn(async (_path: string) => {});

const folder: FolderTree = {
  root: () => ROOT,
  entriesFor: (path) => h.state.entries[path],
  errorFor: (path) => h.state.errors[path] ?? null,
  unreadableFor: (path) => h.state.unreadable[path] ?? 0,
  isLoading: (path) => h.state.loading[path] === true,
  loadDir,
  revealEntry,
};

function file(path: string): WorkspaceEntry {
  return { name: path.slice(path.lastIndexOf("/") + 1), path, is_dir: false, conflict_copy: null };
}

function dir(path: string): WorkspaceEntry {
  return { name: path.slice(path.lastIndexOf("/") + 1), path, is_dir: true, conflict_copy: null };
}

function items(container: HTMLElement): HTMLElement[] {
  return Array.from(container.querySelectorAll<HTMLElement>('[role="treeitem"]'));
}

function named(container: HTMLElement, name: string): HTMLElement {
  const row = items(container).find((el) => el.textContent?.startsWith(name));
  expect(row, `a row named ${name}`).toBeDefined();
  return row!;
}

function key(el: Element, k: string) {
  fireEvent.keyDown(el, { key: k });
}

beforeEach(() => {
  h.setState({
    entries: {
      [ROOT]: [dir("/w/notes"), dir("/w/src"), file("/w/main.rs"), file("/w/make.sh")],
      "/w/src": [file("/w/src/lib.rs")],
      "/w/notes": [dir("/w/notes/Projects"), file("/w/notes/Plan.md")],
    },
    errors: {},
    unreadable: {},
    loading: {},
  });
  h.openFile.mockResolvedValue({ id: "doc-1" });
});

afterEach(() => {
  for (const path of ["/w/src", "/w/notes", "/w/locked", "/w/big"]) {
    treeExpansionStore.setExpanded(ROOT, path, false);
  }
  vi.clearAllMocks();
  h.menu = [];
  cleanup();
});

describe("a folder of 20,000 entries (F21)", () => {
  beforeEach(() => {
    h.setState("entries", ROOT, [
      ...Array.from({ length: 20_000 }, (_, i) => file(`/w/f${String(i).padStart(5, "0")}.js`)),
    ]);
  });

  it("draws the rows on screen rather than every row it holds", () => {
    const { container } = render(() => <FileTree folder={folder} />);
    const drawn = items(container);
    expect(drawn.length).toBeGreaterThan(0);
    expect(drawn.length).toBeLessThan(60);
    expect(drawn[0].getAttribute("aria-setsize")).toBe("20000");
    expect(drawn[0].getAttribute("aria-posinset")).toBe("1");
    const list = container.querySelector<HTMLElement>('[role="tree"]')!;
    expect(parseInt(list.style.height, 10)).toBe(20_000 * 28);
  });

  it("reaches the last row with End and the first with Home, still drawing a handful", () => {
    const { container } = render(() => <FileTree folder={folder} />);
    key(items(container)[0], "End");
    const last = document.activeElement as HTMLElement;
    expect(last.getAttribute("aria-posinset")).toBe("20000");
    expect(last.textContent).toContain("f19999");
    expect(items(container).length).toBeLessThan(60);

    key(last, "Home");
    expect((document.activeElement as HTMLElement).getAttribute("aria-posinset")).toBe("1");
  });
});

describe("the tree's keyboard (NV-17)", () => {
  it("is one Tab stop, which the arrows move", () => {
    const { container } = render(() => <FileTree folder={folder} />);
    const stops = () => items(container).filter((el) => el.tabIndex === 0);
    expect(stops()).toHaveLength(1);
    expect(stops()[0].textContent).toContain("notes");

    key(stops()[0], "ArrowDown");
    expect(stops()).toHaveLength(1);
    expect(stops()[0].textContent).toContain("src");
    expect(document.activeElement).toBe(stops()[0]);
  });

  it("opens a folder with Right, steps into it with Right, and back out with Left", () => {
    const { container } = render(() => <FileTree folder={folder} />);
    const src = named(container, "src");
    src.focus();

    key(src, "ArrowRight");
    expect(src.getAttribute("aria-expanded")).toBe("true");
    expect(treeExpansionStore.isExpanded(ROOT, "/w/src")).toBe(true);

    key(src, "ArrowRight");
    expect(document.activeElement?.textContent).toContain("lib.rs");
    expect((document.activeElement as HTMLElement).getAttribute("aria-level")).toBe("2");

    key(document.activeElement!, "ArrowLeft");
    expect(document.activeElement).toBe(src);
    key(src, "ArrowLeft");
    expect(src.getAttribute("aria-expanded")).toBe("false");
    expect(items(container).some((el) => el.textContent?.includes("lib.rs"))).toBe(false);
  });

  it("moves to the next name starting with the letters typed", () => {
    const { container } = render(() => <FileTree folder={folder} />);
    const first = items(container)[0];
    key(first, "m");
    expect(document.activeElement?.textContent).toContain("main.rs");
    key(document.activeElement!, "m");
    expect(document.activeElement?.textContent).toContain("make.sh");
  });

  it("opens a file with Enter", () => {
    const { container } = render(() => <FileTree folder={folder} />);
    key(named(container, "main.rs"), "Enter");
    expect(h.openFile).toHaveBeenCalledWith("/w/main.rs");
  });
});

describe("loading, failed and empty (NV-16)", () => {
  it("says nothing while the folder is first read", () => {
    h.setState("entries", ROOT, undefined);
    h.setState("loading", ROOT, true);
    const { container } = render(() => <FileTree folder={folder} />);
    expect(container.textContent).toBe("");
  });

  it("says a folder it could not read could not be read, with Try again", () => {
    h.setState("entries", ROOT, undefined);
    h.setState("errors", ROOT, "Could not read this folder.");
    const { container } = render(() => <FileTree folder={folder} />);
    expect(container.textContent).toContain("Could not read this folder.");
    expect(container.textContent).not.toContain("Empty folder");
    loadDir.mockClear();
    fireEvent.click(container.querySelector<HTMLButtonElement>(".file-tree-retry-button")!);
    expect(loadDir).toHaveBeenCalledWith(ROOT);
  });

  it("says an empty folder is empty", () => {
    h.setState("entries", ROOT, []);
    const { container } = render(() => <FileTree folder={folder} />);
    expect(container.textContent).toBe("Empty folder");
  });

  it("puts a folder that could not be read beside its row, and reads it again from there", () => {
    h.setState("entries", ROOT, [dir("/w/locked")]);
    h.setState("errors", "/w/locked", "Could not read this folder.");
    treeExpansionStore.setExpanded(ROOT, "/w/locked", true);
    const { container } = render(() => <FileTree folder={folder} />);
    const line = named(container, "Could not read this folder.");
    expect(line.textContent).toContain("Try again");
    expect(line.getAttribute("aria-level")).toBe("2");
    loadDir.mockClear();
    key(line, "Enter");
    expect(loadDir).toHaveBeenCalledWith("/w/locked");
  });

  it("counts the entries of a folder it could not read", () => {
    h.setState("unreadable", ROOT, 3);
    const { container } = render(() => <FileTree folder={folder} />);
    const drawn = items(container);
    expect(drawn[drawn.length - 1].textContent).toBe("Could not read 3 items");
  });
});

describe("a menu on every row (NV-18)", () => {
  function menuOf(row: HTMLElement): string[] {
    fireEvent.contextMenu(row, { clientX: 5, clientY: 5 });
    return h.menu.map((item) => item.label);
  }

  it("offers the file manager and the path on a folder", () => {
    const { container } = render(() => <FileTree folder={folder} />);
    expect(menuOf(named(container, "src"))).toEqual(["Show in Finder", "Copy path"]);
    h.menu.find((item) => item.label === "Show in Finder")!.action();
    expect(revealEntry).toHaveBeenCalledWith("/w/src");
  });

  it("offers open, the file manager and the path on a file outside the notes folder", async () => {
    const { container } = render(() => <FileTree folder={folder} />);
    expect(menuOf(named(container, "main.rs"))).toEqual(["Open", "Show in Finder", "Copy path"]);
    h.menu.find((item) => item.label === "Copy path")!.action();
    await vi.waitFor(() => expect(h.copyText).toHaveBeenCalledWith("/w/main.rs"));
  });

  it("adds New file on a folder the notes folder holds", () => {
    treeExpansionStore.setExpanded(ROOT, "/w/notes", true);
    const { container } = render(() => <FileTree folder={folder} />);
    expect(menuOf(named(container, "Projects"))).toEqual([
      "New file",
      "Show in Finder",
      "Copy path",
    ]);
    h.menu.find((item) => item.label === "New file")!.action();
    expect(h.newNoteInFolder).toHaveBeenCalledWith("/w/notes/Projects");
  });

  it("adds Rename and Move to Trash on a note", () => {
    treeExpansionStore.setExpanded(ROOT, "/w/notes", true);
    const { container } = render(() => <FileTree folder={folder} />);
    expect(menuOf(named(container, "Plan"))).toEqual([
      "Open",
      "Rename",
      "Show in Finder",
      "Copy path",
      "Move to Trash",
    ]);
  });

  it("renames a note in its row", async () => {
    treeExpansionStore.setExpanded(ROOT, "/w/notes", true);
    const { container } = render(() => <FileTree folder={folder} />);
    menuOf(named(container, "Plan"));
    h.menu.find((item) => item.label === "Rename")!.action();

    const input = container.querySelector<HTMLInputElement>(".file-tree-rename-input")!;
    expect(input.value).toBe("Plan.md");
    input.value = "Launch.md";
    fireEvent.keyDown(input, { key: "Enter" });

    await vi.waitFor(() => expect(h.renameNoteAndLinks).toHaveBeenCalledWith("doc-1", "Launch.md"));
    expect(h.openFile).toHaveBeenCalledWith("/w/notes/Plan.md");
    expect(container.querySelector(".file-tree-rename-input")).toBeNull();
  });

  it("drops a rename on Escape", () => {
    treeExpansionStore.setExpanded(ROOT, "/w/notes", true);
    const { container } = render(() => <FileTree folder={folder} />);
    menuOf(named(container, "Plan"));
    h.menu.find((item) => item.label === "Rename")!.action();
    const input = container.querySelector<HTMLInputElement>(".file-tree-rename-input")!;
    input.value = "Other.md";
    fireEvent.keyDown(input, { key: "Escape" });
    expect(container.querySelector(".file-tree-rename-input")).toBeNull();
    expect(h.renameNoteAndLinks).not.toHaveBeenCalled();
  });
});
