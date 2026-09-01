import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  saveBufferContent: vi.fn().mockResolvedValue(null),
}));

import { createEditorStore, DOC_HASH_IDLE_MS } from "../../stores/window/editor-store";
import { hashDocument } from "../../lib/doc-hash";
import { debouncedSave, flushAutosave, resetAutosave } from "../../services/autosave";
import { saveBufferContent } from "../../services/tauri";

const mockedSave = vi.mocked(saveBufferContent);

// crypto.subtle resolves off the event loop rather than on a microtask, so a
// turn of the loop is what a landed hash has to be waited on with.
function turn(ms = 0): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/** Waits until the note's document digest is the one `text` hashes to. */
async function hashedAs(
  store: { docHash: (id: string) => string | undefined },
  id: string,
  text: string,
): Promise<void> {
  const want = await hashDocument(text);
  await vi.waitFor(() => expect(store.docHash(id)).toBe(want), { timeout: 2000 });
}

let stores: Array<{ stopSaveListener: () => void }> = [];

function newStore() {
  const store = createEditorStore();
  stores.push(store);
  return store;
}

beforeEach(() => {
  resetAutosave();
  mockedSave.mockReset();
  mockedSave.mockResolvedValue(null);
});

afterEach(() => {
  for (const store of stores) store.stopSaveListener();
  stores = [];
  resetAutosave();
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

  it("is clean once a freshly opened note has been hashed", async () => {
    const store = newStore();
    store.noteOpened("a", "hello");
    await hashedAs(store, "a", "hello");

    expect(store.lastKnownDiskHash("a")).toBe(store.docHash("a"));
    expect(store.isDirty("a")).toBe(false);
  });

  it("reads dirty for a character typed inside the idle window, before the hash lands", async () => {
    const store = newStore();
    store.noteOpened("a", "hello");
    await hashedAs(store, "a", "hello");
    expect(store.isDirty("a")).toBe(false);

    store.noteEdited("a", "hello!");

    expect(store.isDirty("a")).toBe(true);
    expect(store.docHash("a")).toBe(await hashDocument("hello"));
  });

  it("stays dirty while the edit differs from the file, before and after the hash", async () => {
    const store = newStore();
    store.noteOpened("a", "hello");
    await hashedAs(store, "a", "hello");

    store.noteEdited("a", "hello!");
    expect(store.isDirty("a")).toBe(true);
    await hashedAs(store, "a", "hello!");

    expect(store.isDirty("a")).toBe(true);
  });

  it("goes clean again when the document is typed back to what the file holds", async () => {
    const store = newStore();
    store.noteOpened("a", "hello");
    await hashedAs(store, "a", "hello");

    store.noteEdited("a", "hello!");
    await hashedAs(store, "a", "hello!");
    expect(store.isDirty("a")).toBe(true);

    store.noteEdited("a", "hello");
    await hashedAs(store, "a", "hello");
    expect(store.isDirty("a")).toBe(false);
  });

  it("counts a differing document as dirty with the queue empty and a write just resolved", async () => {
    const store = newStore();
    store.noteOpened("a", "hello");
    await hashedAs(store, "a", "hello");

    mockedSave.mockResolvedValue(await hashDocument("written"));
    debouncedSave("a", "written", 0);
    await flushAutosave("a");
    store.noteEdited("a", "typed after the write");
    await hashedAs(store, "a", "typed after the write");

    expect(store.lastKnownDiskHash("a")).toBe(await hashDocument("written"));
    expect(store.isDirty("a")).toBe(true);
  });

  it("is clean after a write of exactly what the document holds", async () => {
    const store = newStore();
    store.noteOpened("a", "hello");
    await hashedAs(store, "a", "hello");

    store.noteEdited("a", "hello!");
    await hashedAs(store, "a", "hello!");

    mockedSave.mockResolvedValue(await hashDocument("hello!"));
    debouncedSave("a", "hello!", 0);
    await flushAutosave("a");
    await turn();

    expect(store.isDirty("a")).toBe(false);
  });

  it("tracks two notes apart, including the one that is not in front", async () => {
    const store = newStore();
    store.noteOpened("a", "one");
    store.noteOpened("b", "two");
    await hashedAs(store, "a", "one");
    await hashedAs(store, "b", "two");

    store.noteEdited("b", "two edited");
    await hashedAs(store, "b", "two edited");

    expect(store.isDirty("a")).toBe(false);
    expect(store.isDirty("b")).toBe(true);
  });

  it("reads a reloaded note as clean again", async () => {
    const store = newStore();
    store.noteOpened("a", "hello");
    await hashedAs(store, "a", "hello");
    store.noteEdited("a", "typed");
    await hashedAs(store, "a", "typed");
    expect(store.isDirty("a")).toBe(true);

    store.noteOpened("a", "what the file now holds");
    await hashedAs(store, "a", "what the file now holds");
    expect(store.isDirty("a")).toBe(false);
  });

  it("forgets a note whose tab has gone", async () => {
    const store = newStore();
    store.noteOpened("a", "hello");
    await hashedAs(store, "a", "hello");
    store.noteClosed("a");

    expect(store.docHash("a")).toBeUndefined();
    expect(store.isTracked("a")).toBe(false);
  });


  it("drops a hash that lands after a newer edit", async () => {
    const store = newStore();
    store.noteOpened("a", "hello");
    await hashedAs(store, "a", "hello");

    const firstHash = await hashDocument("first");
    let release = () => {};
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const realDigest = crypto.subtle.digest.bind(crypto.subtle);
    const digest = vi
      .spyOn(crypto.subtle, "digest")
      .mockImplementation(async (algorithm, data) => {
        await gate;
        return realDigest(algorithm as AlgorithmIdentifier, data as BufferSource);
      });

    store.noteEdited("a", "first");
    await turn(DOC_HASH_IDLE_MS + 20);
    // The digest for "first" is in flight; a keystroke lands before it returns.
    store.noteEdited("a", "second");
    release();
    await turn();

    expect(store.isDirty("a")).toBe(true);
    expect(store.docHash("a")).not.toBe(firstHash);
    digest.mockRestore();
  });

  it("does not move the record of the file when a save wrote nothing", async () => {
    const store = newStore();
    store.noteOpened("a", "hello");
    await hashedAs(store, "a", "hello");
    const before = store.lastKnownDiskHash("a");

    store.noteSaved("a", null);

    expect(store.lastKnownDiskHash("a")).toBe(before);
  });
});

describe("hashDocument", () => {
  it("matches the digest Rust produces for the same bytes", async () => {
    // The value crates/writ-core/src/hash.rs computes for b"writ".
    expect(await hashDocument("writ")).toBe(
      "bd998adfd46eaff944ee5125a9ffaff2ecbf67d11ee8a2a3d2afd0946bc8adbb",
    );
  });

  it("encodes the text as UTF-8, which is what a save writes", async () => {
    const utf8 = new TextEncoder().encode("héllo نص");
    const digest = await crypto.subtle.digest("SHA-256", utf8);
    const expected = Array.from(new Uint8Array(digest))
      .map((byte) => byte.toString(16).padStart(2, "0"))
      .join("");
    expect(await hashDocument("héllo نص")).toBe(expected);
  });

  it("reads a file with CRLF line endings as the document CodeMirror holds", async () => {
    expect(await hashDocument("a\r\nb")).toBe(await hashDocument("a\nb"));
  });
});
