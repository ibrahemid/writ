import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import type { BufferDocument, TodaysFile } from "../../types/buffer";

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
    encoding: "UTF-8",
    ...overrides,
  };
}

// Today's file as the backend answers it when the file opens.
function opened(row: BufferDocument): TodaysFile {
  return {
    path: row.source_path ?? "",
    opened: { doc: row, mode: { kind: "Normal" }, size_bytes: row.size_bytes },
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
  restoreBuffer: vi.fn().mockResolvedValue({
    doc: null,
    mode: { kind: "Refused", reason: "ERR_OPEN_TOO_LARGE" },
    size_bytes: 0,
  }),
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

vi.mock("../../stores/global/dialogs", () => ({
  requestConfirm: vi.fn(),
}));

vi.mock("../../stores/global/notifications", () => ({
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
import { showToast } from "../../stores/global/notifications";
import { largeFileOpen } from "../../stores/global/large-file-open";
import { createTabStore } from "../../stores/window/tab-store";
import type { DownloadStore } from "../../stores/window/download-store";

const mockedApi = vi.mocked(api);

describe("today's note", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockedApi.todaysNote.mockResolvedValue(opened(doc()));
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
      opened(doc({ title: "7 September 2026", source_path: "/notes/2026-09-07.md" })),
    );

    const outcome = await bufferRegistry.todaysNote();
    if (outcome.kind !== "opened") throw new Error(`expected opened, got ${outcome.kind}`);
    const note = outcome.doc;

    expect(note.title).toBe("7 September 2026");
    expect(note.source_path?.split("/").pop()).toBe("2026-09-07.md");
    expect(note.source_path?.split("/").pop()).toMatch(/^\d{4}-\d{2}-\d{2}\.md$/);
  });
});

describe("today's file answers like any open", () => {
  const PATH = "/notes/2026-10-13.txt";
  const SIXTY_MB = 60_000_000;

  function tabStore() {
    const downloads = {
      select: vi.fn(),
      start: vi.fn(async () => undefined),
    } as unknown as DownloadStore;
    return { tabs: createTabStore({ registry: bufferRegistry, downloads }), downloads };
  }

  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    for (let i = 0; i < 20 && largeFileOpen.state() !== null; i++) {
      largeFileOpen.respond(false);
      largeFileOpen.finish();
    }
  });

  it("asks about a day's file over the size Writ opens without asking, then opens it", async () => {
    mockedApi.todaysNote.mockResolvedValue({
      path: PATH,
      opened: { doc: null, mode: { kind: "LargeFileConfirm" }, size_bytes: SIXTY_MB },
    });
    mockedApi.openFileConfirmed.mockResolvedValue({
      doc: doc({ id: "today-big", source_path: PATH, size_bytes: SIXTY_MB }),
      mode: { kind: "LargeFile" },
      size_bytes: SIXTY_MB,
    });
    const { tabs } = tabStore();

    const opening = tabs.todaysNote();
    await vi.waitFor(() =>
      expect(largeFileOpen.state()).toEqual({
        phase: "asking",
        name: "2026-10-13.txt",
        sizeBytes: SIXTY_MB,
      }),
    );
    expect(mockedApi.openFileConfirmed).not.toHaveBeenCalled();
    largeFileOpen.respond(true);

    expect((await opening)?.id).toBe("today-big");
    expect(mockedApi.openFileConfirmed).toHaveBeenCalledWith(PATH);
    expect(tabs.activeTabId()).toBe("today-big");
  });

  it("says once why a day's file over the size Writ opens was not opened", async () => {
    mockedApi.todaysNote.mockResolvedValue({
      path: PATH,
      opened: {
        doc: null,
        mode: { kind: "Refused", reason: "ERR_OPEN_TOO_LARGE: over the limit" },
        size_bytes: 600_000_000,
      },
    });
    const { tabs } = tabStore();

    expect(await tabs.todaysNote()).toBeNull();

    expect(showToast).toHaveBeenCalledTimes(1);
    expect(showToast).toHaveBeenCalledWith(
      "Could not open 2026-10-13.txt: it is 600 MB, and Writ opens files up to 524 MB.",
      "error",
    );
    expect(tabs.activeTabId()).toBeNull();
  });

  it("downloads a day's file whose bytes are elsewhere", async () => {
    mockedApi.todaysNote.mockResolvedValue({
      path: PATH,
      opened: {
        doc: null,
        mode: { kind: "NotDownloaded", path: PATH, provider: "iCloud Drive" },
        size_bytes: 0,
      },
    });
    const { tabs, downloads } = tabStore();

    expect(await tabs.todaysNote()).toBeNull();

    expect(downloads.start).toHaveBeenCalledWith({
      path: PATH,
      title: "2026-10-13.txt",
      provider: "iCloud Drive",
    });
    expect(showToast).not.toHaveBeenCalled();
  });

  it("selects the day's tab when its file opens", async () => {
    mockedApi.todaysNote.mockResolvedValue({
      path: PATH,
      opened: { doc: doc({ id: "today-2", source_path: PATH }), mode: { kind: "Normal" }, size_bytes: 0 },
    });
    const { tabs } = tabStore();

    expect((await tabs.todaysNote())?.id).toBe("today-2");
    expect(tabs.activeTabId()).toBe("today-2");
  });
});
