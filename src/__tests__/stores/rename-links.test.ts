import { describe, it, expect, vi, beforeEach } from "vitest";
import type { BufferDocument } from "../../types/buffer";

// A rename says how many notes link to the one being renamed before it runs,
// and which notes it left alone after (spec L3). Both sentences are the whole
// promise: a link left pointing at a name no note answers to is the person's
// to fix, and they can only fix what they were told about.

function doc(overrides: Partial<BufferDocument> = {}): BufferDocument {
  return {
    id: "n-1",
    title: "Old note.md",
    filename: "n-1.txt",
    status: "active",
    language: null,
    source_path: "/notes/Old note.md",
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    closed_at: null,
    read_only: false,
    size_bytes: 0,
    line_ending: "lf",
    ...overrides,
  };
}

vi.mock("../../services/tauri", () => ({
  createBuffer: vi.fn(),
  newNote: vi.fn(),
  renameNote: vi.fn(),
  deleteNote: vi.fn(),
  saveNoteCopy: vi.fn(),
  showNoteInFileManager: vi.fn().mockResolvedValue(undefined),
  showNotesFileInFileManager: vi.fn().mockResolvedValue(undefined),
  listActiveBuffers: vi.fn().mockResolvedValue([]),
  listHistory: vi.fn().mockResolvedValue([]),
  closeBuffer: vi.fn().mockResolvedValue(undefined),
  closeBuffers: vi.fn().mockResolvedValue(undefined),
  deleteBuffer: vi.fn().mockResolvedValue(undefined),
  restoreBuffer: vi.fn().mockResolvedValue(undefined),
  clearHistory: vi.fn().mockResolvedValue(undefined),
  renameBuffer: vi.fn().mockResolvedValue(undefined),
  getBuffer: vi.fn(),
  openFile: vi.fn(),
  openFileConfirmed: vi.fn(),
  showOpenFileDialog: vi.fn().mockResolvedValue(null),
  readBufferContent: vi.fn().mockResolvedValue(""),
  previewClose: vi.fn().mockResolvedValue(undefined),
  countLinksTo: vi.fn(),
  renameNoteWithLinks: vi.fn(),
  undoRenameWithLinks: vi.fn(),
}));

vi.mock("../../services/autosave", () => ({
  flushAutosave: vi.fn().mockResolvedValue({ ok: true, failures: [] }),
  cancelAutosave: vi.fn(),
  onAutosaveStart: vi.fn(() => () => {}),
  onAutosaveSuccess: vi.fn(() => () => {}),
  onAutosaveError: vi.fn(() => () => {}),
}));

const chose = vi.fn();
vi.mock("../../components/ConfirmDialog/ConfirmDialog", () => ({
  requestChoice: (options: unknown) => chose(options),
  requestConfirm: (options: unknown) =>
    chose(options).then((answer: string) => answer === "confirm"),
}));

vi.mock("../../components/Notifications/Toast", () => ({
  showToast: vi.fn(),
}));

const reloaded = vi.fn();
const dirty = vi.fn().mockReturnValue(false);
vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: {
    getActive: () => ({
      editor: {
        isDirty: (id: string) => dirty(id),
        requestExternalReload: (id: string) => reloaded(id),
      },
    }),
  },
}));

vi.mock("../../stores/global/notes", () => ({
  notesStore: {
    root: () => "/notes",
    load: vi.fn(),
    contains: (path: string) => path.startsWith("/notes/"),
  },
}));

import { bufferRegistry } from "../../stores/global/buffer-registry";
import { renameLinksStore } from "../../stores/global/rename-links";
import { renameNoteAndLinks } from "../../lib/note-actions";
import * as api from "../../services/tauri";

const mockedApi = vi.mocked(api);

const RENAMED = {
  renamed_path: "/notes/New note.md",
  updated: 1,
  updated_paths: ["/notes/First.md"],
  skipped: [],
};

