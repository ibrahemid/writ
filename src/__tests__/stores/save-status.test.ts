import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  saveBufferContent: vi.fn().mockResolvedValue(null),
}));

const registry = await vi.hoisted(async () => {
  const { createSignal } = await import("solid-js");
  const [buffers, setBuffers] = createSignal<
    Array<{ id: string; title: string; source_path: string | null }>
  >([]);
  return { buffers, setBuffers };
});

vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: { buffers: registry.buffers },
}));

const dirtyNotes = await vi.hoisted(async () => {
  const { createSignal } = await import("solid-js");
  const [ids, setIds] = createSignal<ReadonlySet<string>>(new Set<string>());
  const [tracked, setTracked] = createSignal<ReadonlySet<string>>(new Set<string>());
  return { ids, setIds, tracked, setTracked };
});

vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: {
    getActive: () => ({
      editor: {
        isDirty: (id: string) => dirtyNotes.ids().has(id),
        isTracked: (id: string) => dirtyNotes.tracked().has(id),
      },
    }),
  },
}));

import { saveStatusStore } from "../../stores/global/save-status";
import {
  debouncedSave,
  flushAutosave,
  cancelAutosave,
  resetAutosave,
} from "../../services/autosave";
import { saveBufferContent } from "../../services/tauri";

const mockedSave = vi.mocked(saveBufferContent);

beforeEach(() => {
  registry.setBuffers([
    { id: "one", title: "one", source_path: "/notes/one.md" },
    { id: "two", title: "two", source_path: "/notes/two.md" },
  ]);
  dirtyNotes.setIds(new Set<string>());
  dirtyNotes.setTracked(new Set(["one", "two"]));
});

afterEach(() => {
  vi.useRealTimers();
  resetAutosave();
  saveStatusStore.reset();
  mockedSave.mockReset();
  mockedSave.mockResolvedValue(null);
});

describe("saveStatusStore", () => {
  it("names the file it is reporting on", () => {
    expect(saveStatusStore.forNote("one").fileName).toBe("one.md");
  });

  it("reads a note nothing has happened to as clean", () => {
    expect(saveStatusStore.forNote("one").state).toBe("clean");
  });

  it("reads a note with unsaved edits as dirty, continuously", () => {
    dirtyNotes.setIds(new Set(["one"]));

    expect(saveStatusStore.forNote("one").state).toBe("dirty");
    expect(saveStatusStore.forNote("one").state).toBe("dirty");
    expect(saveStatusStore.forNote("two").state).toBe("clean");
  });

  it("shows saved after a write lands, then goes back to the note's own state", async () => {
    vi.useFakeTimers();

    debouncedSave("one", "hello", 0);
    await flushAutosave("one");
    expect(saveStatusStore.forNote("one").state).toBe("saved");

    await vi.advanceTimersByTimeAsync(1199);
    expect(saveStatusStore.forNote("one").state).toBe("saved");

    await vi.advanceTimersByTimeAsync(1);
    expect(saveStatusStore.forNote("one").state).toBe("clean");
  });

  it("never reads saved over a document that differs from its file", async () => {
    vi.useFakeTimers();

    debouncedSave("one", "hello", 0);
    await flushAutosave("one");
    expect(saveStatusStore.forNote("one").state).toBe("saved");

    // Typing resumes inside the window the `Saved` label is visible for. The
    // file no longer holds what the person is looking at, and saying it does
    // is the one thing this state may not do.
    dirtyNotes.setIds(new Set(["one"]));
    expect(saveStatusStore.forNote("one").state).toBe("dirty");

    await vi.advanceTimersByTimeAsync(1200);
    expect(saveStatusStore.forNote("one").state).toBe("dirty");
  });

  it("reads a note it holds no record of as clean, whatever the fail-closed predicate says", () => {
    // `isDirty` answers `true` for an untracked note so a reload cannot run
    // over it. A tab restored at launch and never opened is untracked and has
    // nothing unsaved, so it must not wear a mark.
    dirtyNotes.setTracked(new Set<string>());
    dirtyNotes.setIds(new Set(["one"]));

    expect(saveStatusStore.forNote("one").state).toBe("clean");
  });

  it("keeps a failure until a write lands, and marks only the note that failed", async () => {
    vi.useFakeTimers();
    mockedSave.mockImplementation(async (id: string) => {
      if (id === "one") throw new Error("ERR_PERMISSION_DENIED: io error");
      return null;
    });

    debouncedSave("one", "oops", 0);
    debouncedSave("two", "fine", 0);
    await flushAutosave();

    expect(saveStatusStore.forNote("one").state).toBe("failed");
    expect(saveStatusStore.forNote("two").state).toBe("saved");

    await vi.advanceTimersByTimeAsync(5000);
    expect(saveStatusStore.forNote("one").state).toBe("failed");
    expect(saveStatusStore.forNote("two").state).toBe("clean");
    cancelAutosave("one");
  });

  it("carries the reason for the failure in plain words, and drops it once a save lands", async () => {
    vi.useFakeTimers();
    mockedSave.mockRejectedValueOnce(new Error("ERR_PERMISSION_DENIED: io error (os error 13)"));

    debouncedSave("one", "oops", 0);
    await flushAutosave("one");

    const reason = saveStatusStore.forNote("one").reason!;
    expect(reason.code).toBe("ERR_PERMISSION_DENIED");
    expect(reason.message).toBe("you do not have permission to change this file.");
    expect(reason.retryable).toBe(true);

    await flushAutosave("one");

    expect(saveStatusStore.forNote("one").state).toBe("saved");
    expect(saveStatusStore.forNote("one").reason).toBeUndefined();
  });

  it("forgets a note whose tab has gone", async () => {
    vi.useFakeTimers();
    mockedSave.mockRejectedValueOnce(new Error("ERR_WRITE_FAILED: io error"));

    debouncedSave("one", "oops", 0);
    await flushAutosave("one");
    expect(saveStatusStore.forNote("one").state).toBe("failed");

    saveStatusStore.forgetNote("one");
    expect(saveStatusStore.forNote("one").state).toBe("clean");
    cancelAutosave("one");
  });

  it("drops a failure when the same file turns into a question about a change", async () => {
    // One bar for one file. A save refused against a change outside Writ, and
    // the question about that change, are the same event twice: the failure
    // has to go before the question renders, or a note carries two bars
    // saying different things about one file. Both callers that raise a bar
    // (`markChanged`, `markRemoved`) forget the note first.
    vi.useFakeTimers();
    mockedSave.mockRejectedValueOnce(
      new Error("ERR_FILE_CHANGED_ON_DISK: the file changed"),
    );

    debouncedSave("one", "mine", 0);
    await flushAutosave("one");
    expect(saveStatusStore.forNote("one").state).toBe("failed");

    saveStatusStore.forgetNote("one");

    expect(saveStatusStore.forNote("one").state).not.toBe("failed");
    expect(saveStatusStore.failureFor("one")).toBeUndefined();
    cancelAutosave("one");
  });
});
