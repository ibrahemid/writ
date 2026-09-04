import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";
import type { BufferDocument } from "../../types/buffer";
import type { MenuItem } from "../../components/ContextMenu/ContextMenu";

// Renaming a tab renames the note's file and Delete moves it to the Trash, so
// both can be stopped. What the backend decides has to reach the person who
// asked, and an action Writ would stop is never offered.

beforeAll(() => {
  Object.defineProperty(window, "matchMedia", {
    writable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  });
  if (!Element.prototype.scrollIntoView) {
    Element.prototype.scrollIntoView = vi.fn();
  }
});

function doc(id: string, sourcePath: string | null): BufferDocument {
  return {
    id,
    title: sourcePath?.split("/").pop() ?? id,
    filename: `${id}.txt`,
    status: "active",
    language: null,
    source_path: sourcePath,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    closed_at: null,
    read_only: false,
    size_bytes: 0,
    line_ending: "lf",
  };
}

const NOTES = doc("mine", "/notes/2026-08-29.md");
const ELSEWHERE = doc("theirs", "/somebody/else.md");

const mocks = vi.hoisted(() => ({
  newNote: vi.fn(),
  setActiveTabId: vi.fn(),
  closeTab: vi.fn(),
  closeOtherTabs: vi.fn(),
  closeAllTabs: vi.fn(),
  renameBuffer: vi.fn(),
  activeTabId: vi.fn(() => "mine" as string | null),
  buffers: vi.fn(() => [] as BufferDocument[]),
  showContextMenu: vi.fn(),
  showToast: vi.fn(),
}));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    tabs: {
      activeTabId: mocks.activeTabId,
      setActiveTabId: mocks.setActiveTabId,
      closeTab: mocks.closeTab,
      closeOtherTabs: mocks.closeOtherTabs,
      closeAllTabs: mocks.closeAllTabs,
      newNote: mocks.newNote,
    },
    // No note is waiting on a sync provider in these cases.
    downloads: {
      pending: () => [],
      selectedPath: () => null,
      select: () => {},
      cancel: async () => {},
      close: () => {},
    },
  }),
}));

vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    activeTabs: mocks.buffers,
    buffers: mocks.buffers,
    renameBuffer: mocks.renameBuffer,
  },
}));

vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: { getActive: () => ({ tabs: { activeTabId: mocks.activeTabId } }) },
}));

// The real containment rule, with a notes folder the test controls.
vi.mock("../../stores/global/notes", () => ({
  notesStore: {
    root: () => "/notes",
    load: vi.fn(),
    contains: (path: string) => path.startsWith("/notes/"),
  },
}));

vi.mock("../../components/ContextMenu/ContextMenu", () => ({
  showContextMenu: mocks.showContextMenu,
}));

vi.mock("../../components/Notifications/Toast", () => ({
  showToast: mocks.showToast,
}));

vi.mock("../../services/tauri", () => ({
  showNoteInFileManager: vi.fn().mockResolvedValue(undefined),
  showNotesFileInFileManager: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../components/ConfirmDialog/ConfirmDialog", () => ({
  requestConfirm: vi.fn().mockResolvedValue(false),
}));

import TabBar from "../../components/Editor/TabBar";

function menuFor(tabIndex: number): MenuItem[] {
  const tab = document.querySelectorAll<HTMLButtonElement>(".tab")[tabIndex];
  fireEvent.contextMenu(tab);
  const calls = mocks.showContextMenu.mock.calls;
  return calls[calls.length - 1][2] as MenuItem[];
}

async function submitRename(value: string) {
  const tab = document.querySelector<HTMLButtonElement>(".tab")!;
  fireEvent.dblClick(tab);
  const input = await waitFor(() => {
    const found = document.querySelector<HTMLInputElement>(".tab input");
    expect(found).not.toBeNull();
    return found!;
  });
  fireEvent.input(input, { target: { value } });
  fireEvent.keyDown(input, { key: "Enter" });
}

describe("TabBar note actions", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.buffers.mockReturnValue([NOTES]);
    mocks.activeTabId.mockReturnValue("mine");
    mocks.renameBuffer.mockResolvedValue(undefined);
  });

  afterEach(cleanup);

  it("says so when a name is empty", async () => {
    mocks.renameBuffer.mockRejectedValue("That name is empty.");
    render(() => <TabBar />);

    await submitRename("   ");

    // The backend decides what an empty name is; the tab bar does not swallow
    // the submit before it gets there.
    expect(mocks.renameBuffer).toHaveBeenCalledWith("mine", "   ");
    await waitFor(() =>
      expect(mocks.showToast).toHaveBeenCalledWith("That name is empty.", "error"),
    );
  });

  it("names the note a rename would have collided with", async () => {
    mocks.renameBuffer.mockRejectedValue('A note named "Grocery list.md" is already there.');
    render(() => <TabBar />);

    await submitRename("Grocery list");

    await waitFor(() =>
      expect(mocks.showToast).toHaveBeenCalledWith(
        'A note named "Grocery list.md" is already there.',
        "error",
      ),
    );
  });

  it("says the file changed rather than naming a copy no rename wrote", async () => {
    mocks.renameBuffer.mockRejectedValue(
      "ERR_FILE_CHANGED_ON_DISK: the file changed on disk: /notes/2026-08-29.md",
    );
    render(() => <TabBar />);

    await submitRename("Grocery list");

    await waitFor(() =>
      expect(mocks.showToast).toHaveBeenCalledWith(
        "The file changed outside Writ, so it was not renamed.",
        "error",
      ),
    );
  });

  it("closes the editor and says nothing when the rename lands", async () => {
    render(() => <TabBar />);

    await submitRename("Grocery list");

    expect(mocks.renameBuffer).toHaveBeenCalledWith("mine", "Grocery list");
    await waitFor(() => expect(document.querySelector(".tab input")).toBeNull());
    expect(mocks.showToast).not.toHaveBeenCalled();
  });

  it("offers Delete for a note in the notes folder", () => {
    render(() => <TabBar />);

    const remove = menuFor(0).find((item) => item.label === "Delete");

    expect(remove).toBeDefined();
    expect(remove!.disabled).toBe(false);
  });

  it("never offers Delete for a file opened from somebody else's folder", () => {
    mocks.buffers.mockReturnValue([ELSEWHERE]);
    render(() => <TabBar />);

    const remove = menuFor(0).find((item) => item.label === "Delete");

    expect(remove).toBeDefined();
    expect(remove!.disabled).toBe(true);
  });
});
