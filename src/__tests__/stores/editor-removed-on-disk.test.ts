import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  saveBufferContent: vi.fn().mockResolvedValue(null),
  noteDiskState: vi.fn(async () => ({ state: "no_file" })),
}));

import { createEditorStore } from "../../stores/window/editor-store";
import { debouncedSave, resetAutosave } from "../../services/autosave";
import { saveBufferContent } from "../../services/tauri";

vi.mock("../../services/autosave", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../services/autosave")>();
  return { ...actual, debouncedSave: vi.fn(actual.debouncedSave) };
});

const queued = vi.mocked(debouncedSave);
const mockedSave = vi.mocked(saveBufferContent);

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

  it("writes nothing on an explicit save either", async () => {
    const store = loadedStore();
    store.markRemovedOnDisk("one");

    const result = await store.saveActiveBuffer();

    expect(result.ok).toBe(true);
    expect(mockedSave).not.toHaveBeenCalled();
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
