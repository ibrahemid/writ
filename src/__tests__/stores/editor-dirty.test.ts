import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { EditorState, Text, type TransactionSpec } from "@codemirror/state";
import { history, isolateHistory, undo } from "@codemirror/commands";

// The file's digest comes from Rust, so the test plays that part: whatever a
// note's file is said to hold is what `noteDiskState` answers with. A note the
// map says nothing about is a new note with no file.
const backend = vi.hoisted(() => ({
  answers: new Map<string, { state: string; disk?: { hash: string } }>(),
  throwFor: new Set<string>(),
}));

vi.mock("../../services/tauri", () => ({
  saveBufferContent: vi.fn().mockResolvedValue(null),
  restoreNoteFile: vi.fn().mockResolvedValue(null),
  recordUnsavedNotes: vi.fn().mockResolvedValue(undefined),
  discardUnsavedText: vi.fn().mockResolvedValue(undefined),
  noteDiskState: vi.fn(async (id: string) => {
    if (backend.throwFor.has(id)) throw new Error("no answer");
    return backend.answers.get(id) ?? { state: "no_file" };
  }),
}));

import type { EditorView } from "@codemirror/view";
import { createEditorStore, DIRTY_COMPARE_IDLE_MS } from "../../stores/window/editor-store";
import { flushAutosave, resetAutosave } from "../../services/autosave";
import {
  noteDiskState,
  restoreNoteFile,
  saveBufferContent,
  type NoteDiskAnswer,
} from "../../services/tauri";

const mockedSave = vi.mocked(saveBufferContent);
const mockedRestore = vi.mocked(restoreNoteFile);
const mockedDiskState = vi.mocked(noteDiskState);

type Store = ReturnType<typeof createEditorStore>;

function doc(text: string): Text {
  return Text.of(text.split("\n"));
}

function turn(ms = 0): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/** Waits past the pause after which an edit is compared with the file's text. */
function compared(): Promise<void> {
  return turn(DIRTY_COMPARE_IDLE_MS + 30);
}

/** Says the note's file was read and holds bytes with digest `hash`. */
function fileHolds(id: string, hash: string): void {
  backend.answers.set(id, { state: "described", disk: { hash } });
}

/** Opens a note whose file holds exactly `text`, read from it: the ordinary case. */
async function openNote(store: Store, id: string, text: string): Promise<Text> {
  fileHolds(id, `${id}-on-disk`.padEnd(64, "0"));
  const opened = doc(text);
  store.noteOpened(id, opened, true);
  await vi.waitFor(() => expect(store.isTracked(id)).toBe(true));
  return opened;
}

/** Types `insert` at `at` into the note's document, as the editor reports a transaction. */
function typeInto(store: Store, id: string, before: Text, at: number, insert: string, remove = 0): Text {
  const after = before.replace(at, at + remove, doc(insert));
  store.noteEdited(id, after);
  return after;
}

/** Writes `text` through the store's autosave, the way a pause in typing does. */
async function writes(store: Store, id: string, text: Text | string, digest = "w".repeat(64)): Promise<void> {
  mockedSave.mockResolvedValue(digest);
  store.scheduleAutosave(id, text, 0);
  await flushAutosave(id);
  await turn();
}

let stores: Store[] = [];

function newStore(): Store {
  const store = createEditorStore();
  stores.push(store);
  return store;
}

/** What each write handed to the ordinary save carried, in order. */
function written(): string[] {
  return mockedSave.mock.calls.map(([, text]) => text);
}

/** A write that stays on the wire until the test lets it land with `digest`. */
function holdNextWrite(): (digest: string) => void {
  let land: (digest: string) => void = () => {};
  mockedSave.mockImplementationOnce(() => new Promise<string>((resolve) => (land = resolve)));
  return (digest) => land(digest);
}

/** Refused for a reason worth trying again: the autosave puts the text back on its queue. */
const WORTH_REPEATING = new Error("ERR_WRITE_FAILED: the disk would not take the write");
/** Refused for a reason the autosave does not repeat: only the failed text keeps it. */
const NOT_REPEATED = new Error("ERR_FILE_NOT_DOWNLOADED: still downloading");

