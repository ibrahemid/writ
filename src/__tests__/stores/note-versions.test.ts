import { describe, it, expect, vi, beforeEach } from "vitest";

// Reads overtake each other whenever the user arrows down a list or restores a
// version, so every write the store makes has to belong to the read the panel
// is actually waiting for.

const h = vi.hoisted(() => ({
  noteVersions: vi.fn(),
  noteVersionContent: vi.fn(),
  restoreNoteVersion: vi.fn(),
  copyNoteVersion: vi.fn(),
  logFailure: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  noteVersions: h.noteVersions,
  noteVersionContent: h.noteVersionContent,
  restoreNoteVersion: h.restoreNoteVersion,
  copyNoteVersion: h.copyNoteVersion,
}));

vi.mock("../../lib/log", () => ({ logFailure: h.logFailure }));

import { noteVersionsStore } from "../../stores/global/note-versions";
import type { NoteVersion } from "../../services/tauri";

function version(id: number, atMs = 1_700_000_000_000): NoteVersion {
  return { id, at_ms: atMs, bytes: 1_200 };
}

function held<T>(): { promise: Promise<T>; settle: (value: T) => void; fail: () => void } {
  let settle!: (value: T) => void;
  let fail!: () => void;
  const promise = new Promise<T>((resolve, reject) => {
    settle = resolve;
    fail = () => reject(new Error("the blob is gone"));
  });
  promise.catch(() => {});
  return { promise, settle, fail };
}

async function flush(times = 6): Promise<void> {
  for (let i = 0; i < times; i += 1) await Promise.resolve();
}

beforeEach(() => {
  noteVersionsStore.clear();
  h.noteVersions.mockReset();
  h.noteVersionContent.mockReset();
  h.logFailure.mockReset();
});

describe("a read the panel has moved past", () => {
  it("says nothing about a version the user already left", async () => {
    const left = held<string>();
    const landed = held<string>();
    h.noteVersionContent.mockReturnValueOnce(left.promise).mockReturnValueOnce(landed.promise);

    void noteVersionsStore.select(1);
    void noteVersionsStore.select(2);

    left.fail();
    await flush();

    expect(h.logFailure).not.toHaveBeenCalled();

    landed.settle("what version 2 said\n");
    await flush();
    expect(noteVersionsStore.text()).toBe("what version 2 said\n");
  });

  it("still says so when the read the panel is waiting for fails", async () => {
    h.noteVersionContent.mockRejectedValueOnce(new Error("the blob is gone"));

    await noteVersionsStore.select(1);

    expect(h.logFailure).toHaveBeenCalledTimes(1);
    expect(noteVersionsStore.text()).toBe("");
  });
});

describe("a read still open when the list is re-read", () => {
  it("cannot write the pane during a restore's reload", async () => {
    // restore() re-reads the list, and the read the old row started is still
    // open across that window.
    const open = held<string>();
    h.noteVersionContent.mockReturnValueOnce(open.promise);
    void noteVersionsStore.select(7);

    const list = held<NoteVersion[]>();
    h.noteVersions.mockReturnValueOnce(list.promise);
    void noteVersionsStore.load("/notes/Launch.md");
    await flush();

    open.settle("the version the note held before the restore\n");
    await flush();

    expect(noteVersionsStore.text()).toBe("");
    expect(noteVersionsStore.selected()).toBeNull();

    h.noteVersionContent.mockResolvedValueOnce("the restored text\n");
    list.settle([version(9)]);
    await flush();

    expect(noteVersionsStore.text()).toBe("the restored text\n");
    expect(noteVersionsStore.selected()).toBe(9);
  });
});
