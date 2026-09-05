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
import {
  collectUnsavedContent,
  keepUnsavedForRecovery,
  resetAutosave,
} from "../../services/autosave";
import FileChangedBar from "../../components/Editor/FileChangedBar";
import SaveFailureBar from "../../components/Editor/SaveFailureBar";

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
      {
        bufferId: NOTE,
        change,
        path: "/notes/note.md",
        // Read only by a move, which needs somewhere to follow the file to.
        newPath: "/notes/note-moved.md",
      },
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

  // Holding the writes takes the note out of the queue, and the queue is what
  // the close and quit paths hand to the recovery snapshot. Without a slot of
  // its own the typing would be in the document and nowhere else, and closing
  // the tab without answering would lose it.
  it("still hands its typing over when the tab closes", async () => {
    const { store, reports } = openNote();
    await reports("modified");

    store.scheduleAutosave(NOTE, `${MINE}typed while it was asking\n`, 1000);

    expect(collectUnsavedContent()).toEqual([
      { id: NOTE, content: `${MINE}typed while it was asking\n` },
    ]);
    await keepUnsavedForRecovery(NOTE);
    expect(api.recordUnsavedNotes).toHaveBeenCalledWith([
      { id: NOTE, content: `${MINE}typed while it was asking\n` },
    ]);
  });

  it("stops holding that text once the question is answered", async () => {
    const { store, reports } = openNote();
    await reports("modified");
    store.scheduleAutosave(NOTE, `${MINE}typed while it was asking\n`, 1000);
    api.resolveExternalChange.mockResolvedValue(ANSWERED);

    await resolveNoteChange(NOTE, "keep_mine");

    // The answer wrote the live document, so the tab has nothing outstanding
    // and closing it must not put back the version answered against.
    expect(collectUnsavedContent()).toEqual([]);
    await keepUnsavedForRecovery(NOTE);
    expect(api.recordUnsavedNotes).not.toHaveBeenCalled();
  });

  it("keeps a failure raised by a write after the answer", async () => {
    const { store, reports } = openNote();
    await reports("modified");
    api.resolveExternalChange.mockResolvedValue(ANSWERED);

    await resolveNoteChange(NOTE, "keep_mine");
    api.saveBufferContent.mockRejectedValue(REFUSED);
    await store.saveActiveBuffer();

    expect(saveStatusStore.failureFor(NOTE)).toBeDefined();
    expect(store.noteFileState(NOTE)).toBe("present");
  });

  // The refusal of a write that was still on the wire arrives after the answer
  // has landed, so clearing what is on screen at the moment of the answer does
  // not reach it. It is about the same superseded file as the failures that
  // were already showing, and the tab it would land on has just been written.
  it("drops the refusal of a write that was out when it answered", async () => {
    const { store, reports } = openNote();
    const stale = deferredWrite();

    const write = store.saveActiveBuffer();
    await waitFor(() => expect(api.saveBufferContent).toHaveBeenCalledTimes(1));

    await reports("modified");
    api.resolveExternalChange.mockResolvedValue(ANSWERED);
    await resolveNoteChange(NOTE, "keep_mine");
    expect(store.noteFileState(NOTE)).toBe("present");

    stale.reject(REFUSED);
    await write;

    expect(saveStatusStore.failureFor(NOTE)).toBeUndefined();
    // The write is over whatever became of its reason, so the tab stops
    // claiming one is running.
    expect(saveStatusStore.stateOf(NOTE)).not.toBe("saving");
  });

  // The answer sends the text as it was read, and the round trip is long
  // enough to type into. That typing was held rather than queued, and the
  // answer releases the slot it was held in, so nothing but the answer itself
  // can put it back on the queue.
  it("keeps what was typed while the answer was in flight", async () => {
    const { store, view, reports } = openNote();
    await reports("modified");

    let land: (outcome: typeof ANSWERED) => void = () => {};
    api.resolveExternalChange.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          land = resolve;
        }),
    );

    const answered = resolveNoteChange(NOTE, "keep_mine");
    await waitFor(() =>
      expect(api.resolveExternalChange).toHaveBeenCalledTimes(1),
    );
    view.dispatch({
      changes: { from: view.state.doc.length, insert: "typed mid-answer\n" },
    });
    store.scheduleAutosave(NOTE, () => view.state.doc.toString(), 1000);

    land(ANSWERED);
    await answered;

    const typed = `${MINE}typed mid-answer\n`;
    expect(api.resolveExternalChange).toHaveBeenCalledWith(
      NOTE,
      "keep_mine",
      MINE,
    );
    // Closing the tab before the write lands still keeps it.
    expect(collectUnsavedContent()).toEqual([{ id: NOTE, content: typed }]);
    await waitFor(() =>
      expect(api.saveBufferContent).toHaveBeenCalledWith(NOTE, typed),
    );
  });

  // A deletion is the case that reaches the bar with a reason worth pressing
  // again: the write that was out fails for a reason of its own rather than
  // under the guard. Pressing would reach the hold and change nothing, so the
  // button is not offered until the file is back.
  it("offers no retry over a note whose file is gone", async () => {
    const { store, reports } = openNote();
    const refused = deferredWrite();

    void store.saveActiveBuffer();
    await waitFor(() => expect(api.saveBufferContent).toHaveBeenCalledTimes(1));
    await reports("removed");
    expect(store.noteFileState(NOTE)).toBe("removed");
    refused.reject("ERR_WRITE_FAILED");
    await waitFor(() => expect(saveStatusStore.failureFor(NOTE)).toBeDefined());

    const { container } = render(() => <SaveFailureBar noteId={NOTE} />);
    const labels = () =>
      [
        ...container.querySelectorAll<HTMLButtonElement>(
          ".save-failure-bar-action",
        ),
      ].map((button) => button.textContent);
    await waitFor(() => expect(labels()).toEqual(["Save a copy…"]));

    // The file turned up again, so the write the button stands for can land.
    await reports("moved");
    expect(store.noteFileState(NOTE)).toBe("present");
    await waitFor(() =>
      expect(labels()).toEqual(["Try again", "Save a copy…"]),
    );
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
