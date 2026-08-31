import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

const { focusHandlers, eventHandlers } = vi.hoisted(() => ({
  focusHandlers: [] as Array<(focused: boolean) => void>,
  eventHandlers: new Map<string, (payload: unknown) => void>(),
}));

vi.mock("../../services/tauri", () => ({
  saveBufferContent: vi.fn().mockResolvedValue(undefined),
  confirmQuitFlush: vi.fn().mockResolvedValue(undefined),
  onWindowCloseRequested: vi.fn().mockResolvedValue(() => {}),
  onWindowFocusChange: vi.fn(async (handler: (focused: boolean) => void) => {
    focusHandlers.push(handler);
    return () => {};
  }),
}));

vi.mock("../../services/events", () => ({
  onEvent: vi.fn(async (kind: string, handler: (payload: unknown) => void) => {
    eventHandlers.set(kind, handler);
    return () => {};
  }),
}));

import { debouncedSave, resetAutosave } from "../../services/autosave";
import { startWindowLifecycle } from "../../services/window-lifecycle";
import { confirmQuitFlush, saveBufferContent } from "../../services/tauri";

const mockedSave = vi.mocked(saveBufferContent);
const mockedConfirm = vi.mocked(confirmQuitFlush);

describe("window lifecycle", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    resetAutosave();
    focusHandlers.length = 0;
    eventHandlers.clear();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("losing_focus_flushes_autosave", async () => {
    await startWindowLifecycle();

    debouncedSave("lifecycle-blur", "typed, not yet written");
    await vi.advanceTimersByTimeAsync(100);
    expect(mockedSave).not.toHaveBeenCalled();

    focusHandlers[0](false);
    await vi.advanceTimersByTimeAsync(0);

    expect(mockedSave).toHaveBeenCalledWith("lifecycle-blur", "typed, not yet written");
  });

  it("regaining_focus_writes_nothing", async () => {
    await startWindowLifecycle();

    debouncedSave("lifecycle-focus", "still being typed");
    focusHandlers[0](true);
    await vi.advanceTimersByTimeAsync(0);

    expect(mockedSave).not.toHaveBeenCalled();
  });

  it("the_quit_flush_event_flushes_then_confirms", async () => {
    await startWindowLifecycle();

    debouncedSave("lifecycle-quit", "first", 100);
    await vi.advanceTimersByTimeAsync(100);
    expect(mockedSave).toHaveBeenCalledOnce();

    // The write above starts the per-note rate cap, so a scheduled save of
    // what follows would sit out the rest of the second. The quit flush is
    // the one path that may not wait: the process is going away.
    debouncedSave("lifecycle-quit", "the last keystrokes", 100);
    await vi.advanceTimersByTimeAsync(100);
    expect(mockedSave).toHaveBeenCalledOnce();

    eventHandlers.get("quit:flush")!({});
    await vi.advanceTimersByTimeAsync(0);

    expect(mockedSave).toHaveBeenLastCalledWith("lifecycle-quit", "the last keystrokes");
    expect(mockedConfirm).toHaveBeenCalledOnce();
  });

  it("a_failed_flush_still_confirms_so_the_quit_does_not_sit_out_the_timeout", async () => {
    await startWindowLifecycle();
    mockedSave.mockRejectedValueOnce(new Error("disk full"));

    debouncedSave("lifecycle-failing", "unwritable");
    eventHandlers.get("quit:flush")!({});
    await vi.advanceTimersByTimeAsync(0);

    expect(mockedConfirm).toHaveBeenCalledOnce();
  });
});
