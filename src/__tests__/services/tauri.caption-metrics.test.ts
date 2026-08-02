import { describe, it, expect, vi, beforeEach } from "vitest";

// Driven through the injected host bridge so the test pins the command name and
// the argument shape the Rust side deserializes, not just the service call.
const hostInvoke = vi.fn();
(globalThis as unknown as { window: Record<string, unknown> }).window.__TAURI_INTERNALS__ = {
  invoke: (cmd: string, args: unknown) => hostInvoke(cmd, args),
  transformCallback: (cb: unknown) => cb,
};

import { reportCaptionButtonMetrics } from "../../services/tauri";

describe("caption button metrics service", () => {
  beforeEach(() => {
    hostInvoke.mockReset();
    hostInvoke.mockResolvedValue(undefined);
  });

  it("sends the measurement under the metrics key the command expects", async () => {
    const metrics = { offsetFromRight: 46, top: 0, width: 46, height: 36 };
    await reportCaptionButtonMetrics(metrics);
    expect(hostInvoke).toHaveBeenCalledWith("set_caption_button_metrics", { metrics });
  });

  it("keeps fractional measurements intact, since Rust scales them by DPI", async () => {
    const metrics = { offsetFromRight: 46.5, top: 0.5, width: 45.75, height: 35.5 };
    await reportCaptionButtonMetrics(metrics);
    expect(hostInvoke.mock.calls[0][1]).toEqual({ metrics });
  });

  // Snap layouts are a nicety; a rejected measurement must not surface as an
  // unhandled rejection in the titlebar's mount path.
  it("warns and continues when the command rejects", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    hostInvoke.mockRejectedValue(new Error("no window"));

    await expect(
      reportCaptionButtonMetrics({ offsetFromRight: 46, top: 0, width: 46, height: 36 }),
    ).resolves.toBeUndefined();
    expect(warn).toHaveBeenCalled();
    warn.mockRestore();
  });
});
