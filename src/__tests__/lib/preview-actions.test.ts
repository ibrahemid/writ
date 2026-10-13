import { describe, it, expect, vi, beforeEach } from "vitest";

const mocks = vi.hoisted(() => ({
  get: vi.fn(),
  set: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../services/tauri", () => ({
  previewGetScripts: mocks.get,
  previewSetScripts: mocks.set,
}));

import { togglePreviewScripts } from "../../lib/preview-actions";

describe("togglePreviewScripts", () => {
  beforeEach(() => {
    mocks.get.mockReset();
    mocks.set.mockReset().mockResolvedValue(undefined);
  });

  it("opts a file in when it was off, remembering it per buffer", async () => {
    mocks.get.mockResolvedValue(false);
    const result = await togglePreviewScripts("buf-1");
    expect(result).toBe(true);
    expect(mocks.get).toHaveBeenCalledWith("buf-1");
    expect(mocks.set).toHaveBeenCalledWith("buf-1", true);
  });

  it("opts a file out when it was on", async () => {
    mocks.get.mockResolvedValue(true);
    const result = await togglePreviewScripts("buf-2");
    expect(result).toBe(false);
    expect(mocks.set).toHaveBeenCalledWith("buf-2", false);
  });

  it("propagates a write failure without swallowing it", async () => {
    mocks.get.mockResolvedValue(false);
    mocks.set.mockRejectedValue(new Error("disk full"));
    await expect(togglePreviewScripts("buf-3")).rejects.toThrow("disk full");
  });
});
