import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  saveBufferContent: vi.fn().mockResolvedValue(null),
  noteDiskState: vi.fn(async () => ({ state: "no_file" })),
  restoreNoteFile: vi.fn().mockResolvedValue(null),
  saveNoteTo: vi.fn().mockResolvedValue("digest"),
  recordUnsavedNotes: vi.fn().mockResolvedValue(undefined),
  discardUnsavedText: vi.fn().mockResolvedValue(undefined),
}));

import { createEditorStore } from "../../stores/window/editor-store";
import {
  collectUnsavedContent,
  keepUnsavedForRecovery,
  resetAutosave,
} from "../../services/autosave";
import {
  discardUnsavedText,
  recordUnsavedNotes,
  saveBufferContent,
  saveNoteTo,
} from "../../services/tauri";

const mockedRecord = vi.mocked(recordUnsavedNotes);
const mockedDiscard = vi.mocked(discardUnsavedText);
const mockedSave = vi.mocked(saveBufferContent);
const mockedSaveTo = vi.mocked(saveNoteTo);

type Store = ReturnType<typeof createEditorStore>;

let stores: Store[] = [];

/** A store whose editor is showing `id`, with `text` in the view. */
function showing(id: string, text: string): Store {
  const store = createEditorStore();
  stores.push(store);
  const view = {
    state: { doc: { toString: () => text } },
  } as unknown as import("@codemirror/view").EditorView;
  store.registerView(view);
  store.setCurrentBufferId(id);
  return store;
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.clearAllMocks();
  resetAutosave();
});

afterEach(() => {
  for (const store of stores) store.stopSaveListener();
  stores = [];
  resetAutosave();
  vi.useRealTimers();
});

// ADR-044 §2: text a bar holds back from every write reaches the journal on
// the autosave's debounce, as the text of a save that could not land does.
describe("text held under a bar", () => {
  it("reaches the journal on the autosave's debounce, not at the close", async () => {
    const store = showing("one", "what the view holds");
    store.markRemovedOnDisk("one");
    await vi.advanceTimersByTimeAsync(1000);
    mockedRecord.mockClear();

    store.scheduleAutosave("one", "typed under the bar", 500);
    await vi.advanceTimersByTimeAsync(499);
    expect(mockedRecord).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);

    expect(mockedRecord).toHaveBeenCalledWith([{ id: "one", content: "typed under the bar" }]);
    expect(mockedSave).not.toHaveBeenCalled();
  });

  it("is journaled from the moment the hold starts", async () => {
    const store = showing("one", "what the view holds");
    store.recordFileEvent("one", "modified");

    await vi.advanceTimersByTimeAsync(1000);

    expect(mockedRecord).toHaveBeenCalledWith([{ id: "one", content: "what the view holds" }]);
  });

  it("goes from the journal when the tab takes the file's text", async () => {
    const store = showing("one", "what the view holds");
    store.markRemovedOnDisk("one");
    await vi.advanceTimersByTimeAsync(1000);
    expect(mockedRecord).toHaveBeenCalled();

    store.recordFileEvent("one", "settled");

    expect(mockedDiscard).toHaveBeenCalledWith(["one"]);
  });

  it("stays for the write that carries it when the file turns up at another path", async () => {
    const store = showing("one", "what the view holds");
    store.markRemovedOnDisk("one");
    await vi.advanceTimersByTimeAsync(1000);

    store.recordFileEvent("one", "moved");
    await vi.advanceTimersByTimeAsync(1000);

    expect(mockedDiscard).not.toHaveBeenCalled();
    expect(mockedSave).toHaveBeenCalledWith("one", "what the view holds");
  });

  it("ends with nothing to drop when the hold ends before the debounce", async () => {
    const store = showing("one", "what the view holds");
    store.markRemovedOnDisk("one");

    store.recordFileEvent("one", "settled");
    await vi.advanceTimersByTimeAsync(1000);

    expect(mockedRecord).not.toHaveBeenCalled();
    expect(mockedDiscard).not.toHaveBeenCalled();
  });
});

