import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// One note has one file, so it has one state. Two independent flags let a tab
// hold "the file changed" and "the file is gone" at the same time, which puts
// two bars on it saying different things, and the answers to the first one
// read a file the second one says is not there.
//
// Driven through the wiring the app runs (`createExternalEditDeps`) over a
// real editor store, because that is where the orderings come from: each
// response is a line of that wiring, and a test over a copy of it would keep
// passing while the app kept the version that was copied.

vi.mock("../../services/tauri", () => ({
  saveBufferContent: vi.fn().mockResolvedValue(null),
  noteDiskState: vi.fn(async () => ({ state: "no_file" })),
}));

import {
  createEditorStore,
  type NoteFileEvent,
  type NoteFileState,
} from "../../stores/window/editor-store";
import { createExternalEditDeps } from "../../lib/external-edit-deps";
import {
  handleExternalEdit,
  type ExternalChange,
} from "../../services/external-edit";
import { resetAutosave } from "../../services/autosave";

const NOTE = "n1";

let stores: Array<{ stopSaveListener: () => void }> = [];

function newStore() {
  const store = createEditorStore();
  stores.push(store);
  return store;
}

/** A store wired the way `App.tsx` wires it, with the collaborators recorded. */
function wiredStore() {
  const store = newStore();
  const refreshed: string[] = [];
  const forgotten: string[] = [];
  const cancelled: string[] = [];
  const deps = createExternalEditDeps({
    editor: store,
    openBuffers: () => [{ id: NOTE, title: "note", filename: "note.md" }],
    refreshBuffer: async (id) => {
      refreshed.push(id);
    },
    forgetSaveStatus: (id) => {
      forgotten.push(id);
    },
    cancelAutosave: (id) => {
      cancelled.push(id);
    },
  });

  // What the watcher reports, as the event carries it.
  async function reports(change: ExternalChange, newPath?: string) {
    await handleExternalEdit(
      { bufferId: NOTE, change, path: "/notes/note.md", newPath },
      deps,
    );
  }

  return { store, reports, refreshed, forgotten, cancelled };
}

beforeEach(() => {
  resetAutosave();
});

afterEach(() => {
  for (const store of stores) store.stopSaveListener();
  stores = [];
  resetAutosave();
});

describe("what a note's file is doing", () => {
  it("is present until something says otherwise", () => {
    const store = newStore();
    expect(store.noteFileState(NOTE)).toBe("present");
    expect(store.isRemovedOnDisk(NOTE)).toBe(false);
    expect(store.isFileChangedOnDisk(NOTE)).toBe(false);
  });

  // The whole table, rather than the rows the orderings below happen to walk.
  // A row added to the policy without a row here is a state nobody chose.
  const TABLE: Array<[NoteFileState, NoteFileEvent, NoteFileState]> = [
    ["present", "modified", "changed"],
    ["present", "removed", "removed"],
    ["present", "moved", "present"],
    ["present", "settled", "present"],
    ["changed", "modified", "changed"],
    ["changed", "removed", "removed"],
    ["changed", "moved", "changed"],
    ["changed", "settled", "present"],
    ["removed", "modified", "changed"],
    ["removed", "removed", "removed"],
    ["removed", "moved", "present"],
    ["removed", "settled", "removed"],
  ];

  it.each(TABLE)("goes from %s on %s to %s", (before, event, after) => {
    const store = newStore();
    if (before === "changed") store.recordFileEvent(NOTE, "modified");
    if (before === "removed") store.recordFileEvent(NOTE, "removed");
    expect(store.noteFileState(NOTE)).toBe(before);

    store.recordFileEvent(NOTE, event);

    expect(store.noteFileState(NOTE)).toBe(after);
  });

  it("covers every state against every event", () => {
    const seen = new Set(TABLE.map(([before, event]) => `${before}/${event}`));
    expect(seen.size).toBe(12);
  });

  it("is never two things at once, whatever it is told", () => {
    const store = newStore();
    const events: NoteFileEvent[] = ["modified", "removed", "moved", "settled"];
    for (const first of events) {
      for (const second of events) {
        const id = `${first}-${second}`;
        store.recordFileEvent(id, first);
        store.recordFileEvent(id, second);
        expect(
          [store.isRemovedOnDisk(id), store.isFileChangedOnDisk(id)].filter(
            Boolean,
          ).length,
          `${first} then ${second} raises two bars`,
        ).toBeLessThan(2);
      }
    }
  });

  it("is forgotten with the tab", () => {
    const store = newStore();
    store.recordFileEvent(NOTE, "removed");

    store.noteClosed(NOTE);

    expect(store.noteFileState(NOTE)).toBe("present");
  });
});

