import { describe, it, expect, vi, beforeEach } from "vitest";

const mocks = vi.hoisted(() => ({
  get: vi.fn(),
  set: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../services/tauri", () => ({
  previewGetScripts: mocks.get,
  previewSetScripts: mocks.set,
}));

import { createPreviewScriptsStore } from "../../stores/global/preview-scripts";

describe("previewScriptsStore.toggle", () => {
  beforeEach(() => {
    mocks.get.mockReset();
    mocks.set.mockReset().mockResolvedValue(undefined);
  });

  it("opts a file in when it was off, remembering it per buffer", async () => {
    mocks.get.mockResolvedValue(false);
    const store = createPreviewScriptsStore();
    expect(await store.toggle("buf-1")).toBe(true);
    expect(mocks.get).toHaveBeenCalledWith("buf-1");
    expect(mocks.set).toHaveBeenCalledWith("buf-1", true);
    expect(store.enabled("buf-1")).toBe(true);
  });

  it("opts a file out when it was on", async () => {
    mocks.get.mockResolvedValue(true);
    const store = createPreviewScriptsStore();
    expect(await store.toggle("buf-2")).toBe(false);
    expect(mocks.set).toHaveBeenCalledWith("buf-2", false);
  });

  it("flips the opt-in the user is shown without reading the stored one", async () => {
    mocks.get.mockResolvedValue(false);
    const store = createPreviewScriptsStore();
    store.settle("buf-4", store.issue(), true);
    expect(await store.toggle("buf-4")).toBe(false);
    expect(mocks.get).not.toHaveBeenCalled();
    expect(mocks.set).toHaveBeenCalledWith("buf-4", false);
  });

  it("propagates a write failure without swallowing it", async () => {
    mocks.get.mockResolvedValue(false);
    mocks.set.mockRejectedValue(new Error("disk full"));
    const store = createPreviewScriptsStore();
    await expect(store.toggle("buf-3")).rejects.toThrow("disk full");
  });
});
