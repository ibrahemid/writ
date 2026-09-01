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
  return { ids, setIds };
});

vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: {
    getActive: () => ({ editor: { isDirty: (id: string) => dirtyNotes.ids().has(id) } }),
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
});
