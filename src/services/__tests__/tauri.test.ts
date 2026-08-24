import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

const mockWindow = {
  hide: vi.fn(),
  minimize: vi.fn(),
  startDragging: vi.fn(),
  isMaximized: vi.fn(),
  isMinimized: vi.fn(),
  maximize: vi.fn(),
  unmaximize: vi.fn(),
  isFullscreen: vi.fn(),
  setFullscreen: vi.fn(),
  innerSize: vi.fn(),
  outerSize: vi.fn(),
  scaleFactor: vi.fn(),
  onResized: vi.fn(),
  onFocusChanged: vi.fn(),
  onCloseRequested: vi.fn(),
  destroy: vi.fn(),
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
  maximizeWindow,
  toggleMaximizeWindow,
  isWindowMaximized,
  isWindowMinimized,
  isWindowFullscreen,
  getLogicalWindowSize,
  toggleFullscreenWindow,
  onWindowFocusChange,
  onWindowCloseRequested,
} from "../tauri";

// Everything that still reports goes through logFailure(), which writes one
// string to console.error. A window operation the user drove writes nothing.
let logSpy: ReturnType<typeof vi.spyOn>;
let warnSpy: ReturnType<typeof vi.spyOn>;

function expectOneBareLine() {
  expect(logSpy).toHaveBeenCalledTimes(1);
  const [line, ...rest] = logSpy.mock.calls[0];
  expect(typeof line).toBe("string");
  expect(rest).toEqual([]);
}

beforeEach(() => {
  logSpy = vi.spyOn(console, "error").mockImplementation(() => {});
  warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
  for (const fn of Object.values(mockWindow)) fn.mockReset();
});

afterEach(() => {
  logSpy.mockRestore();
  warnSpy.mockRestore();
});

describe("hideWindow", () => {
  it("stays silent when window.hide rejects", async () => {
    mockWindow.isFullscreen.mockResolvedValueOnce(false);
    mockWindow.hide.mockRejectedValueOnce(new Error("no window"));
    await hideWindow();
    expect(logSpy).not.toHaveBeenCalled();
    expect(warnSpy).not.toHaveBeenCalled();
  });

  it("does not log on the happy path", async () => {
    mockWindow.isFullscreen.mockResolvedValueOnce(false);
    mockWindow.hide.mockResolvedValueOnce(undefined);
    await hideWindow();
    expect(logSpy).not.toHaveBeenCalled();
  });

  it("hides directly when not fullscreen", async () => {
    mockWindow.isFullscreen.mockResolvedValueOnce(false);
    mockWindow.hide.mockResolvedValueOnce(undefined);
    await hideWindow();
    expect(mockWindow.setFullscreen).not.toHaveBeenCalled();
    expect(mockWindow.hide).toHaveBeenCalledOnce();
  });

  it("exits fullscreen before hiding when fullscreen", async () => {
    mockWindow.isFullscreen.mockResolvedValueOnce(true);
    mockWindow.onResized.mockImplementationOnce((cb: () => void) => {
      cb();
      return Promise.resolve(() => {});
    });
    mockWindow.setFullscreen.mockResolvedValueOnce(undefined);
    mockWindow.hide.mockResolvedValueOnce(undefined);
    await hideWindow();
    expect(mockWindow.setFullscreen).toHaveBeenCalledWith(false);
    const exitOrder = mockWindow.setFullscreen.mock.invocationCallOrder[0];
    const hideOrder = mockWindow.hide.mock.invocationCallOrder[0];
    expect(hideOrder).toBeGreaterThan(exitOrder);
  });
});

describe("minimizeWindow", () => {
  it("stays silent when window.minimize rejects", async () => {
    mockWindow.isFullscreen.mockResolvedValueOnce(false);
    mockWindow.minimize.mockRejectedValueOnce(new Error("denied"));
    await minimizeWindow();
    expect(logSpy).not.toHaveBeenCalled();
  });

  it("minimizes directly when not fullscreen", async () => {
    mockWindow.isFullscreen.mockResolvedValueOnce(false);
    mockWindow.minimize.mockResolvedValueOnce(undefined);
    await minimizeWindow();
    expect(mockWindow.setFullscreen).not.toHaveBeenCalled();
    expect(mockWindow.minimize).toHaveBeenCalledOnce();
  });

  it("exits fullscreen before minimizing when fullscreen", async () => {
    mockWindow.isFullscreen.mockResolvedValueOnce(true);
    mockWindow.onResized.mockImplementationOnce((cb: () => void) => {
      cb();
      return Promise.resolve(() => {});
    });
    mockWindow.setFullscreen.mockResolvedValueOnce(undefined);
    mockWindow.minimize.mockResolvedValueOnce(undefined);
    await minimizeWindow();
    expect(mockWindow.setFullscreen).toHaveBeenCalledWith(false);
    const exitOrder = mockWindow.setFullscreen.mock.invocationCallOrder[0];
    const minOrder = mockWindow.minimize.mock.invocationCallOrder[0];
    expect(minOrder).toBeGreaterThan(exitOrder);
  });

  it("waits for the fullscreen-exit transition before minimizing", async () => {
    mockWindow.isFullscreen.mockResolvedValueOnce(true);
    let fireResize: () => void = () => {};
    const unlisten = vi.fn();
    mockWindow.onResized.mockImplementationOnce((cb: () => void) => {
      fireResize = cb;
      return Promise.resolve(unlisten);
    });
    mockWindow.setFullscreen.mockResolvedValueOnce(undefined);
    mockWindow.minimize.mockResolvedValueOnce(undefined);

    const pending = minimizeWindow();
    await new Promise((r) => setTimeout(r, 0));
    expect(mockWindow.minimize).not.toHaveBeenCalled();

    fireResize();
    await pending;
    expect(mockWindow.minimize).toHaveBeenCalledOnce();
    expect(unlisten).toHaveBeenCalledOnce();
  });
});