/** A view showing note `id` holding `text`, opened from its file the way a load does. */
async function showNote(store: Store, id: string, text: string): Promise<{ state: { doc: Text } }> {
  const view = { state: { doc: doc(text) } };
  store.registerView(view as unknown as EditorView);
  store.setCurrentBufferId(id);
  fileHolds(id, `${id}-on-disk`.padEnd(64, "0"));
  store.noteOpened(id, view.state.doc, true);
  await vi.waitFor(() => expect(store.isTracked(id)).toBe(true));
  return view;
}

beforeEach(() => {
  backend.answers.clear();
  backend.throwFor.clear();
  resetAutosave();
  mockedSave.mockReset();
  mockedSave.mockResolvedValue(null);
  mockedRestore.mockReset();
  mockedRestore.mockResolvedValue(null);
});

afterEach(() => {
  for (const store of stores) store.stopSaveListener();
  stores = [];
  resetAutosave();
  vi.restoreAllMocks();
});

describe("editorStore dirty contract", () => {
  it("reads a note it has never seen as dirty, not clean", () => {
    // Fail closed. The callers of this decide whether a file may be reloaded
    // over the document, so "no idea" has to stop the reload. A tab restored
    // at launch and never brought to the front is the ordinary case.
    const store = newStore();
    expect(store.isDirty("unknown")).toBe(true);
    expect(store.isTracked("unknown")).toBe(false);
  });

  it("is clean once a note opened from its file has been answered for", async () => {
    const store = newStore();
    await openNote(store, "a", "hello");

    expect(store.lastKnownDiskHash("a")).toBe("a-on-disk".padEnd(64, "0"));
    expect(store.isDirty("a")).toBe(false);
  });

  it("reads a note opened from text that is not its file's as dirty, whatever the file holds", async () => {
    // Text a hold kept, or typing a save could not write, is not what the
    // file holds. Only the read says the document is the file's text, so the
    // frontend has nothing to mint a match from.
    const store = newStore();
    fileHolds("a", "0".repeat(64));
    store.noteOpened("a", doc("hello"), false);
    await vi.waitFor(() => expect(store.isTracked("a")).toBe(true));

    expect(store.lastKnownDiskHash("a")).toBe("0".repeat(64));
    expect(store.isDirty("a")).toBe(true);
  });

  it("reads a note whose file could not be described as dirty, not clean", async () => {
    // The file is missing, or its bytes are not on this machine. The editor
    // holds a document with nothing to compare it against, which is the same
    // position the error path leaves it in, so it gets the same answer: no
    // record, and dirty. Reading it clean would let a reload replace text no
    // file holds.
    const store = newStore();
    backend.answers.set("evicted", { state: "undescribed" });
    store.noteOpened("evicted", doc("text only this tab has"), true);
    await vi.waitFor(() => expect(store.isTracked("evicted")).toBe(false));

    expect(store.isDirty("evicted")).toBe(true);
    expect(store.lastKnownDiskHash("evicted")).toBeUndefined();
  });

  it("reads a note whose file could not be read at all as dirty", async () => {
    const store = newStore();
    backend.throwFor.add("unreadable");
    store.noteOpened("unreadable", doc("text only this tab has"), true);
    await vi.waitFor(() => expect(store.isTracked("unreadable")).toBe(false));

    expect(store.isDirty("unreadable")).toBe(true);
  });

  it("records nothing for a note with no file, and reads it clean until it is typed into", async () => {
    // A new note nothing has saved yet: there is no digest to be measured
    // against, and an empty new tab is not unsaved work.
    const store = newStore();
    const opened = doc("");
    store.noteOpened("new", opened, true);
    await vi.waitFor(() => expect(store.isTracked("new")).toBe(true));

    expect(store.lastKnownDiskHash("new")).toBeUndefined();
    expect(store.isDirty("new")).toBe(false);

    typeInto(store, "new", opened, 0, "first words");
    expect(store.isDirty("new")).toBe(true);
    await compared();
    expect(store.isDirty("new")).toBe(true);
  });

  it("reads dirty for a character typed inside the idle window, before the comparison", async () => {
    const store = newStore();
    const opened = await openNote(store, "a", "hello");
    expect(store.isDirty("a")).toBe(false);

    typeInto(store, "a", opened, 5, "!");

    expect(store.isDirty("a")).toBe(true);
  });

  it("stays dirty while the edit differs from the file, before and after the comparison", async () => {
    const store = newStore();
    const opened = await openNote(store, "a", "hello");

    typeInto(store, "a", opened, 5, "!");
    expect(store.isDirty("a")).toBe(true);
    await compared();

    expect(store.isDirty("a")).toBe(true);
  });

  it("goes clean again when the document is typed back to what the file holds", async () => {
    const store = newStore();
    const opened = await openNote(store, "a", "hello");

    const edited = typeInto(store, "a", opened, 5, "!");
    await compared();
    expect(store.isDirty("a")).toBe(true);

    typeInto(store, "a", edited, 5, "", 1);
    await compared();
    expect(store.isDirty("a")).toBe(false);
  });

  it("decides the answer for the newest edit only", async () => {
    const store = newStore();
    const opened = await openNote(store, "a", "hello");

    const first = typeInto(store, "a", opened, 5, "!");
    await turn(DIRTY_COMPARE_IDLE_MS / 3);
    typeInto(store, "a", first, 5, "", 1);
    expect(store.isDirty("a")).toBe(true);

    await compared();
    expect(store.isDirty("a")).toBe(false);
  });

  it("counts a differing document as dirty with the queue empty and a write just resolved", async () => {
    const store = newStore();
    const opened = await openNote(store, "a", "hello");

    const written = typeInto(store, "a", opened, 0, "written ");
    await writes(store, "a", written, "x".repeat(64));
    typeInto(store, "a", written, 0, "typed after the write ");
    await compared();

    expect(store.lastKnownDiskHash("a")).toBe("x".repeat(64));
    expect(store.isDirty("a")).toBe(true);
  });

  it("is clean as soon as a write of exactly what the document holds lands", async () => {
    const store = newStore();
    const opened = await openNote(store, "a", "hello");

    const edited = typeInto(store, "a", opened, 5, "!");
    await writes(store, "a", edited);

    expect(store.isDirty("a")).toBe(false);
  });

  it("stays dirty when a write handed over as a string lands after typing it does not carry", async () => {
    const store = newStore();
    const opened = await openNote(store, "a", "hello");
    let release: (value: string) => void = () => {};
    mockedSave.mockImplementationOnce(() => new Promise<string>((resolve) => (release = resolve)));
    const typed = typeInto(store, "a", opened, 5, "!");
    store.scheduleAutosave("a", typed.toString(), 0);
    const landing = flushAutosave("a");
    await turn();
    typeInto(store, "a", typed, 6, "?");

    release("o".repeat(64));
    await landing;
    await compared();

    expect(store.isDirty("a")).toBe(true);
  });

  it("goes clean when undo takes the document back to the text a write left", async () => {
    // The acceptance check's case: type, let the save land, type on, undo
    // back. No write follows the undo, so only the comparison can say so.
    const store = newStore();
    let state = EditorState.create({ doc: "the saved text", extensions: [history()] });
    fileHolds("a", "f".repeat(64));
    store.noteOpened("a", state.doc, true);
    await vi.waitFor(() => expect(store.isTracked("a")).toBe(true));

    const dispatch = (spec: TransactionSpec) => {
      state = state.update(spec).state;
      store.noteEdited("a", state.doc);
    };
    dispatch({ changes: { from: state.doc.length, insert: " saved" }, userEvent: "input.type" });
    await writes(store, "a", state.doc);
    expect(store.isDirty("a")).toBe(false);

    dispatch({
      changes: { from: state.doc.length, insert: " and more" },
      userEvent: "input.type",
      annotations: isolateHistory.of("before"),
    });
    await compared();
    expect(store.isDirty("a")).toBe(true);

    undo({
      state,
      dispatch: (tr) => {
        state = tr.state;
        store.noteEdited("a", state.doc);
      },
    });
    expect(state.doc.toString()).toBe("the saved text saved");
    await compared();
    expect(store.isDirty("a")).toBe(false);
  });

  it("reads only the edited stretches when undo goes back to the saved text across edits far apart", async () => {
    const store = newStore();
    const lines = Array.from({ length: 100_000 }, (_, i) => `line ${i} of the saved text`);
    let state = EditorState.create({ doc: lines.join("\n"), extensions: [history()] });
    fileHolds("a", "f".repeat(64));
    store.noteOpened("a", state.doc, true);
    await vi.waitFor(() => expect(store.isTracked("a")).toBe(true));

    const apply = (tr: { state: EditorState }) => {
      state = tr.state;
      store.noteEdited("a", state.doc);
    };
    apply(state.update({ changes: { from: 0, insert: "top " }, annotations: isolateHistory.of("full") }));
    apply(
      state.update({
        changes: { from: state.doc.length, insert: " bottom" },
        annotations: isolateHistory.of("full"),
      }),
    );
    await compared();
    expect(store.isDirty("a")).toBe(true);

    const eq = vi.spyOn(Text.prototype, "eq");
    undo({ state, dispatch: apply });
    undo({ state, dispatch: apply });
    expect(state.doc.toString()).toBe(lines.join("\n"));
    await compared();

    expect(store.isDirty("a")).toBe(false);
    const read = eq.mock.contexts.reduce<number>((sum, text) => sum + (text as Text).length, 0);
    expect(read).toBeLessThan(16 * 1024);
  });

  it("never compares documents inside the dirty question itself", async () => {
    const store = newStore();
    const opened = await openNote(store, "a", "hello");
    // As long as the file's text, so a comparison would reach `Text.eq`.
    typeInto(store, "a", opened, 4, "O", 1);
    const eq = vi.spyOn(Text.prototype, "eq");

    for (let i = 0; i < 100; i++) store.isDirty("a");

    expect(eq).not.toHaveBeenCalled();
  });

  it("tracks two notes apart, including the one that is not in front", async () => {
    const store = newStore();
    await openNote(store, "a", "one");
    const two = await openNote(store, "b", "two");

    typeInto(store, "b", two, 3, " edited");
    await compared();

    expect(store.isDirty("a")).toBe(false);
    expect(store.isDirty("b")).toBe(true);
  });

  it("reads a reloaded note as clean again", async () => {
    const store = newStore();
    const opened = await openNote(store, "a", "hello");
    typeInto(store, "a", opened, 0, "typed ");
    await compared();
    expect(store.isDirty("a")).toBe(true);

    await openNote(store, "a", "what the file now holds");
    expect(store.isDirty("a")).toBe(false);
  });

  it("forgets a note whose tab has gone", async () => {
    const store = newStore();
    await openNote(store, "a", "hello");
    store.noteClosed("a");

    expect(store.isTracked("a")).toBe(false);
    expect(store.lastKnownDiskHash("a")).toBeUndefined();
  });

  it("does not move the record of the file when a save wrote nothing", async () => {
    const store = newStore();
    await openNote(store, "a", "hello");
    const before = store.lastKnownDiskHash("a");

    store.noteSaved("a", null, false);

    expect(store.lastKnownDiskHash("a")).toBe(before);
  });
});

