import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

const mockWindow = {
  hide: vi.fn(),
  minimize: vi.fn(),
  startDragging: vi.fn(),
  isMaximized: vi.fn(),
  maximize: vi.fn(),
  unmaximize: vi.fn(),
  onFocusChanged: vi.fn(),
};

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => mockWindow,
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

import {
  hideWindow,
  minimizeWindow,
  startDraggingWindow,
  toggleMaximizeWindow,
  onWindowFocusChange,
} from "../tauri";

let warnSpy: ReturnType<typeof vi.spyOn>;

beforeEach(() => {
  warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
  for (const fn of Object.values(mockWindow)) fn.mockReset();
});

afterEach(() => {
  warnSpy.mockRestore();
});

describe("hideWindow", () => {
  it("logs a warning when window.hide rejects", async () => {
    mockWindow.hide.mockRejectedValueOnce(new Error("no window"));
    await hideWindow();
    expect(warnSpy).toHaveBeenCalledWith("hideWindow failed:", expect.any(Error));
  });

  it("does not log on the happy path", async () => {
    mockWindow.hide.mockResolvedValueOnce(undefined);
    await hideWindow();
    expect(warnSpy).not.toHaveBeenCalled();
  });
});

describe("minimizeWindow", () => {
  it("logs a warning when window.minimize rejects", async () => {
    mockWindow.minimize.mockRejectedValueOnce(new Error("denied"));
    await minimizeWindow();
    expect(warnSpy).toHaveBeenCalledWith("minimizeWindow failed:", expect.any(Error));
  });
});

describe("startDraggingWindow", () => {
  it("logs a warning when startDragging rejects", async () => {
    mockWindow.startDragging.mockRejectedValueOnce(new Error("not draggable"));
    await startDraggingWindow();
    expect(warnSpy).toHaveBeenCalledWith("startDraggingWindow failed:", expect.any(Error));
  });
});

describe("toggleMaximizeWindow", () => {
  it("logs a warning when isMaximized rejects", async () => {
    mockWindow.isMaximized.mockRejectedValueOnce(new Error("query failed"));
    await toggleMaximizeWindow();
    expect(warnSpy).toHaveBeenCalledWith("toggleMaximizeWindow failed:", expect.any(Error));
  });

  it("maximizes when currently unmaximized", async () => {
    mockWindow.isMaximized.mockResolvedValueOnce(false);
    mockWindow.maximize.mockResolvedValueOnce(undefined);
    await toggleMaximizeWindow();
    expect(mockWindow.maximize).toHaveBeenCalledOnce();
    expect(mockWindow.unmaximize).not.toHaveBeenCalled();
  });

  it("unmaximizes when currently maximized", async () => {
    mockWindow.isMaximized.mockResolvedValueOnce(true);
    mockWindow.unmaximize.mockResolvedValueOnce(undefined);
    await toggleMaximizeWindow();
    expect(mockWindow.unmaximize).toHaveBeenCalledOnce();
    expect(mockWindow.maximize).not.toHaveBeenCalled();
  });
});

describe("onWindowFocusChange", () => {
  it("returns a no-op and logs when subscription fails", async () => {
    mockWindow.onFocusChanged.mockRejectedValueOnce(new Error("no listener"));
    const unlisten = await onWindowFocusChange(() => {});
    expect(warnSpy).toHaveBeenCalledWith(
      "onWindowFocusChange subscription failed:",
      expect.any(Error),
    );
    expect(typeof unlisten).toBe("function");
    expect(() => unlisten()).not.toThrow();
  });

  it("forwards focus payloads to the handler", async () => {
    let listener: ((arg: { payload: boolean }) => void) | undefined;
    mockWindow.onFocusChanged.mockImplementationOnce((cb: (arg: { payload: boolean }) => void) => {
      listener = cb;
      return Promise.resolve(() => {});
    });
    const calls: boolean[] = [];
    await onWindowFocusChange((focused) => calls.push(focused));
    listener?.({ payload: true });
    listener?.({ payload: false });
    expect(calls).toEqual([true, false]);
  });
});
