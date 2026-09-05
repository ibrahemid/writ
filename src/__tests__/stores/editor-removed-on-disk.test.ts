import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  saveBufferContent: vi.fn().mockResolvedValue(null),
  noteDiskState: vi.fn(async () => ({ state: "no_file" })),
  restoreNoteFile: vi.fn().mockResolvedValue(null),
}));

import { createEditorStore } from "../../stores/window/editor-store";
import { debouncedSave, resetAutosave } from "../../services/autosave";
import { restoreNoteFile, saveBufferContent } from "../../services/tauri";

vi.mock("../../services/autosave", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../services/autosave")>();
  return { ...actual, debouncedSave: vi.fn(actual.debouncedSave) };
});

const queued = vi.mocked(debouncedSave);
const mockedSave = vi.mocked(saveBufferContent);
const mockedRestore = vi.mocked(restoreNoteFile);

let stores: Array<{ stopSaveListener: () => void }> = [];

function newStore() {
  const store = createEditorStore();
  stores.push(store);
  return store;
}

/** A store with note "one" loaded into the editor, which is what a press saves. */
function loadedStore() {
  const store = newStore();
  const view = {
    state: { doc: { toString: () => "text" } },
  } as unknown as import("@codemirror/view").EditorView;
  store.registerView(view);
  store.setCurrentBufferId("one");
  return store;
}

beforeEach(() => {
  resetAutosave();
  queued.mockClear();
  mockedSave.mockReset();
  mockedSave.mockResolvedValue(null);
  mockedRestore.mockReset();
  mockedRestore.mockResolvedValue(null);
});

afterEach(() => {
  for (const store of stores) store.stopSaveListener();
  stores = [];
  resetAutosave();
});

describe("editorStore removed-on-disk", () => {
  it("reads a note with a file as present", () => {
    expect(newStore().isRemovedOnDisk("one")).toBe(false);
  });

  it("marks the one note whose file went", () => {
    const store = newStore();
    store.markRemovedOnDisk("one");
    expect(store.isRemovedOnDisk("one")).toBe(true);
    expect(store.isRemovedOnDisk("two")).toBe(false);
  });

  it("says it once, however many times it is told", () => {
    const store = newStore();
    store.markRemovedOnDisk("one");
    const first = store.removedOnDisk();
    store.markRemovedOnDisk("one");
    expect(store.removedOnDisk()).toBe(first);
  });

  it("clears the mark when the file comes back", () => {
    const store = newStore();
    store.markRemovedOnDisk("one");
    store.clearRemovedOnDisk("one");
    expect(store.isRemovedOnDisk("one")).toBe(false);
  });

  it("queues nothing while the file is gone", () => {
    // Every keystroke would otherwise queue a write the backend refuses, and
    // each refusal replaces the bar's reason with a fresh failure.
    const store = newStore();
    store.markRemovedOnDisk("one");

    store.scheduleAutosave("one", () => "text", 0);

    expect(queued).not.toHaveBeenCalled();
  });

  it("queues again once the note has a file", () => {
    const store = newStore();
    store.markRemovedOnDisk("one");
    store.clearRemovedOnDisk("one");

    store.scheduleAutosave("one", () => "text", 0);

    expect(queued).toHaveBeenCalledWith("one", expect.any(Function), 0);
  });

  it("puts the file back on an explicit save", async () => {
    // Autosave stays quiet, but the save keystroke is the person asking for
    // the file back, and it goes through the one write the backend allows for
    // a note whose file is gone.
    const store = loadedStore();
    store.markRemovedOnDisk("one");

    const result = await store.saveActiveBuffer();

    expect(result.ok).toBe(true);
    expect(mockedSave).not.toHaveBeenCalled();
    expect(mockedRestore).toHaveBeenCalledWith("one", "text");
    expect(store.isRemovedOnDisk("one")).toBe(false);
  });

  it("stays marked when the file could not be put back", async () => {
    const store = loadedStore();
    store.markRemovedOnDisk("one");
    mockedRestore.mockRejectedValueOnce(new Error("ERR_FILE_MISSING: no folder"));

    const result = await store.saveActiveBuffer();

    expect(result.ok).toBe(false);
    expect(store.isRemovedOnDisk("one")).toBe(true);
  });

  it("keeps the text of a note whose file went", () => {
    const store = loadedStore();
    store.markRemovedOnDisk("one");
    // The view is the loaded note's, so the mark takes its text with it.
    expect(store.textOfRemoved("one")).toBe("text");
    // A background tab's text is on disk, which is what went, so there is
    // nothing to keep and nothing is claimed.
    store.markRemovedOnDisk("two");
    expect(store.textOfRemoved("two")).toBeUndefined();
  });

  it("forgets the kept text when the file comes back", () => {
    const store = loadedStore();
    store.markRemovedOnDisk("one");
    store.clearRemovedOnDisk("one");
    expect(store.textOfRemoved("one")).toBeUndefined();
  });

  it("puts the file back when the bar's retry is pressed", async () => {
    // The bar over a restore that could not land offers a retry, and the press
    // has to mean the same restore. An ordinary write is refused every time,
    // so it would answer the bar with the same bar.
    const store = loadedStore();
    store.markRemovedOnDisk("one");

    const result = await store.retrySave("one");

    expect(result.ok).toBe(true);
    expect(mockedSave).not.toHaveBeenCalled();
    expect(mockedRestore).toHaveBeenCalledWith("one", "text");
    expect(store.isRemovedOnDisk("one")).toBe(false);
  });

  it("drops the mark when a restore lands from the queue", async () => {
    // A restore stopped by something that passes reaches the queue, and the
    // flush a tab switch runs is what writes it. The file is back at that
    // point, so the bar over it goes too.
    const store = loadedStore();
    store.markRemovedOnDisk("one");
    mockedRestore.mockRejectedValueOnce(new Error("ERR_FILE_MISSING: no folder"));
    await store.saveActiveBuffer();
    expect(store.isRemovedOnDisk("one")).toBe(true);

    const result = await store.flushAutosave("one");

    expect(result.ok).toBe(true);
    expect(mockedRestore).toHaveBeenLastCalledWith("one", "text");
    expect(store.isRemovedOnDisk("one")).toBe(false);
  });

  it("writes on an explicit save while the file is there", async () => {
    // The control for the test above: without the mark the same press writes.
    const store = loadedStore();

    await store.saveActiveBuffer();

    expect(mockedSave).toHaveBeenCalled();
  });

  it("forgets the mark with the tab", () => {
    // A note id is reused by nothing, but a tab closed and its file restored
    // must not reopen wearing the old answer.
    const store = newStore();
    store.markRemovedOnDisk("one");
    store.noteClosed("one");
    expect(store.isRemovedOnDisk("one")).toBe(false);
  });
});