describe("a note with no view", () => {
  it("reads clean once a write of its newest text lands", async () => {
    // The tab was switched away from while its save was failing; Try again
    // writes the text the failure kept, and nothing newer exists.
    const store = newStore();
    const opened = await openNote(store, "a", "hello");
    typeInto(store, "a", opened, 5, "!");
    store.registerView(null);
    expect(store.isDirty("a")).toBe(true);

    await writes(store, "a", doc("hello!"));

    expect(store.isDirty("a")).toBe(false);
  });

  it("stays dirty when newer text was handed to the autosave after the write", async () => {
    const store = newStore();
    await openNote(store, "a", "hello");
    store.registerView(null);
    mockedSave.mockResolvedValue("v".repeat(64));

    let release: (value: string) => void = () => {};
    mockedSave.mockImplementationOnce(() => new Promise<string>((resolve) => (release = resolve)));
    store.scheduleAutosave("a", "older", 0);
    const landing = flushAutosave("a");
    await turn();
    store.scheduleAutosave("a", "newer", 60_000);
    release("o".repeat(64));
    await landing;
    await turn();

    expect(store.isDirty("a")).toBe(true);
  });

  it("stays dirty when newer text was held after the write", async () => {
    const store = newStore();
    await openNote(store, "a", "hello");
    store.registerView(null);

    const land = holdNextWrite();
    store.scheduleAutosave("a", "older", 0);
    const landing = flushAutosave("a");
    store.markRemovedOnDisk("a");
    store.scheduleAutosave("a", "newer", 0);
    land("o".repeat(64));
    await landing;
    await turn();

    expect(store.textOfRemoved("a")).toBe("newer");
    expect(store.isDirty("a")).toBe(true);
  });

  it("stays dirty when a write lands after typing the autosave was not handed", async () => {
    // Typing while a bar is up is held rather than queued, so it moves the
    // document past a write already on its way without queueing another.
    const store = newStore();
    const opened = await openNote(store, "a", "hello");
    let release: (value: string) => void = () => {};
    mockedSave.mockImplementationOnce(() => new Promise<string>((resolve) => (release = resolve)));
    const typed = typeInto(store, "a", opened, 5, "!");
    store.scheduleAutosave("a", typed, 0);
    const landing = flushAutosave("a");
    await turn();
    typeInto(store, "a", typed, 6, "?");
    store.registerView(null);

    release("o".repeat(64));
    await landing;
    await turn();

    expect(store.isDirty("a")).toBe(true);
  });

  it("reads clean after an answer that wrote its text, when the caller says that text is current", async () => {
    const store = newStore();
    const opened = await openNote(store, "a", "hello");
    const typed = typeInto(store, "a", opened, 5, "!");
    store.registerView(null);

    store.noteSaved("a", "k".repeat(64), false, { source: typed, current: false });
    expect(store.isDirty("a")).toBe(true);

    store.noteSaved("a", "k".repeat(64), false, { source: typed, current: true });
    expect(store.isDirty("a")).toBe(false);
  });

  it("keeps no document of its own once the view lets it go", async () => {
    const store = newStore();
    const opened = await openNote(store, "a", "hello");
    // As long as the file's text, so a comparison would reach `Text.eq`.
    typeInto(store, "a", opened, 4, "O", 1);
    store.registerView(null);
    const eq = vi.spyOn(Text.prototype, "eq");

    await compared();

    expect(eq).not.toHaveBeenCalled();
    expect(store.isDirty("a")).toBe(true);
  });
});

