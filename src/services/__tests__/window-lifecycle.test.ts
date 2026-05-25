import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../tauri", () => ({
  onWindowCloseRequested: vi.fn(),
}));

vi.mock("../autosave", () => ({
  flushAutosave: vi.fn(),
}));

import { installCloseFlush } from "../window-lifecycle";
import { onWindowCloseRequested } from "../tauri";
import { flushAutosave } from "../autosave";

const mockedOnClose = vi.mocked(onWindowCloseRequested);
const mockedFlush = vi.mocked(flushAutosave);

describe("installCloseFlush", () => {
  beforeEach(() => {
    mockedOnClose.mockReset();
    mockedFlush.mockReset();
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
    mockedFlush.mockResolvedValueOnce(undefined);

    await installCloseFlush();
    expect(captured).toBeDefined();

    await captured!();

    expect(mockedFlush).toHaveBeenCalledOnce();
    expect(mockedFlush).toHaveBeenCalledWith();
  });
});
