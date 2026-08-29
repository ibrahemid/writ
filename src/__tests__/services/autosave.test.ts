import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  saveBufferContent: vi.fn().mockResolvedValue(undefined),
}));

import {
  debouncedSave,
  cancelAutosave,
  onAutosaveError,
  onAutosaveSuccess,
  flushAutosave,
  hasPendingAutosave,
  saveNow,
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
      cancelAutosave("buf-1");
    });
  });

  describe("onAutosaveSuccess", () => {
    it("notifies success listeners with the buffer id after a save lands", async () => {
      const listener = vi.fn();
      const unsubscribe = onAutosaveSuccess(listener);

      debouncedSave("buf-ok", "content", 50);
      await vi.advanceTimersByTimeAsync(50);

      expect(listener).toHaveBeenCalledOnce();
      expect(listener).toHaveBeenCalledWith("buf-ok");
      unsubscribe();
    });

    it("does not notify success listeners when the save fails", async () => {
      mockedSave.mockRejectedValueOnce(new Error("disk full"));
      const success = vi.fn();
      const unsubscribe = onAutosaveSuccess(success);

      debouncedSave("buf-fail", "content", 50);
      await vi.advanceTimersByTimeAsync(50);

      expect(success).not.toHaveBeenCalled();
      unsubscribe();
      cancelAutosave("buf-fail");
    });

    it("stops notifying after unsubscribe", async () => {
      const listener = vi.fn();
      const unsubscribe = onAutosaveSuccess(listener);
      unsubscribe();

      debouncedSave("buf-gone", "content", 50);
      await vi.advanceTimersByTimeAsync(50);

      expect(listener).not.toHaveBeenCalled();
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
      cancelAutosave("buf-1");
    });
  });

  describe("failed writes keep the text", () => {
    it("reports the failure through the flush result", async () => {
      mockedSave.mockRejectedValueOnce(new Error("path not authorized"));
      debouncedSave("buf-keep", "precious", 300);

      const result = await flushAutosave("buf-keep");

      expect(result.ok).toBe(false);
      expect(result.failures).toEqual([
        { bufferId: "buf-keep", error: expect.any(Error) },
      ]);
      cancelAutosave("buf-keep");
    });

    it("retries the same text on the next flush", async () => {
      mockedSave.mockRejectedValueOnce(new Error("disk full"));
      debouncedSave("buf-keep", "precious", 300);

      const failed = await flushAutosave("buf-keep");
      expect(failed.ok).toBe(false);
      expect(hasPendingAutosave("buf-keep")).toBe(true);

      const retried = await flushAutosave("buf-keep");

      expect(retried.ok).toBe(true);
      expect(mockedSave).toHaveBeenCalledTimes(2);
      expect(mockedSave).toHaveBeenLastCalledWith("buf-keep", "precious");
      expect(hasPendingAutosave("buf-keep")).toBe(false);
    });

    it("keeps the newer text when an edit lands during a failing write", async () => {
      let rejectWrite: ((reason: Error) => void) | undefined;
      mockedSave.mockImplementationOnce(
        () =>
          new Promise((_resolve, reject) => {
            rejectWrite = reject;
          }),
      );

      debouncedSave("buf-race", "old", 300);
      const flushing = flushAutosave("buf-race");
      await Promise.resolve();

      debouncedSave("buf-race", "new", 300);
      rejectWrite!(new Error("disk full"));
      await flushing;

      await flushAutosave("buf-race");

      expect(mockedSave).toHaveBeenLastCalledWith("buf-race", "new");
      expect(hasPendingAutosave("buf-race")).toBe(false);
    });

    it("writes the newer text when two writes for one buffer both fail", async () => {
      // Cmd+S landing on top of an in-flight autosave that is failing. The
      // older attempt must not put its stale text back over the newer text.
      let rejectFirst: ((reason: Error) => void) | undefined;
      let rejectSecond: ((reason: Error) => void) | undefined;
      mockedSave.mockImplementationOnce(
        () => new Promise((_resolve, reject) => { rejectFirst = reject; }),
      );
      mockedSave.mockImplementationOnce(
        () => new Promise((_resolve, reject) => { rejectSecond = reject; }),
      );

      debouncedSave("p1", "old", 300);
      const firstFlush = flushAutosave("p1");
      await Promise.resolve();
      expect(rejectFirst).toBeDefined();

      const explicit = saveNow("p1", "new");
      await Promise.resolve();

      rejectFirst!(new Error("disk full"));
      await firstFlush;
      // The second write only starts once the first settles: one write per
      // buffer at a time.
      await Promise.resolve();
      await Promise.resolve();
      expect(rejectSecond).toBeDefined();
      rejectSecond!(new Error("disk full"));
      await explicit;

      mockedSave.mockClear();
      const retried = await flushAutosave("p1");

      expect(retried.ok).toBe(true);
      expect(mockedSave).toHaveBeenCalledOnce();
      expect(mockedSave).toHaveBeenCalledWith("p1", "new");
      expect(hasPendingAutosave("p1")).toBe(false);
    });

    it("never runs two writes for one buffer at the same time", async () => {
      let inFlightCount = 0;
      let peak = 0;
      mockedSave.mockImplementation(async () => {
        inFlightCount++;
        peak = Math.max(peak, inFlightCount);
        await Promise.resolve();
        inFlightCount--;
      });

      debouncedSave("p2", "a", 300);
      const first = flushAutosave("p2");
      const second = saveNow("p2", "b");
      const third = flushAutosave("p2");
      await Promise.all([first, second, third]);

      expect(peak).toBe(1);
      expect(mockedSave).toHaveBeenLastCalledWith("p2", "b");
      mockedSave.mockReset();
      mockedSave.mockResolvedValue(undefined);
    });

    it("reports the in-flight failure to a flush that arrives during the write", async () => {
      let rejectWrite: ((reason: Error) => void) | undefined;
      mockedSave.mockImplementationOnce(
        () => new Promise((_resolve, reject) => { rejectWrite = reject; }),
      );
      mockedSave.mockImplementationOnce(
        () => Promise.reject(new Error("disk full")),
      );

      debouncedSave("p3", "precious", 300);
      const writing = flushAutosave("p3");
      await Promise.resolve();

      const closing = flushAutosave("p3");
      rejectWrite!(new Error("disk full"));

      expect((await writing).ok).toBe(false);
      const result = await closing;
      expect(result.ok).toBe(false);
      expect(result.failures[0].bufferId).toBe("p3");
      expect(hasPendingAutosave("p3")).toBe(true);
      cancelAutosave("p3");
    });

    it("drops the failed text when the buffer was cancelled meanwhile", async () => {
      let rejectWrite: ((reason: Error) => void) | undefined;
      mockedSave.mockImplementationOnce(
        () => new Promise((_resolve, reject) => { rejectWrite = reject; }),
      );

      debouncedSave("p4", "discarded", 300);
      const writing = flushAutosave("p4");
      await Promise.resolve();

      cancelAutosave("p4");
      rejectWrite!(new Error("disk full"));
      await writing;

      expect(hasPendingAutosave("p4")).toBe(false);
    });

    it("does not try a save the write guard stopped again until the document changes", async () => {
      mockedSave.mockRejectedValueOnce(
        new Error("the file changed on disk: /Users/x/Writ/shared.md"),
      );

      debouncedSave("guarded", "mine", 300);
      await vi.advanceTimersByTimeAsync(300);

      expect(mockedSave).toHaveBeenCalledTimes(1);
      expect(hasPendingAutosave("guarded")).toBe(false);

      // Flushing on tab close, window hide or quit must not write it again:
      // every attempt lands another copy beside the note and stops again.
      const flushed = await flushAutosave("guarded");
      expect(flushed.ok).toBe(true);
      expect(mockedSave).toHaveBeenCalledTimes(1);

      // The next keystroke is a new document, and it is written.
      debouncedSave("guarded", "mine, edited", 300);
      await vi.advanceTimersByTimeAsync(300);
      expect(mockedSave).toHaveBeenCalledTimes(2);
      expect(mockedSave).toHaveBeenLastCalledWith("guarded", "mine, edited");
    });

    it("keeps the text of any other failed write for the next flush", async () => {
      mockedSave.mockRejectedValueOnce(new Error("io error: permission denied"));

      debouncedSave("kept", "mine", 300);
      await vi.advanceTimersByTimeAsync(300);

      expect(hasPendingAutosave("kept")).toBe(true);
      cancelAutosave("kept");
    });

    it("resolves ok when nothing was pending", async () => {
      const result = await flushAutosave("buf-absent");
      expect(result).toEqual({ ok: true, failures: [] });
    });

    it("reports every failure when flushing all buffers", async () => {
      mockedSave.mockRejectedValueOnce(new Error("one"));
      mockedSave.mockRejectedValueOnce(new Error("two"));
      debouncedSave("buf-x", "x", 300);
      debouncedSave("buf-y", "y", 300);

      const result = await flushAutosave();

      expect(result.ok).toBe(false);
      expect(result.failures.map((f) => f.bufferId).sort()).toEqual(["buf-x", "buf-y"]);
      cancelAutosave("buf-x");
      cancelAutosave("buf-y");
    });
  });
});