describe("Try again", () => {
  it("leaves the note dirty when it rewrites a failed text after a newer write went out", async () => {
    const store = newStore();
    const opened = await openNote(store, "a", "hello");
    mockedSave.mockRejectedValueOnce(WORTH_REPEATING);
    const one = typeInto(store, "a", opened, 5, " 1");
    store.scheduleAutosave("a", one, 0);
    await flushAutosave("a");

    const two = typeInto(store, "a", one, 7, " 2");
    const landSecond = holdNextWrite();
    store.scheduleAutosave("a", two, 0);
    const second = flushAutosave("a");
    mockedSave.mockResolvedValueOnce("r".repeat(64));
    const retried = store.retrySave("a");
    landSecond("2".repeat(64));
    await Promise.all([second, retried]);
    await compared();

    expect(written()).toEqual(["hello 1", "hello 1 2", "hello 1"]);
    expect(store.isDirty("a")).toBe(true);
  });

  it("leaves a note with no view dirty when it rewrites a failed text after a newer write went out", async () => {
    const store = newStore();
    const opened = await openNote(store, "a", "hello");
    mockedSave.mockRejectedValueOnce(WORTH_REPEATING);
    const one = typeInto(store, "a", opened, 5, " 1");
    store.scheduleAutosave("a", one, 0);
    await flushAutosave("a");

    const two = typeInto(store, "a", one, 7, " 2");
    const landSecond = holdNextWrite();
    store.scheduleAutosave("a", two, 0);
    const second = flushAutosave("a");
    store.registerView(null);
    mockedSave.mockResolvedValueOnce("r".repeat(64));
    const retried = store.retrySave("a");
    landSecond("2".repeat(64));
    await Promise.all([second, retried]);
    await compared();

    expect(written()).toEqual(["hello 1", "hello 1 2", "hello 1"]);
    expect(store.isDirty("a")).toBe(true);
  });

  it("reads clean when the text it rewrites is the document, with a save of that document on the wire", async () => {
    const store = newStore();
    const view = await showNote(store, "a", "hello");
    mockedSave.mockRejectedValueOnce(WORTH_REPEATING);
    view.state.doc = typeInto(store, "a", view.state.doc, 5, " 1");
    store.scheduleAutosave("a", view.state.doc, 0);
    await flushAutosave("a");

    const landSave = holdNextWrite();
    const saved = store.saveActiveBuffer();
    mockedSave.mockResolvedValueOnce("r".repeat(64));
    const retried = store.retrySave("a");
    landSave("s".repeat(64));
    await Promise.all([saved, retried]);
    await compared();

    expect(written()).toEqual(["hello 1", "hello 1", "hello 1"]);
    expect(store.isDirty("a")).toBe(false);
  });

  it("reads clean when it rewrites a failed text the autosave does not repeat and nothing was typed since", async () => {
    const store = newStore();
    const opened = await openNote(store, "a", "hello");
    mockedSave.mockRejectedValueOnce(NOT_REPEATED);
    const one = typeInto(store, "a", opened, 5, " 1");
    store.scheduleAutosave("a", one, 0);
    await flushAutosave("a");

    mockedSave.mockResolvedValueOnce("r".repeat(64));
    await store.retrySave("a");
    await compared();

    expect(written()).toEqual(["hello 1", "hello 1"]);
    expect(store.isDirty("a")).toBe(false);
  });

  it("reads clean when it rewrites the queued text of a failed write and nothing newer exists", async () => {
    const store = newStore();
    const opened = await openNote(store, "a", "hello");
    mockedSave.mockRejectedValueOnce(WORTH_REPEATING);
    const one = typeInto(store, "a", opened, 5, " 1");
    store.scheduleAutosave("a", one, 0);
    await flushAutosave("a");

    mockedSave.mockResolvedValueOnce("r".repeat(64));
    await store.retrySave("a");
    await compared();

    expect(written()).toEqual(["hello 1", "hello 1"]);
    expect(store.isDirty("a")).toBe(false);
  });
});

