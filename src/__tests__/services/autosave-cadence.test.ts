import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  saveBufferContent: vi.fn().mockResolvedValue(undefined),
}));

import { debouncedSave } from "../../services/autosave";
import { saveBufferContent } from "../../services/tauri";

const mockedSave = vi.mocked(saveBufferContent);

describe("autosave cadence", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
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
});
