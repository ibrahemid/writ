import { describe, it, expect, vi, beforeEach } from "vitest";
import type { BufferDocument } from "../../types/buffer";

// The note operations as the frontend performs them (ADR-028 §3): the file
// moves in the backend, and the row it hands back is what the registry keeps.

function doc(overrides: Partial<BufferDocument> = {}): BufferDocument {
  return {
    id: "n-1",
    title: "2026-08-29.md",
    filename: "n-1.txt",
    status: "active",
    language: null,
    source_path: "/notes/2026-08-29.md",
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
  openFile: vi.fn(),
  openFileConfirmed: vi.fn(),
  showOpenFileDialog: vi.fn().mockResolvedValue(null),
  readBufferContent: vi.fn().mockResolvedValue(""),
  previewClose: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../services/autosave", () => ({
  flushAutosave: vi.fn().mockResolvedValue({ ok: true, failures: [] }),
  cancelAutosave: vi.fn(),
  onAutosaveStart: vi.fn(() => () => {}),
  onAutosaveSuccess: vi.fn(() => () => {}),
  onAutosaveError: vi.fn(() => () => {}),
}));

const confirmed = vi.fn();
vi.mock("../../components/ConfirmDialog/ConfirmDialog", () => ({
  requestConfirm: (options: unknown) => confirmed(options),
}));

vi.mock("../../components/Notifications/Toast", () => ({
  showToast: vi.fn(),
}));

vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: { getActive: () => null },
}));

vi.mock("../../stores/global/notes", () => ({
  notesStore: {
    root: () => "/notes",
    load: vi.fn(),
    contains: (path: string) => path.startsWith("/notes/"),
  },
}));

import { bufferRegistry } from "../../stores/global/buffer-registry";
import { confirmAndDeleteNote, noteIsDeletable } from "../../lib/note-actions";
import * as api from "../../services/tauri";
import { cancelAutosave } from "../../services/autosave";

const mockedApi = vi.mocked(api);

describe("note operations", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    mockedApi.listActiveBuffers.mockResolvedValue([doc()]);
    mockedApi.listHistory.mockResolvedValue([]);
    await bufferRegistry.load();
  });

  it("rename_calls_rename_note_and_updates_source_path", async () => {
    mockedApi.renameNote.mockResolvedValue(
      doc({ title: "Grocery list.md", source_path: "/notes/Grocery list.md" }),
    );

    await bufferRegistry.renameBuffer("n-1", "Grocery list");

    expect(mockedApi.renameNote).toHaveBeenCalledWith("n-1", "Grocery list");
    expect(mockedApi.renameBuffer).not.toHaveBeenCalled();
    const after = bufferRegistry.buffers().find((b) => b.id === "n-1");
    expect(after?.title).toBe("Grocery list.md");
    expect(after?.source_path).toBe("/notes/Grocery list.md");
  });

  it("delete_confirms_before_trashing", async () => {
    confirmed.mockResolvedValueOnce(false);
    await confirmAndDeleteNote("n-1");
    expect(mockedApi.deleteNote).not.toHaveBeenCalled();
    expect(bufferRegistry.buffers().map((b) => b.id)).toEqual(["n-1"]);

    const asked = confirmed.mock.calls[0][0] as { title: string };
    expect(asked.title).toBe('Move "2026-08-29.md" to the Trash?');

    confirmed.mockResolvedValueOnce(true);
    mockedApi.deleteNote.mockResolvedValue(undefined);
    await confirmAndDeleteNote("n-1");

    expect(mockedApi.deleteNote).toHaveBeenCalledWith("n-1");
    expect(vi.mocked(cancelAutosave)).toHaveBeenCalledWith("n-1");
    expect(bufferRegistry.buffers()).toEqual([]);
  });

  it("only_a_note_in_the_notes_folder_may_be_deleted", async () => {
    mockedApi.listActiveBuffers.mockResolvedValue([
      doc(),
      doc({ id: "theirs", source_path: "/somebody/else.md" }),
      doc({ id: "unwritten", source_path: null }),
    ]);
    await bufferRegistry.load();

    expect(noteIsDeletable("n-1")).toBe(true);
    expect(noteIsDeletable("theirs")).toBe(false);
    // A note that never reached a file has nothing outside the folder.
    expect(noteIsDeletable("unwritten")).toBe(true);
    expect(noteIsDeletable("no-such-note")).toBe(false);

    confirmed.mockResolvedValue(true);
    await confirmAndDeleteNote("theirs");
    expect(mockedApi.deleteNote).not.toHaveBeenCalled();
  });

  it("save_copy_returns_the_path_the_copy_was_written_to", async () => {
    mockedApi.saveNoteCopy.mockResolvedValue("/notes/report.md");

    const path = await bufferRegistry.saveCopy("n-1", "the text");

    expect(mockedApi.saveNoteCopy).toHaveBeenCalledWith("n-1", "the text");
    expect(path).toBe("/notes/report.md");
    expect(bufferRegistry.buffers().find((b) => b.id === "n-1")?.source_path).toBe(
      "/notes/2026-08-29.md",
    );
  });

  it("new_note_registers_the_note_the_backend_created", async () => {
    mockedApi.newNote.mockResolvedValue(doc({ id: "n-2", title: "2026-08-30.md" }));

    const created = await bufferRegistry.newNote();

    expect(mockedApi.newNote).toHaveBeenCalledTimes(1);
    expect(mockedApi.createBuffer).not.toHaveBeenCalled();
    expect(created.id).toBe("n-2");
    expect(bufferRegistry.activeTabs().map((b) => b.id)).toEqual(["n-1", "n-2"]);
  });
});