describe("the write after a file turns up at another path", () => {
  it("reads clean once the view's text it writes lands", async () => {
    const store = newStore();
    const view = await showNote(store, "a", "hello");
    view.state.doc = typeInto(store, "a", view.state.doc, 5, " 1");
    store.markRemovedOnDisk("a");
    view.state.doc = typeInto(store, "a", view.state.doc, 7, " 2");
    store.scheduleAutosave("a", view.state.doc, 0);

    mockedSave.mockResolvedValueOnce("m".repeat(64));
    store.recordFileEvent("a", "moved");
    await flushAutosave("a");
    await compared();

    expect(written()).toEqual(["hello 1 2"]);
    expect(store.isDirty("a")).toBe(false);
  });

  it("reads clean once the held text of a note with no view lands", async () => {
    const store = newStore();
    const view = await showNote(store, "a", "hello");
    view.state.doc = typeInto(store, "a", view.state.doc, 5, " 1");
    store.markRemovedOnDisk("a");
    store.keepTextOfRemoved("a", view.state.doc.toString());
    store.registerView(null);

    mockedSave.mockResolvedValueOnce("m".repeat(64));
    store.recordFileEvent("a", "moved");
    await flushAutosave("a");
    await compared();

    expect(written()).toEqual(["hello 1"]);
    expect(store.isDirty("a")).toBe(false);
  });

  it("reads clean once a failed restore's queued text lands, when it is still the note's text", async () => {
    const store = newStore();
    const view = await showNote(store, "a", "hello");
    view.state.doc = typeInto(store, "a", view.state.doc, 5, " 1");
    store.markRemovedOnDisk("a");
    store.keepTextOfRemoved("a", view.state.doc.toString());
    store.registerView(null);
    mockedRestore.mockRejectedValueOnce(WORTH_REPEATING);
    await store.restoreRemovedFile("a");

    mockedSave.mockResolvedValueOnce("m".repeat(64));
    store.recordFileEvent("a", "moved");
    await flushAutosave("a");
    await compared();

    expect(written()).toEqual(["hello 1"]);
    expect(store.isDirty("a")).toBe(false);
  });

  it("leaves a note with no view dirty when it writes a failed restore's text that typing has since moved past", async () => {
    const store = newStore();
    const view = await showNote(store, "a", "hello");
    view.state.doc = typeInto(store, "a", view.state.doc, 5, " 1");
    store.markRemovedOnDisk("a");
    mockedRestore.mockRejectedValueOnce(WORTH_REPEATING);
    await store.restoreRemovedFile("a");
    view.state.doc = typeInto(store, "a", view.state.doc, 7, " 2");
    store.scheduleAutosave("a", view.state.doc, 0);
    store.keepTextOfRemoved("a", view.state.doc.toString());
    store.registerView(null);

    mockedSave.mockResolvedValueOnce("m".repeat(64));
    store.recordFileEvent("a", "moved");
    await flushAutosave("a");
    await compared();

    expect(written()).toEqual(["hello 1"]);
    expect(store.isDirty("a")).toBe(true);
  });

  it("leaves a note with no view dirty when it writes a failed restore's text that an undo had moved past", async () => {
    // Fuzz seed 5529: the failed text is held when the file goes again, and
    // that hold is what the file found at another path is written.
    const store = newStore();
    const view = await showNote(store, "a", "hello");
    store.markRemovedOnDisk("a");
    let landFirst: (digest: string) => void = () => {};
    mockedRestore.mockImplementationOnce(() => new Promise<string>((resolve) => (landFirst = resolve)));
    const first = store.saveActiveBuffer();
    view.state.doc = typeInto(store, "a", view.state.doc, 5, " z");
    store.scheduleAutosave("a", view.state.doc, 0);
    mockedRestore.mockRejectedValueOnce(NOT_REPEATED);
    const second = store.saveActiveBuffer();
    view.state.doc = typeInto(store, "a", view.state.doc, 5, "", 2);
    store.scheduleAutosave("a", view.state.doc, 0);
    landFirst("r".repeat(64));
    await Promise.all([first, second]);

    store.registerView(null);
    store.recordFileEvent("a", "removed");
    expect(store.liveTextOf("a")).toEqual({ content: "hello z", current: false });
    mockedSave.mockResolvedValueOnce("m".repeat(64));
    store.recordFileEvent("a", "moved");
    await flushAutosave("a");
    await compared();

    expect(mockedRestore.mock.calls.map(([, text]) => text)).toEqual(["hello", "hello z"]);
    expect(written()).toEqual(["hello z"]);
    expect(store.isDirty("a")).toBe(true);
  });
});