describe("startDraggingWindow", () => {
  it("stays silent when startDragging rejects", async () => {
    mockWindow.startDragging.mockRejectedValueOnce(new Error("not draggable"));
    await startDraggingWindow();
    expect(logSpy).not.toHaveBeenCalled();
  });
});

describe("maximizeWindow", () => {
  it("maximizes without querying the current state", async () => {
    mockWindow.maximize.mockResolvedValueOnce(undefined);
    await maximizeWindow();
    expect(mockWindow.maximize).toHaveBeenCalledOnce();
    expect(mockWindow.isMaximized).not.toHaveBeenCalled();
    expect(logSpy).not.toHaveBeenCalled();
  });

  it("stays silent when maximize rejects", async () => {
    mockWindow.maximize.mockRejectedValueOnce(new Error("denied"));
    await maximizeWindow();
    expect(logSpy).not.toHaveBeenCalled();
  });
});

describe("toggleMaximizeWindow", () => {
  it("stays silent when isMaximized rejects", async () => {
    mockWindow.isMaximized.mockRejectedValueOnce(new Error("query failed"));
    await toggleMaximizeWindow();
    expect(logSpy).not.toHaveBeenCalled();
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

describe("isWindowMaximized", () => {
  it("reports the window state", async () => {
    mockWindow.isMaximized.mockResolvedValueOnce(true);
    await expect(isWindowMaximized()).resolves.toBe(true);
  });

  // The titlebar reads this on every resize; a rejection must not leave the
  // maximize button stuck, so it degrades to the restored icon.
  it("reports false without a console line when isMaximized rejects", async () => {
    mockWindow.isMaximized.mockRejectedValueOnce(new Error("query failed"));
    await expect(isWindowMaximized()).resolves.toBe(false);
    expect(logSpy).not.toHaveBeenCalled();
  });
});

describe("isWindowMinimized", () => {
  it("reports the window state", async () => {
    mockWindow.isMinimized.mockResolvedValueOnce(true);
    await expect(isWindowMinimized()).resolves.toBe(true);
  });

  it("reports false without a console line when isMinimized rejects", async () => {
    mockWindow.isMinimized.mockRejectedValueOnce(new Error("query failed"));
    await expect(isWindowMinimized()).resolves.toBe(false);
    expect(logSpy).not.toHaveBeenCalled();
  });
});

describe("isWindowFullscreen", () => {
  it("reports the window state", async () => {
    mockWindow.isFullscreen.mockResolvedValueOnce(true);
    await expect(isWindowFullscreen()).resolves.toBe(true);
  });

  it("reports false without a console line when isFullscreen rejects", async () => {
    mockWindow.isFullscreen.mockRejectedValueOnce(new Error("query failed"));
    await expect(isWindowFullscreen()).resolves.toBe(false);
    expect(logSpy).not.toHaveBeenCalled();
  });
});

describe("getLogicalWindowSize", () => {
  // Restore applies the size with set_size, which is the inner rect; reading
  // the outer one would re-save the shadow insets and grow the window on every
  // Windows launch.
  it("measures the inner size, never the outer one", async () => {
    mockWindow.innerSize.mockResolvedValueOnce({ width: 2048, height: 1536 });
    mockWindow.outerSize.mockResolvedValueOnce({ width: 2080, height: 1554 });
    mockWindow.scaleFactor.mockResolvedValueOnce(2);

    await expect(getLogicalWindowSize()).resolves.toEqual({ width: 1024, height: 768 });
    expect(mockWindow.outerSize).not.toHaveBeenCalled();
  });

  it("rounds the logical size at fractional scale factors", async () => {
    mockWindow.innerSize.mockResolvedValueOnce({ width: 1801, height: 1201 });
    mockWindow.scaleFactor.mockResolvedValueOnce(1.5);
    await expect(getLogicalWindowSize()).resolves.toEqual({ width: 1201, height: 801 });
  });

  it("reports null without a console line when the measurement rejects", async () => {
    mockWindow.innerSize.mockRejectedValueOnce(new Error("no window"));
    await expect(getLogicalWindowSize()).resolves.toBeNull();
    expect(logSpy).not.toHaveBeenCalled();
  });
});

describe("toggleFullscreenWindow", () => {
  it("stays silent when isFullscreen rejects", async () => {
    mockWindow.isFullscreen.mockRejectedValueOnce(new Error("query failed"));
    await toggleFullscreenWindow();
    expect(logSpy).not.toHaveBeenCalled();
  });

  it("enters fullscreen when currently windowed", async () => {
    mockWindow.isFullscreen.mockResolvedValueOnce(false);
    mockWindow.setFullscreen.mockResolvedValueOnce(undefined);
    await toggleFullscreenWindow();
    expect(mockWindow.setFullscreen).toHaveBeenCalledWith(true);
  });

  it("exits fullscreen when currently fullscreen", async () => {
    mockWindow.isFullscreen.mockResolvedValueOnce(true);
    mockWindow.setFullscreen.mockResolvedValueOnce(undefined);
    await toggleFullscreenWindow();
    expect(mockWindow.setFullscreen).toHaveBeenCalledWith(false);
  });
});

describe("onWindowFocusChange", () => {
  it("returns a no-op and logs one bare line when subscription fails", async () => {
    mockWindow.onFocusChanged.mockRejectedValueOnce(new Error("no listener"));
    const unlisten = await onWindowFocusChange(() => {});
    expectOneBareLine();
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

describe("onWindowCloseRequested", () => {
  it("preventsDefault, awaits the handler, then destroys the window", async () => {
    let captured: ((e: { preventDefault: () => void }) => Promise<void>) | undefined;
    mockWindow.onCloseRequested.mockImplementationOnce((cb: typeof captured) => {
      captured = cb;
      return Promise.resolve(() => {});
    });
    mockWindow.destroy.mockResolvedValue(undefined);

    const order: string[] = [];
    await onWindowCloseRequested(async () => {
      order.push("handler:start");
      await Promise.resolve();
      order.push("handler:end");
    });

    const event = { preventDefault: vi.fn() };
    await captured!(event);

    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(order).toEqual(["handler:start", "handler:end"]);
    expect(mockWindow.destroy).toHaveBeenCalledOnce();
    const destroyOrder = mockWindow.destroy.mock.invocationCallOrder[0];
    expect(destroyOrder).toBeGreaterThan(event.preventDefault.mock.invocationCallOrder[0]);
  });

  it("still destroys the window when the handler throws", async () => {
    let captured: ((e: { preventDefault: () => void }) => Promise<void>) | undefined;
    mockWindow.onCloseRequested.mockImplementationOnce((cb: typeof captured) => {
      captured = cb;
      return Promise.resolve(() => {});
    });
    mockWindow.destroy.mockResolvedValue(undefined);

    await onWindowCloseRequested(async () => {
      throw new Error("flush failed");
    });

    const event = { preventDefault: vi.fn() };
    await captured!(event);

    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(mockWindow.destroy).toHaveBeenCalledOnce();
  });

  it("ignores re-entry while a close is already in progress", async () => {
    let captured: ((e: { preventDefault: () => void }) => Promise<void>) | undefined;
    mockWindow.onCloseRequested.mockImplementationOnce((cb: typeof captured) => {
      captured = cb;
      return Promise.resolve(() => {});
    });
    mockWindow.destroy.mockResolvedValue(undefined);

    let releaseFlush: () => void = () => {};
    const flushBlocked = new Promise<void>((resolve) => {
      releaseFlush = resolve;
    });

    await onWindowCloseRequested(async () => {
      await flushBlocked;
    });

    const firstEvent = { preventDefault: vi.fn() };
    const secondEvent = { preventDefault: vi.fn() };
    const firstCall = captured!(firstEvent);
    const secondCall = captured!(secondEvent);

    releaseFlush();
    await firstCall;
    await secondCall;

    expect(firstEvent.preventDefault).toHaveBeenCalledOnce();
    expect(secondEvent.preventDefault).toHaveBeenCalledOnce();
    expect(mockWindow.destroy).toHaveBeenCalledOnce();
  });

  it("returns a no-op and logs one bare line when subscription fails", async () => {
    mockWindow.onCloseRequested.mockRejectedValueOnce(new Error("no window"));
    const unlisten = await onWindowCloseRequested(async () => {});
    expectOneBareLine();
    expect(typeof unlisten).toBe("function");
    expect(() => unlisten()).not.toThrow();
  });
});
