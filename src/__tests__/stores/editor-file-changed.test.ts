import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// The file, as the operating system holds it, keyed by note.
const fileOnDisk = new Map<string, string>();

vi.mock("../../services/tauri", () => ({
  saveBufferContent: vi.fn().mockResolvedValue(null),
  noteDiskState: vi.fn(async (id: string) => {
    const text = fileOnDisk.get(id);
    if (text === undefined) return { state: "no_file" };
    const { hashDocument } = await import("../../lib/doc-hash");
    return {
      state: "described",
      disk: { hash: await hashDocument(text), size: text.length, mtime_ms: null },
    };
  }),
}));

import {
  createEditorStore,
  UPDATED_FROM_DISK_MS,
} from "../../stores/window/editor-store";
import { hashDocument } from "../../lib/doc-hash";
import { resetAutosave } from "../../services/autosave";

let stores: Array<{ stopSaveListener: () => void }> = [];

function newStore() {
  const store = createEditorStore();
  stores.push(store);
  return store;
}

/** Waits for the note's record to name `text` as what its file holds. */
async function recordCatchesUp(
  store: ReturnType<typeof createEditorStore>,
  id: string,
  text: string,
) {
  const digest = await hashDocument(text);
  // A budget rather than the one-second default: this waits on a digest and
  // a mocked round trip, and a busy machine takes longer than a second to
  // land both. A test that is genuinely stuck still fails, on the timeout.
  await vi.waitFor(() => expect(store.lastKnownDiskHash(id)).toBe(digest), {
    timeout: 4_000,
    interval: 10,
  });
}

beforeEach(() => {
  fileOnDisk.clear();
  resetAutosave();
});

afterEach(() => {
  for (const store of stores) store.stopSaveListener();
  stores = [];
  resetAutosave();
  vi.useRealTimers();
});

describe("the note whose file changed under unsaved text", () => {
  it("is marked, one note at a time", () => {
    const store = newStore();
    expect(store.isFileChangedOnDisk("one")).toBe(false);

    store.recordFileEvent("one", "modified");
    expect(store.isFileChangedOnDisk("one")).toBe(true);
    expect(store.isFileChangedOnDisk("two")).toBe(false);

    store.recordFileEvent("one", "settled");
    expect(store.isFileChangedOnDisk("one")).toBe(false);
  });

  it("stops being asked about once its tab has gone", () => {
    const store = newStore();
    store.recordFileEvent("one", "modified");

    store.noteClosed("one");

    expect(store.isFileChangedOnDisk("one")).toBe(false);
  });
});

describe("text arriving from the file", () => {
  it("moves the record of a note that is not the one in the view", async () => {
    // Nothing to dispatch into and nothing to show, but the record has to
    // move: a background note left holding the digest of a file that has
    // changed reads dirty against a file it matches, and the next change to
    // it asks a question that has no reason to be asked.
    const store = newStore();
    fileOnDisk.set("one", "as Writ opened it\n");
    store.noteOpened("one", "as Writ opened it\n");
    await recordCatchesUp(store, "one", "as Writ opened it\n");
    expect(store.isDirty("one")).toBe(false);

    fileOnDisk.set("one", "rewritten by another program\n");
    store.applyExternalContent("one", "rewritten by another program\n");

    await recordCatchesUp(store, "one", "rewritten by another program\n");
    expect(store.isDirty("one")).toBe(false);
  });

  it("says so once, and stops saying it", async () => {
    vi.useFakeTimers();
    const store = newStore();
    fileOnDisk.set("one", "from the file\n");

    store.applyExternalContent("one", "from the file\n");
    expect(store.isUpdatedFromDisk("one")).toBe(true);
    expect(store.isUpdatedFromDisk("two")).toBe(false);

    vi.advanceTimersByTime(UPDATED_FROM_DISK_MS - 1);
    expect(store.isUpdatedFromDisk("one")).toBe(true);
    vi.advanceTimersByTime(1);
    expect(store.isUpdatedFromDisk("one")).toBe(false);
  });

  it("reports on the note it last happened to", async () => {
    vi.useFakeTimers();
    const store = newStore();
    fileOnDisk.set("one", "one\n");
    fileOnDisk.set("two", "two\n");

    store.applyExternalContent("one", "one\n");
    store.applyExternalContent("two", "two\n");

    expect(store.isUpdatedFromDisk("one")).toBe(false);
    expect(store.isUpdatedFromDisk("two")).toBe(true);
  });

  it("takes the marker with the tab when it closes", () => {
    vi.useFakeTimers();
    const store = newStore();
    fileOnDisk.set("one", "one\n");
    store.applyExternalContent("one", "one\n");

    store.noteClosed("one");

    expect(store.isUpdatedFromDisk("one")).toBe(false);
  });
});
