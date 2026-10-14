import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../tauri", () => ({
  onWindowCloseRequested: vi.fn(),
  recordUnsavedNotes: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../autosave", () => ({
  flushAutosave: vi.fn(),
  collectUnsavedContent: vi.fn(() => []),
}));

import { installCloseFlush } from "../window-lifecycle";
import { onWindowCloseRequested, recordUnsavedNotes } from "../tauri";
import { collectUnsavedContent, flushAutosave } from "../autosave";

const mockedOnClose = vi.mocked(onWindowCloseRequested);
const mockedFlush = vi.mocked(flushAutosave);

describe("installCloseFlush", () => {
  beforeEach(() => {
    mockedOnClose.mockReset();
    mockedFlush.mockReset();
    mockedFlush.mockResolvedValue({ ok: true, failures: [] });
  });

  it("subscribes via onWindowCloseRequested and returns the unlisten", async () => {
    const unlisten = vi.fn();
    mockedOnClose.mockResolvedValueOnce(unlisten);

    const result = await installCloseFlush();

    expect(mockedOnClose).toHaveBeenCalledOnce();
    expect(result).toBe(unlisten);
  });

  it("flushes ALL pending autosaves (no buffer id) when the close handler fires", async () => {
    let captured: (() => Promise<void> | void) | undefined;
    mockedOnClose.mockImplementationOnce(async (handler) => {
      captured = handler;
      return () => {};
    });
    mockedFlush.mockResolvedValueOnce({ ok: true, failures: [] });

    await installCloseFlush();
    expect(captured).toBeDefined();

    await captured!();

    expect(mockedFlush).toHaveBeenCalledOnce();
    expect(mockedFlush).toHaveBeenCalledWith();
  });

  it("runs extra flushes after autosave, awaiting each before the handler resolves", async () => {
    let captured: (() => Promise<void> | void) | undefined;
    mockedOnClose.mockImplementationOnce(async (handler) => {
      captured = handler;
      return () => {};
    });
    const order: string[] = [];
    mockedFlush.mockImplementationOnce(async () => {
      order.push("autosave");
      return { ok: true, failures: [] };
    });
    const geometryFlush = vi.fn(async () => {
      order.push("geometry");
    });

    await installCloseFlush([geometryFlush]);
    await captured!();

    expect(order).toEqual(["autosave", "geometry"]);
    expect(geometryFlush).toHaveBeenCalledOnce();
  });

  it("closes past the deadline when a save never answers, keeping the text that was out", async () => {
    vi.useFakeTimers();
    vi.spyOn(console, "error").mockImplementation(() => {});
    let captured: (() => Promise<void> | void) | undefined;
    mockedOnClose.mockImplementationOnce(async (handler) => {
      captured = handler;
      return () => {};
    });
    mockedFlush.mockImplementationOnce(() => new Promise(() => {}));
    vi.mocked(collectUnsavedContent).mockReturnValueOnce([{ id: "n-1", content: "stalled" }]);
    const extra = vi.fn();

    await installCloseFlush([extra], 100);
    let closed = false;
    void Promise.resolve(captured!()).then(() => {
      closed = true;
    });

    await vi.advanceTimersByTimeAsync(99);
    expect(closed).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    expect(closed).toBe(true);
    expect(vi.mocked(recordUnsavedNotes)).toHaveBeenCalledWith([{ id: "n-1", content: "stalled" }]);
    expect(extra).toHaveBeenCalledOnce();
    vi.useRealTimers();
  });
});

