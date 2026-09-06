import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { armReveal, REVEAL_DEADLINE_MS } from "../../lib/boot-reveal";

describe("armReveal", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("reveals once when the boot steps finish", () => {
    const reveal = vi.fn().mockResolvedValue(undefined);
    const armed = armReveal(reveal);

    armed.now();
    vi.advanceTimersByTime(REVEAL_DEADLINE_MS * 2);

    expect(reveal).toHaveBeenCalledTimes(1);
  });

  it("reveals when a boot step rejects", async () => {
    const reveal = vi.fn().mockResolvedValue(undefined);
    const armed = armReveal(reveal);

    try {
      await Promise.reject(new Error("createTab failed"));
    } catch {
      // the caller's own catch; the reveal is not its business
    } finally {
      armed.now();
    }

    expect(reveal).toHaveBeenCalledTimes(1);
  });

  it("reveals on the deadline when a boot step never settles", () => {
    const reveal = vi.fn().mockResolvedValue(undefined);
    armReveal(reveal);

    void new Promise<void>(() => {});
    expect(reveal).not.toHaveBeenCalled();

    vi.advanceTimersByTime(REVEAL_DEADLINE_MS);

    expect(reveal).toHaveBeenCalledTimes(1);
  });

  it("reveals once when the deadline and the boot steps both arrive", () => {
    const reveal = vi.fn().mockResolvedValue(undefined);
    const armed = armReveal(reveal);

    vi.advanceTimersByTime(REVEAL_DEADLINE_MS);
    armed.now();
    armed.now();

    expect(reveal).toHaveBeenCalledTimes(1);
  });

  it("keeps the boot going when the reveal itself fails", async () => {
    const reveal = vi.fn().mockRejectedValue(new Error("no window"));
    const armed = armReveal(reveal);

    expect(() => armed.now()).not.toThrow();
    await vi.runAllTimersAsync();

    expect(reveal).toHaveBeenCalledTimes(1);
  });
});
