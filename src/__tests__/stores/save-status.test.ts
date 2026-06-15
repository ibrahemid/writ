import { describe, it, expect, vi, afterEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  saveBufferContent: vi.fn().mockResolvedValue(undefined),
}));

import { saveStatusStore } from "../../stores/global/save-status";
import { debouncedSave, flushAutosave } from "../../services/autosave";
import { saveBufferContent } from "../../services/tauri";

const mockedSave = vi.mocked(saveBufferContent);

afterEach(() => {
  vi.useRealTimers();
  mockedSave.mockReset();
  mockedSave.mockResolvedValue(undefined);
});

describe("saveStatusStore", () => {
  it("shows 'saved' on autosave success then returns to idle after the visible window", async () => {
    vi.useFakeTimers();
    mockedSave.mockResolvedValue(undefined);

    debouncedSave("buf-saved", "hello", 0);
    await flushAutosave("buf-saved");

    expect(saveStatusStore.status()).toBe("saved");

    await vi.advanceTimersByTimeAsync(1199);
    expect(saveStatusStore.status()).toBe("saved");

    await vi.advanceTimersByTimeAsync(1);
    expect(saveStatusStore.status()).toBe("idle");
  });

  it("shows 'failed' on autosave error and does not auto-clear", async () => {
    vi.useFakeTimers();
    mockedSave.mockRejectedValueOnce(new Error("disk full"));

    debouncedSave("buf-failed", "oops", 0);
    await flushAutosave("buf-failed");

    expect(saveStatusStore.status()).toBe("failed");

    await vi.advanceTimersByTimeAsync(5000);
    expect(saveStatusStore.status()).toBe("failed");
  });
});
