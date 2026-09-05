import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, waitFor } from "@solidjs/testing-library";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";

// A note whose file changed under text no file holds writes nothing until the
// bar is answered. Every write while it is up reaches the guard, the guard
// refuses it, and each refusal leaves another dated copy beside the note, so a
// tab left writing puts a file into the notes folder for every pause in
// typing. Nothing here counts copies: they come only from the guard, which is
// only reached through `saveBufferContent`, so a write that never reaches the
// IPC never writes one.
//
// Driven over a real editor store, a real editor view, the real bar and the
// wiring the app runs, because the question is which of those lets a write
// through.

const api = vi.hoisted(() => ({
  saveBufferContent: vi.fn(),
  resolveExternalChange: vi.fn(),
  noteDiskState: vi.fn(async () => ({ state: "no_file" })),
  recordUnsavedNotes: vi.fn(async () => {}),
}));

vi.mock("../../services/tauri", () => ({
  ...api,
  listActiveBuffers: vi.fn().mockResolvedValue([]),
  listHistory: vi.fn().mockResolvedValue([]),
}));
vi.mock("../../components/Notifications/Toast", () => ({ showToast: vi.fn() }));
vi.mock("../../components/ConfirmDialog/ConfirmDialog", () => ({
  requestConfirm: vi.fn(),
}));
vi.mock("../../lib/log", () => ({ logFailure: vi.fn() }));

const active = vi.hoisted(() => ({
  win: null as { editor: unknown; tabs: { openFile: unknown } } | null,
}));

vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: { getActive: () => active.win },
}));
vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => active.win,
}));

import { createEditorStore } from "../../stores/window/editor-store";
import { createExternalEditDeps } from "../../lib/external-edit-deps";
import {
  handleExternalEdit,
  type ExternalChange,
} from "../../services/external-edit";
import { saveStatusStore } from "../../stores/global/save-status";
import { resolveNoteChange } from "../../lib/note-actions";
import { resetAutosave } from "../../services/autosave";
import FileChangedBar from "../../components/Editor/FileChangedBar";

const NOTE = "n1";
const MINE = "the text only the tab has\n";
const REFUSED = "ERR_FILE_CHANGED_ON_DISK";
const ANSWERED = { content: null, disk_hash: "h", conflict_copy_path: null };

let stores: Array<{ stopSaveListener: () => void }> = [];
let views: EditorView[] = [];

/** A note in a live view, wired as the window, with the watcher on the line. */
function openNote() {
  const store = createEditorStore();
  stores.push(store);
  const view = new EditorView({
    state: EditorState.create({ doc: MINE }),
    parent: document.body,
  });
  views.push(view);
  store.registerView(view);
  store.setCurrentBufferId(NOTE);
  active.win = { editor: store, tabs: { openFile: vi.fn() } };

  const deps = createExternalEditDeps({
    editor: store,
    openBuffers: () => [{ id: NOTE, title: "note", filename: "note.md" }],
    refreshBuffer: async () => {},
    forgetSaveStatus: (id) => saveStatusStore.forgetNote(id),
    cancelAutosave: (id) => store.cancelAutosave(id),
  });

  /** What the watcher reports, through the path the subscription takes. */
  async function reports(change: ExternalChange) {
    await handleExternalEdit(
      { bufferId: NOTE, change, path: "/notes/note.md" },
      deps,
    );
  }

  return { store, view, reports };
}

function buttons(container: HTMLElement): HTMLButtonElement[] {
  return [
    ...container.querySelectorAll<HTMLButtonElement>(".file-changed-bar-action"),
  ];
}

beforeEach(() => {
  vi.clearAllMocks();
  resetAutosave();
  saveStatusStore.reset();
  api.saveBufferContent.mockResolvedValue(null);
  api.noteDiskState.mockResolvedValue({ state: "no_file" });
});

afterEach(() => {
  cleanup();
  for (const store of stores) store.stopSaveListener();
  stores = [];
  for (const view of views) view.destroy();
  views = [];
  active.win = null;
  resetAutosave();
  saveStatusStore.reset();
});

