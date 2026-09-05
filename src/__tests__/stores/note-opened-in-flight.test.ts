import { describe, it, expect, vi, afterEach } from "vitest";

// The file's digest comes from Rust, and these cases are about the window in
// which that answer has not come back yet, so the promises are resolved by
// hand rather than by the runtime.
const backend = vi.hoisted(() => ({
  pending: [] as Array<(answer: unknown) => void>,
}));

vi.mock("../../services/tauri", () => ({
  saveBufferContent: vi.fn().mockResolvedValue(null),
  noteDiskState: vi.fn(
    () =>
      new Promise((resolve) => {
        backend.pending.push(resolve);
      }),
  ),
}));

import { createEditorStore } from "../../stores/window/editor-store";
import { hashDocument } from "../../lib/doc-hash";
import { planExternalEdit } from "../../services/external-edit";
import { resetAutosave } from "../../services/autosave";

/** Lets the store's awaits run without answering any of them. */
async function turns(count = 12): Promise<void> {
  for (let i = 0; i < count; i += 1) {
    await new Promise((resolve) => setTimeout(resolve, 1));
  }
}

function described(hash: string) {
  return { state: "described", disk: { hash, size: 1, mtime_ms: null } };
}

afterEach(() => {
  backend.pending.length = 0;
  resetAutosave();
  vi.clearAllMocks();
});

describe("a note whose open has not answered yet", () => {
  it("reads dirty, so a change arriving in that window is asked about", async () => {
    // The window is a real one: the person clicks a background tab whose bar
    // is up, and the other program writes the file a second time before the
    // note_disk_state round trip returns.
    const store = createEditorStore();
    store.recordFileEvent("one", "modified");
    store.keepTextOfRemoved("one", "typing no file has\n");

    store.noteOpened("one", "typing no file has\n");
    await turns();

    expect(backend.pending.length).toBe(1);
    expect(store.savesAreHeld("one")).toBe(true);
    expect(store.isDirty("one")).toBe(true);
    expect(
      planExternalEdit({
        change: "modified",
        known: true,
        hasUnsaved: store.isDirty("one"),
        removedOnDisk: store.isRemovedOnDisk("one"),
      }),
    ).toBe("prompt");

    store.stopSaveListener();
  });

  it("is not shown as differing from its file until it answers", async () => {
    // The fail-closed answer is for the reload decision and not for the tab's
    // mark, which is what `isTracked` is asked first to keep out of it.
    const store = createEditorStore();
    store.noteOpened("one", "as Writ opened it\n");
    await turns();

    expect(store.isTracked("one")).toBe(false);

    backend.pending[0](described(await hashDocument("as Writ opened it\n")));
    await turns();

    expect(store.isTracked("one")).toBe(true);
    expect(store.isDirty("one")).toBe(false);

    store.stopSaveListener();
  });

  it("stops reading dirty once its file turns out not to exist", async () => {
    const store = createEditorStore();
    store.noteOpened("one", "never saved\n");
    await turns();

    backend.pending[0]({ state: "no_file" });
    await turns();

    expect(store.isDirty("one")).toBe(false);
    expect(store.isTracked("one")).toBe(true);

    store.stopSaveListener();
  });
});

describe("two opens of one note in flight at once", () => {
  it("keeps the later one's answer when the earlier one comes back last", async () => {
    // The reload of a background tab opens the note a second time
    // (`applyExternalContent`), so two round trips for one note is the
    // ordinary case and not a contrived one.
    const store = createEditorStore();
    const asOpened = await hashDocument("as Writ opened it\n");
    const asRewritten = await hashDocument("rewritten while it was behind\n");

    store.noteOpened("one", "as Writ opened it\n");
    await turns();
    store.applyExternalContent("one", "rewritten while it was behind\n");
    await turns();
    expect(backend.pending.length).toBe(2);

    // The reload's answer lands first, then the open's, which is the order
    // that lets the older one win.
    backend.pending[1](described(asRewritten));
    await turns();
    expect(store.lastKnownDiskHash("one")).toBe(asRewritten);

    backend.pending[0](described(asOpened));
    await turns();

    expect(store.lastKnownDiskHash("one")).toBe(asRewritten);
    expect(store.docHash("one")).toBe(asRewritten);
    expect(store.isDirty("one")).toBe(false);

    store.stopSaveListener();
  });

  it("does not let a superseded open drop the record the later one filled", async () => {
    // The other exit from the same async body. A call that fails after being
    // overtaken must not take the live record with it.
    const store = createEditorStore();
    const asRewritten = await hashDocument("rewritten while it was behind\n");

    store.noteOpened("one", "as Writ opened it\n");
    await turns();
    store.applyExternalContent("one", "rewritten while it was behind\n");
    await turns();

    backend.pending[1](described(asRewritten));
    await turns();

    backend.pending[0]({ state: "undescribed" });
    await turns();

    expect(store.isTracked("one")).toBe(true);
    expect(store.lastKnownDiskHash("one")).toBe(asRewritten);

    store.stopSaveListener();
  });
});
