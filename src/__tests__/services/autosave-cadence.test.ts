import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  saveBufferContent: vi.fn().mockResolvedValue(undefined),
}));

import { debouncedSave, resetAutosave } from "../../services/autosave";
import { saveBufferContent } from "../../services/tauri";

const mockedSave = vi.mocked(saveBufferContent);

describe("autosave cadence", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    resetAutosave();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("default_delay_is_one_second", async () => {
    debouncedSave("cadence-default", "typed");

    await vi.advanceTimersByTimeAsync(999);
    expect(mockedSave).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(1);
    expect(mockedSave).toHaveBeenCalledOnce();
  });

  it("a_paste_burst_writes_at_most_once_per_second", async () => {
    for (let elapsed = 0; elapsed < 3000; elapsed += 100) {
      debouncedSave("cadence-burst", `chunk-${elapsed}`, 100);
      await vi.advanceTimersByTimeAsync(100);
    }

    expect(mockedSave.mock.calls.length).toBeGreaterThan(0);
    expect(mockedSave.mock.calls.length).toBeLessThanOrEqual(3);
  });

  it("continuous_typing_writes_nothing_until_the_user_pauses", async () => {
    for (let keystroke = 0; keystroke < 100; keystroke += 1) {
      debouncedSave("cadence-typing", `keystroke-${keystroke}`);
      await vi.advanceTimersByTimeAsync(50);
    }
    expect(mockedSave).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(1000);
    expect(mockedSave).toHaveBeenCalledOnce();
    expect(mockedSave).toHaveBeenCalledWith("cadence-typing", "keystroke-99");
  });

  it("a_deferred_write_is_re_armed_not_dropped", async () => {
    debouncedSave("cadence-rearm", "first", 100);
    await vi.advanceTimersByTimeAsync(100);
    expect(mockedSave).toHaveBeenCalledOnce();

    debouncedSave("cadence-rearm", "second", 100);
    await vi.advanceTimersByTimeAsync(100);
    expect(mockedSave).toHaveBeenCalledOnce();

    await vi.advanceTimersByTimeAsync(900);
    expect(mockedSave).toHaveBeenCalledTimes(2);
    expect(mockedSave).toHaveBeenLastCalledWith("cadence-rearm", "second");
  });
});
