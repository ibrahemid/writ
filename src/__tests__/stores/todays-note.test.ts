import { describe, it, expect, vi, beforeEach } from "vitest";
import type { BufferDocument } from "../../types/buffer";

// `Today's note` (spec D1) as the frontend runs it: the command reaches the
// backend once per invocation, and the file keeps the sortable name whatever
// the tab is captioned with.

function doc(overrides: Partial<BufferDocument> = {}): BufferDocument {
  return {
    id: "today-1",
    title: "2026-09-07.md",
    filename: "today-1.txt",
    status: "active",
    language: null,
    source_path: "/notes/2026-09-07.md",
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
  todaysNote: vi.fn(),
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

vi.mock("../../components/ConfirmDialog/ConfirmDialog", () => ({
  requestConfirm: vi.fn(),
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
import {
  registerCommand,
  unregisterCommand,
  executeCommand,
} from "../../commands/registry";
import * as api from "../../services/tauri";

const mockedApi = vi.mocked(api);

describe("today's note", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockedApi.todaysNote.mockResolvedValue(doc());
  });

  it("reaches the backend once per invocation", async () => {
    const ran: Promise<unknown>[] = [];
    registerCommand({
      id: "note.today",
      label: "Today's note",
      scope: "app",
      execute: () => {
        ran.push(bufferRegistry.todaysNote());
      },
    });

    executeCommand("note.today");
    executeCommand("note.today");
    await Promise.all(ran);
    unregisterCommand("note.today");

    expect(mockedApi.todaysNote).toHaveBeenCalledTimes(2);
    expect(ran).toHaveLength(2);
  });

  it("holds one entry for the day however often it is asked for", async () => {
    await bufferRegistry.todaysNote();
    await bufferRegistry.todaysNote();

    const today = bufferRegistry.buffers().filter((b) => b.id === "today-1");
    expect(today).toHaveLength(1);
  });

  it("keeps the sortable file name under a tab captioned for the reader", async () => {
    // The backend is free to caption the tab in the reader's own calendar;
    // the file it hands back is still named for a name that sorts.
    mockedApi.todaysNote.mockResolvedValue(
      doc({ title: "7 September 2026", source_path: "/notes/2026-09-07.md" }),
    );

    const note = await bufferRegistry.todaysNote();

    expect(note.title).toBe("7 September 2026");
    expect(note.source_path?.split("/").pop()).toBe("2026-09-07.md");
    expect(note.source_path?.split("/").pop()).toMatch(/^\d{4}-\d{2}-\d{2}\.md$/);
  });
});
