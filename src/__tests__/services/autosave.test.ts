import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  saveBufferContent: vi.fn().mockResolvedValue(undefined),
}));

import {
  debouncedSave,
  cancelAutosave,
  onAutosaveError,
  flushAutosave,
} from "../../services/autosave";
import { saveBufferContent } from "../../services/tauri";

const mockedSave = vi.mocked(saveBufferContent);

describe("autosave", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  describe("debouncedSave", () => {
    it("calls saveBufferContent after the delay", async () => {
      debouncedSave("buf-1", "hello", 300);

      expect(mockedSave).not.toHaveBeenCalled();

      await vi.advanceTimersByTimeAsync(300);

      expect(mockedSave).toHaveBeenCalledOnce();
      expect(mockedSave).toHaveBeenCalledWith("buf-1", "hello");
    });

    it("resets the timer on rapid calls", async () => {
      debouncedSave("buf-1", "v1", 300);
      await vi.advanceTimersByTimeAsync(200);

      debouncedSave("buf-1", "v2", 300);
      await vi.advanceTimersByTimeAsync(200);

      expect(mockedSave).not.toHaveBeenCalled();

      await vi.advanceTimersByTimeAsync(100);

      expect(mockedSave).toHaveBeenCalledOnce();
      expect(mockedSave).toHaveBeenCalledWith("buf-1", "v2");
    });

    it("handles separate buffers independently", async () => {
      debouncedSave("buf-a", "content-a", 100);
      debouncedSave("buf-b", "content-b", 100);

      await vi.advanceTimersByTimeAsync(100);

      expect(mockedSave).toHaveBeenCalledTimes(2);
      expect(mockedSave).toHaveBeenCalledWith("buf-a", "content-a");
      expect(mockedSave).toHaveBeenCalledWith("buf-b", "content-b");
    });

    it("notifies error listeners when save fails", async () => {
      mockedSave.mockRejectedValueOnce(new Error("disk full"));
      const listener = vi.fn();
      const unsubscribe = onAutosaveError(listener);

      debouncedSave("buf-1", "data", 50);
      await vi.advanceTimersByTimeAsync(50);

      expect(listener).toHaveBeenCalledWith("buf-1", expect.any(Error));
      unsubscribe();
    });
  });

  describe("cancelAutosave", () => {
    it("prevents a pending save from firing", async () => {
      debouncedSave("buf-1", "content", 300);

      cancelAutosave("buf-1");

      await vi.advanceTimersByTimeAsync(300);

      expect(mockedSave).not.toHaveBeenCalled();
    });

    it("does nothing for a buffer with no pending save", () => {
      expect(() => cancelAutosave("unknown")).not.toThrow();
    });

    it("cancels only the targeted buffer", async () => {
      debouncedSave("buf-a", "a", 100);
      debouncedSave("buf-b", "b", 100);

      cancelAutosave("buf-a");

      await vi.advanceTimersByTimeAsync(100);

      expect(mockedSave).toHaveBeenCalledOnce();
      expect(mockedSave).toHaveBeenCalledWith("buf-b", "b");
    });
  });

  describe("flushAutosave", () => {
    it("fires a pending save immediately for the given buffer", async () => {
      debouncedSave("buf-1", "fresh", 300);

      expect(mockedSave).not.toHaveBeenCalled();

      await flushAutosave("buf-1");

      expect(mockedSave).toHaveBeenCalledOnce();
      expect(mockedSave).toHaveBeenCalledWith("buf-1", "fresh");

      await vi.advanceTimersByTimeAsync(300);
      expect(mockedSave).toHaveBeenCalledOnce();
    });

    it("flushes every pending buffer when called without an id", async () => {
      debouncedSave("buf-a", "a", 300);
      debouncedSave("buf-b", "b", 300);

      await flushAutosave();

      expect(mockedSave).toHaveBeenCalledTimes(2);
      expect(mockedSave).toHaveBeenCalledWith("buf-a", "a");
      expect(mockedSave).toHaveBeenCalledWith("buf-b", "b");
    });

    it("is a no-op when nothing is pending", async () => {
      await flushAutosave();
      await flushAutosave("nobody");
      expect(mockedSave).not.toHaveBeenCalled();
    });
  });

  describe("lazy content source", () => {
    it("materializes the getter at fire time, not at schedule time", async () => {
      let live = "v1";
      const getter = vi.fn(() => live);
      debouncedSave("buf-1", getter, 300);

      // Not yet read: scheduling must not force a materialization.
      expect(getter).not.toHaveBeenCalled();

      live = "v2";
      await vi.advanceTimersByTimeAsync(300);

      expect(getter).toHaveBeenCalledOnce();
      expect(mockedSave).toHaveBeenCalledWith("buf-1", "v2");
    });

    it("flush reads the latest value from the getter", async () => {
      let live = "first";
      debouncedSave("buf-1", () => live, 300);
      live = "latest";

      await flushAutosave("buf-1");

      expect(mockedSave).toHaveBeenCalledWith("buf-1", "latest");
    });

    it("a throwing getter notifies error listeners and saves nothing", async () => {
      const listener = vi.fn();
      const unsubscribe = onAutosaveError(listener);
      debouncedSave("buf-1", () => {
        throw new Error("view destroyed");
      }, 50);

      await vi.advanceTimersByTimeAsync(50);

      expect(mockedSave).not.toHaveBeenCalled();
      expect(listener).toHaveBeenCalledWith("buf-1", expect.any(Error));
      unsubscribe();
    });
  });
});