describe("two changes to one file, in the order they arrive", () => {
  it("drops the question when the file is then deleted", async () => {
    // The bar's three answers all read the file they are answering about. A
    // file that is gone cannot be read, so the question has to go with it and
    // leave the answers that suit a deleted file.
    const { store, reports } = wiredStore();

    await reports("modified");
    expect(store.noteFileState(NOTE)).toBe("changed");

    await reports("removed");

    expect(store.noteFileState(NOTE)).toBe("removed");
    expect(store.isFileChangedOnDisk(NOTE)).toBe(false);
  });

  it("keeps the question when the file is then moved", async () => {
    // A rename after an edit is one `mv` in a synced folder. The file at the
    // new path still differs from the tab, so the question stands; the tab is
    // repointed under it and the answer goes by the note's id, which the
    // command reads the current path from.
    const { store, reports, refreshed } = wiredStore();

    await reports("modified");
    await reports("moved", "/notes/renamed.md");

    expect(store.noteFileState(NOTE)).toBe("changed");
    expect(store.isRemovedOnDisk(NOTE)).toBe(false);
    expect(refreshed).toEqual([NOTE]);
  });

  it("asks about a file that was deleted and then came back different", async () => {
    const { store, reports } = wiredStore();

    await reports("removed");
    expect(store.noteFileState(NOTE)).toBe("removed");

    await reports("modified");

    expect(store.noteFileState(NOTE)).toBe("changed");
    expect(store.isRemovedOnDisk(NOTE)).toBe(false);
  });

  it("stops marking a note deleted once its file turns up elsewhere", async () => {
    const { store, reports } = wiredStore();

    await reports("removed");
    await reports("moved", "/notes/moved-here.md");

    expect(store.noteFileState(NOTE)).toBe("present");
  });

  it("drops the failed save's bar before either question renders", async () => {
    // The refused save and the change it was refused over are one event twice.
    const changed = wiredStore();
    await changed.reports("modified");
    expect(changed.forgotten).toEqual([NOTE]);

    const removed = wiredStore();
    await removed.reports("removed");
    expect(removed.forgotten).toEqual([NOTE]);
  });
});

describe("a note that catches up with its file", () => {
  it("stops being asked about once the file's text is in it", async () => {
    const { store, reports } = wiredStore();
    await reports("modified");

    store.applyExternalContent(NOTE, "what the file holds\n");

    expect(store.noteFileState(NOTE)).toBe("present");
  });

  it("stops being asked about once a write lands on the file", async () => {
    const { store, reports } = wiredStore();
    await reports("modified");

    store.noteSaved(NOTE, "a-digest");

    expect(store.noteFileState(NOTE)).toBe("present");
  });

  it("is still deleted after a write that raced the deletion", () => {
    // The queue is cancelled when the deletion arrives, but a call already in
    // flight lands after it. Letting that reply clear the mark would take the
    // bar off a file that is still gone and let the next keystroke queue a
    // write the backend refuses.
    const store = newStore();
    store.recordFileEvent(NOTE, "removed");

    store.noteSaved(NOTE, "a-digest");

    expect(store.noteFileState(NOTE)).toBe("removed");
  });
});