describe("a rename that carries the links", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    dirty.mockReturnValue(false);
    mockedApi.listActiveBuffers.mockResolvedValue([
      doc(),
      doc({ id: "n-2", title: "First.md", source_path: "/notes/First.md" }),
    ]);
    mockedApi.listHistory.mockResolvedValue([]);
    mockedApi.getBuffer.mockResolvedValue(
      doc({ title: "New note.md", source_path: "/notes/New note.md" }),
    );
    await bufferRegistry.load();
    renameLinksStore.clearSkipped();
  });

  it("asks with the count before it renames", async () => {
    mockedApi.countLinksTo.mockResolvedValue(3);
    chose.mockResolvedValueOnce("confirm");
    mockedApi.renameNoteWithLinks.mockResolvedValue(RENAMED);

    await renameNoteAndLinks("n-1", "New note");

    expect(mockedApi.countLinksTo).toHaveBeenCalledWith("/notes/Old note.md");
    const asked = chose.mock.calls[0][0] as { title: string };
    expect(asked.title).toBe("3 notes link here. Update them?");
    expect(mockedApi.renameNoteWithLinks).toHaveBeenCalledWith(
      "/notes/Old note.md",
      "New note",
      true,
    );
  });

  it("asks about one note in the singular", async () => {
    mockedApi.countLinksTo.mockResolvedValue(1);
    chose.mockResolvedValueOnce("confirm");
    mockedApi.renameNoteWithLinks.mockResolvedValue(RENAMED);

    await renameNoteAndLinks("n-1", "New note");

    const asked = chose.mock.calls[0][0] as { title: string };
    expect(asked.title).toBe("1 note links here. Update it?");
  });

  it("renames without touching the links when the offer is declined", async () => {
    mockedApi.countLinksTo.mockResolvedValue(2);
    chose.mockResolvedValueOnce("cancel");
    mockedApi.renameNoteWithLinks.mockResolvedValue({
      ...RENAMED,
      updated: 0,
      updated_paths: [],
    });

    await renameNoteAndLinks("n-1", "New note");

    expect(mockedApi.renameNoteWithLinks).toHaveBeenCalledWith(
      "/notes/Old note.md",
      "New note",
      false,
    );
  });

  it("renames nothing when the offer is dismissed", async () => {
    mockedApi.countLinksTo.mockResolvedValue(3);
    chose.mockResolvedValueOnce("dismissed");

    await renameNoteAndLinks("n-1", "New note");

    expect(mockedApi.renameNoteWithLinks).not.toHaveBeenCalled();
  });

  it("renames nothing when the count cannot be read", async () => {
    mockedApi.countLinksTo.mockRejectedValue("the index is not there");

    await expect(renameNoteAndLinks("n-1", "New note")).rejects.toBe(
      "the index is not there",
    );

    expect(chose).not.toHaveBeenCalled();
    expect(mockedApi.renameNoteWithLinks).not.toHaveBeenCalled();
  });

  it("asks nothing when no note links here", async () => {
    mockedApi.countLinksTo.mockResolvedValue(0);
    mockedApi.renameNoteWithLinks.mockResolvedValue({
      ...RENAMED,
      updated: 0,
      updated_paths: [],
    });

    await renameNoteAndLinks("n-1", "New note");

    expect(chose).not.toHaveBeenCalled();
    expect(mockedApi.renameNoteWithLinks).toHaveBeenCalledWith(
      "/notes/Old note.md",
      "New note",
      false,
    );
  });

  it("keeps the notes it left alone, by name", async () => {
    mockedApi.countLinksTo.mockResolvedValue(3);
    chose.mockResolvedValueOnce("confirm");
    mockedApi.renameNoteWithLinks.mockResolvedValue({
      renamed_path: "/notes/New note.md",
      updated: 1,
      updated_paths: ["/notes/First.md"],
      skipped: [
        { path: "/notes/Second.md", reason: "ERR_FILE_NOT_DOWNLOADED" },
        { path: "/notes/Third.md", reason: "ERR_READ_ONLY_DESTINATION" },
      ],
    });

    await renameNoteAndLinks("n-1", "New note");

    expect(renameLinksStore.skippedNames()).toEqual([
      "Second.md",
      "Third.md",
    ]);
  });

  it("re-reads a tab whose file it rewrote, and leaves an edited one alone", async () => {
    mockedApi.countLinksTo.mockResolvedValue(1);
    chose.mockResolvedValueOnce("confirm");
    mockedApi.renameNoteWithLinks.mockResolvedValue(RENAMED);

    await renameNoteAndLinks("n-1", "New note");
    expect(reloaded).toHaveBeenCalledWith("n-2");

    reloaded.mockClear();
    dirty.mockReturnValue(true);
    await renameNoteAndLinks("n-1", "New note");
    expect(reloaded).not.toHaveBeenCalled();
  });

  it("puts the rename back over the files it rewrote and no others", async () => {
    mockedApi.countLinksTo.mockResolvedValue(1);
    chose.mockResolvedValueOnce("confirm");
    mockedApi.renameNoteWithLinks.mockResolvedValue(RENAMED);
    await renameNoteAndLinks("n-1", "New note");

    expect(renameLinksStore.canUndo()).toBe(true);
    mockedApi.undoRenameWithLinks.mockResolvedValue({
      renamed_path: "/notes/Old note.md",
      updated: 1,
      updated_paths: ["/notes/First.md"],
      skipped: [],
    });

    await renameLinksStore.undoRename();

    expect(mockedApi.undoRenameWithLinks).toHaveBeenCalledWith(
      "/notes/New note.md",
      "Old note",
      ["/notes/First.md"],
    );
    expect(renameLinksStore.canUndo()).toBe(false);
  });
});