describe("text handed to the autosave that is not the note's document", () => {
  it("leaves a note with no view dirty once its write lands", async () => {
    const store = newStore();
    await openNote(store, "a", "hello");
    store.registerView(null);

    mockedSave.mockResolvedValueOnce("w".repeat(64));
    store.scheduleAutosave("a", "an older text", 0, false);
    await flushAutosave("a");
    await compared();

    expect(written()).toEqual(["an older text"]);
    expect(store.isDirty("a")).toBe(true);
  });

  it("is held as other than the note's text, so writing it to the file found at another path leaves the note dirty", async () => {
    const store = newStore();
    const view = await showNote(store, "a", "hello");
    view.state.doc = typeInto(store, "a", view.state.doc, 5, " 1");
    store.markRemovedOnDisk("a");
    store.registerView(null);
    store.scheduleAutosave("a", "an older text", 0, false);
    expect(store.liveTextOf("a")).toEqual({ content: "an older text", current: false });

    mockedSave.mockResolvedValueOnce("m".repeat(64));
    store.recordFileEvent("a", "moved");
    await flushAutosave("a");
    await compared();

    expect(written()).toEqual(["an older text"]);
    expect(store.isDirty("a")).toBe(true);
  });
});

describe("a write that lands while the open is still out", () => {
  function answerLater(): (hash: string) => void {
    let answer: (value: NoteDiskAnswer) => void = () => {};
    mockedDiskState.mockImplementationOnce(() => new Promise<NoteDiskAnswer>((resolve) => (answer = resolve)));
    return (hash) => answer({ state: "described", disk: { hash, size: 6, mtime_ms: null } });
  }

  it("compares the written document once an open that answers after the pause is in", async () => {
    const store = newStore();
    const answer = answerLater();
    const opened = doc("abcdef");
    store.noteOpened("a", opened, true);
    typeInto(store, "a", opened, 0, "x", 1);
    store.noteSaved("a", "k".repeat(64), false, { source: doc("xbcdef"), current: true });
    await compared();

    answer("f".repeat(64));
    await compared();

    expect(store.isDirty("a")).toBe(false);
  });

  it("stays dirty once that open is in when the write carried other text", async () => {
    const store = newStore();
    const answer = answerLater();
    const opened = doc("abcdef");
    store.noteOpened("a", opened, true);
    typeInto(store, "a", opened, 0, "x", 1);
    store.noteSaved("a", "k".repeat(64), false, { source: doc("ybcdef"), current: true });
    await compared();

    answer("f".repeat(64));
    await compared();

    expect(store.isDirty("a")).toBe(true);
  });
});
