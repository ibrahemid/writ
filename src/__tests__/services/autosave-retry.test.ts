import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// A write that fails for a reason that passes is tried again on its own, with
// a growing wait, until it lands; text that is out when the window stops
// waiting still reaches the unsaved-text journal.

vi.mock("../../services/tauri", () => ({
  saveBufferContent: vi.fn(),
  recordUnsavedNotes: vi.fn().mockResolvedValue(undefined),
  discardUnsavedText: vi.fn().mockResolvedValue(undefined),
}));

import {
  RETRY_FIRST_MS,
  RETRY_MAX_MS,
  cancelAutosave,
  collectUnsavedContent,
  debouncedSave,
  flushAutosave,
  resetAutosave,
} from "../../services/autosave";
import { saveBufferContent } from "../../services/tauri";

const mockedSave = vi.mocked(saveBufferContent);

beforeEach(() => {
  vi.useFakeTimers();
  vi.clearAllMocks();
  resetAutosave();
});

afterEach(() => {
  resetAutosave();
  vi.useRealTimers();
});

describe("a retryable save failure", () => {
  it("is written again on its own, waiting longer each time, until it lands", async () => {
    mockedSave
      .mockRejectedValueOnce(new Error("ERR_FILE_IN_USE: held"))
      .mockRejectedValueOnce(new Error("ERR_FILE_IN_USE: held"))
      .mockResolvedValue("hash");

    debouncedSave("n-1", "the text", 0);
    await vi.advanceTimersByTimeAsync(0);
    expect(mockedSave).toHaveBeenCalledTimes(1);

    await vi.advanceTimersByTimeAsync(RETRY_FIRST_MS - 1);
    expect(mockedSave).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(mockedSave).toHaveBeenCalledTimes(2);

    await vi.advanceTimersByTimeAsync(RETRY_FIRST_MS * 2 - 1);
    expect(mockedSave).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(1);
    expect(mockedSave).toHaveBeenCalledTimes(3);
    expect(mockedSave).toHaveBeenLastCalledWith("n-1", "the text");

    await vi.advanceTimersByTimeAsync(RETRY_MAX_MS * 2);
    expect(mockedSave).toHaveBeenCalledTimes(3);
  });

  it("never waits longer than the cap between tries", async () => {
    mockedSave.mockRejectedValue(new Error("ERR_FILE_IN_USE: held"));
    debouncedSave("n-1", "the text", 0);
    await vi.advanceTimersByTimeAsync(0);
    for (let tries = 0; tries < 8; tries += 1) await vi.advanceTimersByTimeAsync(RETRY_MAX_MS);
    const before = mockedSave.mock.calls.length;
    await vi.advanceTimersByTimeAsync(RETRY_MAX_MS);
    expect(mockedSave.mock.calls.length).toBe(before + 1);
    cancelAutosave("n-1");
  });

  it("stops when the note's text is discarded", async () => {
    mockedSave.mockRejectedValue(new Error("ERR_FILE_IN_USE: held"));
    debouncedSave("n-1", "the text", 0);
    await vi.advanceTimersByTimeAsync(0);
    cancelAutosave("n-1");
    await vi.advanceTimersByTimeAsync(RETRY_MAX_MS * 4);
    expect(mockedSave).toHaveBeenCalledTimes(1);
  });
});

describe("a write that never answers", () => {
  it("still hands its text to the close path", async () => {
    mockedSave.mockImplementation(() => new Promise(() => {}));
    debouncedSave("n-1", "text on a stalled share", 0);
    await vi.advanceTimersByTimeAsync(0);
    void flushAutosave();

    expect(collectUnsavedContent()).toEqual([{ id: "n-1", content: "text on a stalled share" }]);
  });
});