// ADR-044 §4: a note whose first file the launch could not make is held like a
// deleted one, named by the file it was to be, and "Save As…" gives it a file.
describe("a note whose first file could not be made", () => {
  const unmade = { name: "2026-10-13.txt", folder: "Writ" };

  it("is held and named by the file it was to be", () => {
    const store = showing("two", "the note in front");
    store.markRemovedOnDisk("one", "the first words", unmade);

    expect(store.isRemovedOnDisk("one")).toBe(true);
    expect(store.savesAreHeld("one")).toBe(true);
    expect(store.textOfRemoved("one")).toBe("the first words");
    expect(store.unmadeFileOf("one")).toEqual(unmade);
    expect(store.unmadeFileOf("two")).toBeNull();
  });

  it("a deleted note is not named as one that was never made", () => {
    const store = showing("two", "the note in front");
    store.markRemovedOnDisk("one", "kept text");

    expect(store.unmadeFileOf("one")).toBeNull();
  });

  it("is written where the person picked, and the hold ends", async () => {
    const store = showing("two", "the note in front");
    store.markRemovedOnDisk("one", "the first words", unmade);

    const result = await store.saveRemovedNoteTo("one", "/Users/me/Documents/Chosen.txt");

    expect(result.ok).toBe(true);
    expect(mockedSaveTo).toHaveBeenCalledWith(
      "one",
      "the first words",
      "/Users/me/Documents/Chosen.txt",
    );
    expect(store.isRemovedOnDisk("one")).toBe(false);
    expect(store.unmadeFileOf("one")).toBeNull();
  });

  it("stays held and named when the write does not land", async () => {
    mockedSaveTo.mockRejectedValueOnce(new Error("ERR_FOLDER_NOT_WRITABLE: /Users/me/Locked"));
    const store = showing("two", "the note in front");
    store.markRemovedOnDisk("one", "the first words", unmade);

    const result = await store.saveRemovedNoteTo("one", "/Users/me/Locked/Chosen.txt");

    expect(result.ok).toBe(false);
    expect(store.isRemovedOnDisk("one")).toBe(true);
    expect(store.unmadeFileOf("one")).toEqual(unmade);
  });
});

// A tab restored at launch that Writ has not read has no text of its own, and
// one whose read was refused for its size keeps what its refused save left in
// the journal (ADR-044 §2, ADR-019).
describe("a tab whose file Writ has not read", () => {
  it("hands nothing over, whatever happens to its file", async () => {
    const store = showing("two", "the note in front");

    store.recordFileEvent("one", "modified");
    await vi.advanceTimersByTimeAsync(1000);
    store.recordFileEvent("one", "removed");
    await vi.advanceTimersByTimeAsync(1000);
    store.recordFileEvent("one", "settled");
    await keepUnsavedForRecovery("one");

    expect(collectUnsavedContent()).toEqual([]);
    expect(mockedRecord).not.toHaveBeenCalled();
    expect(mockedDiscard).not.toHaveBeenCalled();
  });

  it("keeps what its refused save left once its read is refused for size", async () => {
    mockedSave.mockRejectedValueOnce(new Error("ERR_READ_ONLY_DESTINATION: read-only"));
    const store = showing("one", "typed");
    store.scheduleAutosave("one", "typed", 0);
    await vi.advanceTimersByTimeAsync(0);
    expect(mockedSave).toHaveBeenCalledWith("one", "typed");

    // The editor drops the view of a tab whose read was refused for size.
    store.registerView(null);
    store.setCurrentBufferId(null);
    store.recordFileEvent("one", "modified");
    await vi.advanceTimersByTimeAsync(1000);
    await keepUnsavedForRecovery("one");

    expect(mockedRecord).toHaveBeenCalled();
    for (const [notes] of mockedRecord.mock.calls) {
      expect(notes).toEqual([{ id: "one", content: "typed" }]);
    }
    expect(mockedDiscard).not.toHaveBeenCalled();
  });
});