describe("a note that is waiting for an answer about its file", () => {
  it("writes nothing however long the person keeps typing", async () => {
    const { store, reports } = openNote();
    await reports("modified");
    expect(store.noteFileState(NOTE)).toBe("changed");

    vi.useFakeTimers();
    try {
      for (let keystroke = 0; keystroke < 5; keystroke += 1) {
        store.scheduleAutosave(NOTE, `${MINE}${keystroke}\n`, 1000);
        vi.advanceTimersByTime(1000);
      }
      vi.advanceTimersByTime(5000);
    } finally {
      vi.useRealTimers();
    }

    expect(api.saveBufferContent).not.toHaveBeenCalled();

    // The queue is empty as well as held, so the flushes on quit, on blur and
    // on a tab switch have nothing to write either.
    await store.flushAutosave();
    expect(api.saveBufferContent).not.toHaveBeenCalled();
    expect(store.noteFileState(NOTE)).toBe("changed");
  });

  it("sends an explicit save to the question instead of the file", async () => {
    const { store, reports } = openNote();
    await reports("modified");

    const { container } = render(() => <FileChangedBar noteId={NOTE} />);
    await waitFor(() => expect(buttons(container)).toHaveLength(3));

    // The person carried on typing, so the focus is back in the editor and
    // the keystroke is theirs to spend.
    store.focusEditor();
    expect(document.activeElement).not.toBe(buttons(container)[0]);

    const result = await store.saveActiveBuffer();

    expect(api.saveBufferContent).not.toHaveBeenCalled();
    expect(result).toEqual({ ok: true, failures: [] });
    expect(store.noteFileState(NOTE)).toBe("changed");
    await waitFor(() =>
      expect(document.activeElement).toBe(buttons(container)[0]),
    );
  });

  it("holds a retry of the write the question is about", async () => {
    const { store, reports } = openNote();
    const refused = deferredWrite();

    void store.saveActiveBuffer();
    await waitFor(() => expect(api.saveBufferContent).toHaveBeenCalledTimes(1));
    await reports("modified");
    refused.reject(REFUSED);
    await waitFor(() =>
      expect(saveStatusStore.failureFor(NOTE)).toBeDefined(),
    );

    api.saveBufferContent.mockResolvedValue(null);
    await store.retrySave(NOTE);

    expect(api.saveBufferContent).toHaveBeenCalledTimes(1);
  });

  it("writes again once the question is answered", async () => {
    const { store, reports } = openNote();
    await reports("modified");
    api.resolveExternalChange.mockResolvedValue(ANSWERED);

    await resolveNoteChange(NOTE, "keep_mine");

    expect(api.resolveExternalChange).toHaveBeenCalledTimes(1);
    expect(store.noteFileState(NOTE)).toBe("present");

    store.scheduleAutosave(NOTE, "what was typed after the answer\n", 0);
    await waitFor(() => expect(api.saveBufferContent).toHaveBeenCalledTimes(1));
  });

  // A save already on the wire when the watcher reports is the one write the
  // bar cannot hold: cancelling the queue cannot cancel a call that has left.
  // Its failure is about the change the question is asking about, so the
  // answer takes it with it rather than leaving a bar saying the note could
  // not be written under one saying it has been.
  it("drops the bar of a save that failed before it was answered", async () => {
    const { store, reports } = openNote();
    const refused = deferredWrite();

    void store.saveActiveBuffer();
    await waitFor(() => expect(api.saveBufferContent).toHaveBeenCalledTimes(1));

    await reports("modified");
    expect(store.noteFileState(NOTE)).toBe("changed");

    refused.reject(REFUSED);
    await waitFor(() => expect(saveStatusStore.failureFor(NOTE)).toBeDefined());

    api.resolveExternalChange.mockResolvedValue(ANSWERED);
    await resolveNoteChange(NOTE, "keep_mine");

    expect(saveStatusStore.failureFor(NOTE)).toBeUndefined();
    expect(store.noteFileState(NOTE)).toBe("present");
  });

  it("keeps a failure the answer itself raised", async () => {
    const { store, reports } = openNote();
    await reports("modified");
    api.resolveExternalChange.mockResolvedValue(ANSWERED);

    await resolveNoteChange(NOTE, "keep_mine");
    api.saveBufferContent.mockRejectedValue(REFUSED);
    await store.saveActiveBuffer();

    expect(saveStatusStore.failureFor(NOTE)).toBeDefined();
    expect(store.noteFileState(NOTE)).toBe("present");
  });
});

/** A write held open, so the watcher can report while it is still out. */
function deferredWrite() {
  let reject: (error: unknown) => void = () => {};
  api.saveBufferContent.mockImplementationOnce(
    () =>
      new Promise((_resolve, fail) => {
        reject = fail;
      }),
  );
  return {
    reject: (error: unknown) => {
      reject(error);
    },
  };
}
