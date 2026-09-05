import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  saveBufferContent: vi.fn().mockResolvedValue(null),
  noteDiskState: vi.fn(async () => ({ state: "no_file" })),
  restoreNoteFile: vi.fn().mockResolvedValue(null),
}));

import { createEditorStore } from "../../stores/window/editor-store";
import {
  cancelAutosave,
  collectUnsavedContent,
  debouncedSave,
  peekUnsavedContent,
  resetAutosave,
  saveNow,
} from "../../services/autosave";
import { handleExternalEdit, type ExternalEditDeps } from "../../services/external-edit";
import { restoreNoteFile, saveBufferContent } from "../../services/tauri";

vi.mock("../../services/autosave", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../services/autosave")>();
  return { ...actual, debouncedSave: vi.fn(actual.debouncedSave) };
});

const queued = vi.mocked(debouncedSave);
const mockedSave = vi.mocked(saveBufferContent);
const mockedRestore = vi.mocked(restoreNoteFile);

type Store = ReturnType<typeof createEditorStore>;

let stores: Store[] = [];

function newStore(): Store {
  const store = createEditorStore();
  stores.push(store);
  return store;
}

/** A store whose editor is showing `id`, with `text` in the view. */
function showing(id: string, text: string): Store {
  const store = newStore();
  const view = {
    state: { doc: { toString: () => text } },
  } as unknown as import("@codemirror/view").EditorView;
  store.registerView(view);
  store.setCurrentBufferId(id);
  return store;
}

/** The deps AppShell builds for `buffer:external`, against a real store. */
function depsFor(store: Store): ExternalEditDeps {
  return {
    findBuffer: (id: string) => ({ id, title: `${id}.md` }),
    hasUnsaved: (id: string) => store.isDirty(id),
    isRemovedOnDisk: (id: string) => store.isRemovedOnDisk(id),
    reload: (id: string) => {
      store.recordFileEvent(id, "settled");
      store.requestExternalReload(id);
    },
    markChanged: (id: string) => store.recordFileEvent(id, "modified"),
    followMove: (id: string) => store.recordFileEvent(id, "moved"),
    markRemoved: (id: string) => store.markRemovedOnDisk(id),
  };
}

function removed(id: string) {
  return { bufferId: id, change: "removed" as const, path: `/notes/${id}.md` };
}

