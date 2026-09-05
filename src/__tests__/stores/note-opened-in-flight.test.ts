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

  it("keeps reading dirty when it has no file and the tab holds its text", async () => {
    // The backend answers `no_file` only for a note that names no file, so a
    // held note never gets it. The tab does not rest on that: two empty
    // digests compare clean, and clean here would let the file's return be
    // read over text nothing else has.
    const store = createEditorStore();
    store.recordFileEvent("one", "removed");
    store.keepTextOfRemoved("one", "the only copy of it\n");

    store.noteOpened("one", "the only copy of it\n");
    await turns();
    backend.pending[0]({ state: "no_file" });
    await turns();

    expect(store.isDirty("one")).toBe(true);
    expect(store.textOfRemoved("one")).toBe("the only copy of it\n");

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

describe("a write landing while the open is still out", () => {
  it("keeps what the write put on the file, not the read that came before it", async () => {
    // The queued write and the open's read describe the same file at two
    // moments, and the write's moment is the later one. Nothing orders them
    // but the record, because a write moves no generation.
    const store = createEditorStore();
    const text = "what the tab holds\n";
    const written = await hashDocument(text);

    store.noteOpened("one", text);
    await turns();

    store.noteSaved("one", written, false);
    await turns();

    backend.pending[0](described("read before the write landed"));
    await turns();

    expect(store.lastKnownDiskHash("one")).toBe(written);
    expect(store.isDirty("one")).toBe(false);

    store.stopSaveListener();
  });

  it("takes the read's digest again on the next open", async () => {
    // The skip belongs to the call that was overtaken and not to the note:
    // an open that starts after the write reads the file the write left.
    const store = createEditorStore();
    const text = "what the tab holds\n";
    const written = await hashDocument(text);

    store.noteOpened("one", text);
    await turns();
    store.noteSaved("one", written, false);
    await turns();
    backend.pending[0](described("read before the write landed"));
    await turns();

    store.noteOpened("one", text);
    await turns();
    backend.pending[1](described("what another program left"));
    await turns();

    expect(store.lastKnownDiskHash("one")).toBe("what another program left");
    expect(store.isDirty("one")).toBe(true);

    store.stopSaveListener();
  });
});
