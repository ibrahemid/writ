import { describe, it, expect, vi, beforeEach } from "vitest";
import type { BufferDocument } from "../../types/buffer";

// A read whose text no tab takes goes through peekBufferContent, which leaves
// the record of the file where the tab left it. Through readBufferContent it
// moves the record to bytes the tab never holds: the tab's switch back then
// restores its old text unread, and its next save passes the guard over the
// other program's change.

function doc(overrides: Partial<BufferDocument> = {}): BufferDocument {
  return {
    id: "n-1",
    title: "Background.md",
    filename: "n-1.txt",
    status: "active",
    language: null,
    source_path: "/notes/Background.md",
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    closed_at: null,
    read_only: false,
    size_bytes: 0,
    line_ending: "lf",
    encoding: "UTF-8",
    ...overrides,
  };
}

vi.mock("../../services/tauri", () => ({
  saveNoteCopy: vi.fn(),
  listActiveBuffers: vi.fn().mockResolvedValue([]),
  listHistory: vi.fn().mockResolvedValue([]),
  getBuffer: vi.fn(),
  readBufferContent: vi.fn().mockResolvedValue("what the tab read"),
  peekBufferContent: vi.fn().mockResolvedValue("first line\n[[Renamed note]] in another program's text\n"),
}));

vi.mock("../../services/autosave", () => ({
  flushAutosave: vi.fn().mockResolvedValue({ ok: true, failures: [] }),
  cancelAutosave: vi.fn(),
  onAutosaveStart: vi.fn(() => () => {}),
  onAutosaveSuccess: vi.fn(() => () => {}),
  onAutosaveError: vi.fn(() => () => {}),
}));

vi.mock("../../stores/global/notifications", () => ({
  showToast: vi.fn(),
}));

const h = vi.hoisted(() => ({
  openFile: vi.fn(async () => {}),
  openPath: vi.fn(),
  requestReveal: vi.fn(),
}));

vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: {
    getActive: () => ({
      editor: {
        currentBufferId: () => "n-2",
        getActiveText: () => ({ text: "the note in front" }),
        textOfRemoved: () => null,
        requestReveal: h.requestReveal,
      },
      tabs: { openFile: h.openFile, openPath: h.openPath },
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
import { saveCopyOfNote } from "../../lib/note-actions";
import * as api from "../../services/tauri";

const mockedApi = vi.mocked(api);

describe("a read whose text no tab takes", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    mockedApi.listActiveBuffers.mockResolvedValue([doc(), doc({ id: "n-2", title: "Front.md" })]);
    mockedApi.saveNoteCopy.mockResolvedValue("/notes/Background copy.md");
    await bufferRegistry.load();
  });

  it("copies a background note from its file without moving the tab's record of it", async () => {
    await saveCopyOfNote("n-1");

    expect(mockedApi.peekBufferContent).toHaveBeenCalledWith("n-1");
    expect(mockedApi.readBufferContent).not.toHaveBeenCalled();
    expect(mockedApi.saveNoteCopy).toHaveBeenCalledWith(
      "n-1",
      "first line\n[[Renamed note]] in another program's text\n",
    );
    expect(h.openFile).toHaveBeenCalledWith("/notes/Background copy.md");
  });

  it("copies the note in front from its view and reads nothing", async () => {
    await saveCopyOfNote("n-2");

    expect(mockedApi.peekBufferContent).not.toHaveBeenCalled();
    expect(mockedApi.readBufferContent).not.toHaveBeenCalled();
    expect(mockedApi.saveNoteCopy).toHaveBeenCalledWith("n-2", "the note in front");
  });

  it("looks up a stale link in a note the rename left alone without moving its tab's record", async () => {
    h.openPath.mockResolvedValue({ kind: "opened", doc: doc() });

    await renameLinksStore.openSkipped({
      name: "Background.md",
      path: "/notes/Background.md",
      reason: "it changed while the rename ran",
      linkName: "Renamed note",
    });

    expect(mockedApi.peekBufferContent).toHaveBeenCalledWith("n-1");
    expect(mockedApi.readBufferContent).not.toHaveBeenCalled();
    expect(h.requestReveal).toHaveBeenCalledWith("n-1", 2);
  });
});