function backAtItsPath(id: string) {
  return { bufferId: id, change: "modified" as const, path: `/notes/${id}.md` };
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

// One test per row of the table in ADR-033 decision 15. The rows about what a
// component adds on top (the view a tab switch rebuilds, the bar's buttons)
// are in removed-on-disk-tab-switch.test.tsx.
describe("a note whose file is gone: the store's machine", () => {
  it("takes the view's text when the removal lands on the tab in front", async () => {
    const store = showing("one", "what the view holds");

    await handleExternalEdit(removed("one"), depsFor(store));

    expect(store.isRemovedOnDisk("one")).toBe(true);
    expect(store.textOfRemoved("one")).toBe("what the view holds");
  });

  it("takes the text of a background tab's refused write", async () => {
    // The row this closes: cancelling the queue drops the text of a write that
    // came back refused, and for a background tab that is the only copy there
    // is. The mark reads it before anything cancels.
    const store = showing("two", "the note in front");
    mockedSave.mockRejectedValue(new Error("ERR_PERMISSION_DENIED: os error 13"));
    await saveNow("one", "the unsaved line");
    expect(peekUnsavedContent("one")).toBe("the unsaved line");

    await handleExternalEdit(removed("one"), depsFor(store));

    expect(store.isRemovedOnDisk("one")).toBe(true);
    expect(store.textOfRemoved("one")).toBe("the unsaved line");
    expect(peekUnsavedContent("one")).toBe("the unsaved line");
  });

  it("claims nothing for a background tab whose text was on the file", async () => {
    const store = showing("two", "the note in front");

    await handleExternalEdit(removed("one"), depsFor(store));

    expect(store.isRemovedOnDisk("one")).toBe(true);
    expect(store.textOfRemoved("one")).toBeUndefined();
  });

  it("keeps the text through a second removal for the same note", async () => {
    const store = showing("one", "what the view holds");
    const deps = depsFor(store);
    await handleExternalEdit(removed("one"), deps);

    await handleExternalEdit(removed("one"), deps);

    expect(store.isRemovedOnDisk("one")).toBe(true);
    expect(store.textOfRemoved("one")).toBe("what the view holds");
    expect(peekUnsavedContent("one")).toBe("what the view holds");
  });

  it("goes back to present when the file turns up at another path", async () => {
    const store = showing("one", "what the view holds");
    const deps = depsFor(store);
    await handleExternalEdit(removed("one"), deps);

    await handleExternalEdit(
      { bufferId: "one", change: "moved", newPath: "/notes/moved/one.md" },
      deps,
    );

    expect(store.isRemovedOnDisk("one")).toBe(false);
    queued.mockClear();
    store.scheduleAutosave("one", "a keystroke at the new path", 0);
    expect(queued).toHaveBeenCalled();
  });

  it("writes what it was holding to the path the file turned up at", async () => {
    // The hold is the only copy by then: the queue went when the mark went on,
    // and the file at the new path is the one the tab was editing before it
    // was renamed. Nothing else would write it, so the move puts it back.
    const store = showing("one", "what the view holds");
    const deps = depsFor(store);
    await handleExternalEdit(removed("one"), deps);
    queued.mockClear();
    mockedSave.mockClear();

    await handleExternalEdit(
      { bufferId: "one", change: "moved", newPath: "/notes/moved/one.md" },
      deps,
    );

    expect(queued).toHaveBeenCalledWith("one", "what the view holds", 0);
    await store.flushAutosave("one");
    expect(mockedSave).toHaveBeenCalledWith("one", "what the view holds");
  });

  it("reads the file back into a tab that had nothing the file did not", async () => {
    const store = showing("one", "what the view holds");
    // Loading the note records both digests, so the tab reads clean.
    store.noteOpened("one", "what the view holds");
    expect(store.isDirty("one")).toBe(false);
    const deps = depsFor(store);
    await handleExternalEdit(removed("one"), deps);

    await handleExternalEdit(backAtItsPath("one"), deps);

    expect(store.isRemovedOnDisk("one")).toBe(false);
    expect(store.externalReload()?.id).toBe("one");
    expect(queued).not.toHaveBeenCalled();
  });

  it("asks about a dirty tab whose file came back holding something else", async () => {
    // The mark used to stay on over a file that was there, so the bar lied and
    // every later keystroke wrote nothing for the life of the window. It comes
    // off, and the tab is asked rather than written: the file holds bytes
    // nobody has compared to the tab's, and picking one for the person is what
    // the three answers exist to avoid.
    const store = showing("one", "the tab's text");
    expect(store.isDirty("one")).toBe(true);
    const deps = depsFor(store);
    await handleExternalEdit(removed("one"), deps);
    queued.mockClear();

    await handleExternalEdit(backAtItsPath("one"), deps);

    expect(store.isRemovedOnDisk("one")).toBe(false);
    expect(store.isFileChangedOnDisk("one")).toBe(true);
    expect(queued).not.toHaveBeenCalled();
    expect(mockedSave).not.toHaveBeenCalled();
    // Still the only copy of the tab's text, and still on its way to the
    // shutdown snapshot rather than to the file.
    expect(peekUnsavedContent("one")).toBe("the tab's text");
  });

  it("keeps a keystroke it does not queue", async () => {
    const store = showing("one", "what the view holds");
    await handleExternalEdit(removed("one"), depsFor(store));
    queued.mockClear();

    store.scheduleAutosave("one", () => "typed after the file went", 0);

    expect(queued).not.toHaveBeenCalled();
    expect(store.textOfRemoved("one")).toBe("typed after the file went");
  });

  it("writes nothing when a tab switch or a quit flushes", async () => {
    const store = showing("one", "what the view holds");
    await handleExternalEdit(removed("one"), depsFor(store));
    store.scheduleAutosave("one", () => "typed after the file went", 0);

    const result = await store.flushAutosave("one");

    expect(result.ok).toBe(true);
    expect(mockedSave).not.toHaveBeenCalled();
    expect(mockedRestore).not.toHaveBeenCalled();
  });

  it("goes back to present when the file is put back", async () => {
    const store = showing("one", "what the view holds");
    await handleExternalEdit(removed("one"), depsFor(store));

    const result = await store.restoreRemovedFile("one");

    expect(result.ok).toBe(true);
    expect(mockedRestore).toHaveBeenCalledWith("one", "what the view holds");
    expect(store.isRemovedOnDisk("one")).toBe(false);
    queued.mockClear();
    store.scheduleAutosave("one", "a keystroke after the file came back", 0);
    expect(queued).toHaveBeenCalled();
  });

  it("stays removed when putting the file back fails", async () => {
    const store = showing("one", "what the view holds");
    await handleExternalEdit(removed("one"), depsFor(store));
    mockedRestore.mockRejectedValueOnce(new Error("ERR_FILE_MISSING: no folder"));

    const result = await store.restoreRemovedFile("one");

    expect(result.ok).toBe(false);
    expect(store.isRemovedOnDisk("one")).toBe(true);
    expect(store.textOfRemoved("one")).toBe("what the view holds");
  });

  it("hands a copy the kept text without leaving the removed state", async () => {
    // What `saveCopyOfNote` reads. The copy goes to the path the person names,
    // so the note's own path is still empty and the tab is still marked.
    const store = showing("one", "what the view holds");
    await handleExternalEdit(removed("one"), depsFor(store));

    expect(store.textOfRemoved("one")).toBe("what the view holds");
    expect(store.isRemovedOnDisk("one")).toBe(true);
  });

  it("takes the view's text before the view is replaced", async () => {
    // What EditorInstance calls on the way out of a tab. The kept text is what
    // the incoming load of this note reads, since there is no file to read.
    const store = showing("one", "what the view holds");
    await handleExternalEdit(removed("one"), depsFor(store));

    store.keepTextOfRemoved("one", "typed just before the switch");
    store.setCurrentBufferId("two");

    expect(store.textOfRemoved("one")).toBe("typed just before the switch");
  });

  it("still has the text when the tab is switched back to", async () => {
    const store = showing("one", "what the view holds");
    await handleExternalEdit(removed("one"), depsFor(store));

    store.setCurrentBufferId("two");
    store.setCurrentBufferId("one");

    expect(store.textOfRemoved("one")).toBe("what the view holds");
  });

  it("hands the text to the shutdown snapshot when the tab closes", async () => {
    const store = showing("one", "what the view holds");
    await handleExternalEdit(removed("one"), depsFor(store));

    store.noteClosed("one");

    expect(store.isRemovedOnDisk("one")).toBe(false);
    expect(peekUnsavedContent("one")).toBe("what the view holds");
  });

  it("hands the text to the shutdown snapshot when the window quits", async () => {
    const store = showing("one", "what the view holds");
    await handleExternalEdit(removed("one"), depsFor(store));
    store.scheduleAutosave("one", () => "typed after the file went", 0);

    await store.flushAutosave();

    expect(collectUnsavedContent()).toEqual([
      { id: "one", content: "typed after the file went" },
    ]);
  });

  it("comes up removed at the next launch, with its text and no write", () => {
    // The launch wrote nothing for this note (ADR-033 decision 15), so the
    // snapshot's text is seeded straight into the store before any tab loads.
    const store = newStore();

    store.markRemovedOnDisk("one", "the line typed after the file went");

    expect(store.isRemovedOnDisk("one")).toBe(true);
    expect(store.textOfRemoved("one")).toBe("the line typed after the file went");
    store.scheduleAutosave("one", () => "and one more", 0);
    expect(queued).not.toHaveBeenCalled();
    expect(mockedSave).not.toHaveBeenCalled();
    expect(mockedRestore).not.toHaveBeenCalled();
  });
});
