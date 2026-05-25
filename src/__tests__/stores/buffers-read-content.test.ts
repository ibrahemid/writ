import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  listActiveBuffers: vi.fn().mockResolvedValue([]),
  listHistory: vi.fn().mockResolvedValue([]),
  createBuffer: vi.fn(),
  closeBuffer: vi.fn(),
  closeBuffers: vi.fn(),
  restoreBuffer: vi.fn(),
  deleteBuffer: vi.fn(),
  clearHistory: vi.fn(),
  renameBuffer: vi.fn(),
  openFile: vi.fn(),
  showOpenFileDialog: vi.fn(),
  readBufferContent: vi.fn(),
}));

vi.mock("../../services/autosave", () => ({
  flushAutosave: vi.fn().mockResolvedValue(undefined),
}));

import * as api from "../../services/tauri";
import { bufferStore } from "../../stores/buffers";

const apiMock = api as unknown as Record<string, ReturnType<typeof vi.fn>>;

beforeEach(() => {
  apiMock.readBufferContent.mockReset();
});

describe("bufferStore.readContent", () => {
  it("delegates to api.readBufferContent and returns the resolved string", async () => {
    apiMock.readBufferContent.mockResolvedValue("hello world");
    const text = await bufferStore.readContent("buf-1");
    expect(apiMock.readBufferContent).toHaveBeenCalledWith("buf-1");
    expect(text).toBe("hello world");
  });

  it("propagates rejection from the service", async () => {
    apiMock.readBufferContent.mockRejectedValue(new Error("not found"));
    await expect(bufferStore.readContent("missing")).rejects.toThrow("not found");
  });
});
