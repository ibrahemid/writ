import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  showWindow: vi.fn().mockResolvedValue(undefined),
  hideWindow: vi.fn().mockResolvedValue(undefined),
  minimizeWindow: vi.fn().mockResolvedValue(undefined),
  toggleMaximizeWindow: vi.fn().mockResolvedValue(undefined),
  isWindowMaximized: vi.fn().mockResolvedValue(false),
  toggleFullscreenWindow: vi.fn().mockResolvedValue(undefined),
  startDraggingWindow: vi.fn().mockResolvedValue(undefined),
  onWindowFocusChange: vi.fn(),
  getLogicalWindowSize: vi.fn(),
  setLogicalWindowSize: vi.fn().mockResolvedValue(undefined),
  getLogicalWindowPosition: vi.fn(),
  setLogicalWindowPosition: vi.fn().mockResolvedValue(undefined),
  computeWindowPlacement: vi.fn(),
  centerWindow: vi.fn().mockResolvedValue(undefined),
  onWindowResized: vi.fn(),
  onWindowMoved: vi.fn(),
  reportCaptionButtonMetrics: vi.fn(),
}));

vi.mock("../../services/events", () => ({
  onEvent: vi.fn(),
}));

vi.mock("../../stores/global/config", () => ({
  configStore: {
    config: vi.fn(),
    save: vi.fn().mockResolvedValue(undefined),
  },
}));

import * as api from "../../services/tauri";
import { onEvent } from "../../services/events";
import { configStore } from "../../stores/global/config";
import { osWindowStore } from "../../stores/global/os-window";
import type { CaptionHitPhase } from "../../types/events";

const apiMock = api as unknown as Record<string, ReturnType<typeof vi.fn>>;
const onEventMock = onEvent as unknown as ReturnType<typeof vi.fn>;
const configMock = configStore as unknown as {
  config: ReturnType<typeof vi.fn>;
  save: ReturnType<typeof vi.fn>;
};

beforeEach(() => {
  for (const fn of Object.values(apiMock)) {
    if (typeof fn?.mockReset === "function") fn.mockReset();
  }
  apiMock.showWindow.mockResolvedValue(undefined);
  apiMock.hideWindow.mockResolvedValue(undefined);
  apiMock.minimizeWindow.mockResolvedValue(undefined);
  apiMock.toggleMaximizeWindow.mockResolvedValue(undefined);
  apiMock.isWindowMaximized.mockResolvedValue(false);
  apiMock.toggleFullscreenWindow.mockResolvedValue(undefined);
  apiMock.startDraggingWindow.mockResolvedValue(undefined);
  apiMock.setLogicalWindowSize.mockResolvedValue(undefined);
  apiMock.setLogicalWindowPosition.mockResolvedValue(undefined);
  apiMock.centerWindow.mockResolvedValue(undefined);
  apiMock.reportCaptionButtonMetrics.mockResolvedValue(undefined);
  onEventMock.mockReset();
  onEventMock.mockResolvedValue(() => {});
  configMock.config.mockReset();
  configMock.save.mockReset();
  configMock.save.mockResolvedValue(undefined);
});

describe("osWindowStore actions", () => {
  it("hide delegates to api.hideWindow exactly once", async () => {
    await osWindowStore.hide();
    expect(apiMock.hideWindow).toHaveBeenCalledTimes(1);
  });

  it("minimize delegates to api.minimizeWindow exactly once", async () => {
    await osWindowStore.minimize();
    expect(apiMock.minimizeWindow).toHaveBeenCalledTimes(1);
  });

  it("toggleMaximize delegates to api.toggleMaximizeWindow exactly once", async () => {
    await osWindowStore.toggleMaximize();
    expect(apiMock.toggleMaximizeWindow).toHaveBeenCalledTimes(1);
  });

  it("toggleFullscreen delegates to api.toggleFullscreenWindow exactly once", async () => {
    await osWindowStore.toggleFullscreen();
    expect(apiMock.toggleFullscreenWindow).toHaveBeenCalledTimes(1);
  });

  it("startDragging delegates to api.startDraggingWindow exactly once", async () => {
    await osWindowStore.startDragging();
    expect(apiMock.startDraggingWindow).toHaveBeenCalledTimes(1);
  });
});

describe("osWindowStore maximize sync", () => {
  it("installMaximizeSync seeds the signal without blocking on IPC", async () => {
    vi.useFakeTimers();
    try {
      apiMock.onWindowResized.mockResolvedValue(() => {});
      apiMock.isWindowMaximized.mockResolvedValue(true);

      const unlisten = await osWindowStore.installMaximizeSync();
      expect(typeof unlisten).toBe("function");
      // Seeding is scheduled, not awaited: the cold path must not wait on it.
      expect(apiMock.isWindowMaximized).not.toHaveBeenCalled();

      await vi.advanceTimersByTimeAsync(100);
      expect(osWindowStore.maximized()).toBe(true);

      unlisten();
    } finally {
      vi.useRealTimers();
    }
  });

  it("coalesces a resize storm into one read", async () => {
    vi.useFakeTimers();
    try {
      let resize: (() => void) | undefined;
      apiMock.onWindowResized.mockImplementation((cb: () => void) => {
        resize = cb;
        return Promise.resolve(() => {});
      });
      apiMock.isWindowMaximized.mockResolvedValue(false);

      const unlisten = await osWindowStore.installMaximizeSync();
      await vi.advanceTimersByTimeAsync(100);
      apiMock.isWindowMaximized.mockClear();
      apiMock.isWindowMaximized.mockResolvedValue(true);

      for (let i = 0; i < 20; i++) resize?.();
      await vi.advanceTimersByTimeAsync(99);
      expect(apiMock.isWindowMaximized).not.toHaveBeenCalled();

      await vi.advanceTimersByTimeAsync(1);
      expect(apiMock.isWindowMaximized).toHaveBeenCalledTimes(1);
      expect(osWindowStore.maximized()).toBe(true);

      unlisten();
    } finally {
      vi.useRealTimers();
    }
  });

  // A maximize that lands on the bounds the window already had emits no resize,
  // so the toggle has to settle the signal itself or the button lies.
  it("re-reads after toggleMaximize without waiting for a resize event", async () => {
    apiMock.onWindowResized.mockResolvedValue(() => {});
    apiMock.isWindowMaximized.mockResolvedValue(false);
    const unlisten = await osWindowStore.installMaximizeSync();

    apiMock.isWindowMaximized.mockResolvedValue(true);
    await osWindowStore.toggleMaximize();

    expect(osWindowStore.maximized()).toBe(true);
    unlisten();
  });

  it("cancels a pending read on unlisten so a torn-down store stops polling", async () => {
    vi.useFakeTimers();
    try {
      let resize: (() => void) | undefined;
      apiMock.onWindowResized.mockImplementation((cb: () => void) => {
        resize = cb;
        return Promise.resolve(() => {});
      });
      apiMock.isWindowMaximized.mockResolvedValue(false);

      const unlisten = await osWindowStore.installMaximizeSync();
      await vi.advanceTimersByTimeAsync(100);
      apiMock.isWindowMaximized.mockClear();

      resize?.();
      unlisten();
      await vi.advanceTimersByTimeAsync(200);

      expect(apiMock.isWindowMaximized).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("osWindowStore snap overlay", () => {
  const METRICS = { offsetFromRight: 46, top: 0, width: 46, height: 36 };

  async function install(): Promise<{ push: (phase: CaptionHitPhase) => void; dispose: () => void }> {
    let push: ((payload: { phase: CaptionHitPhase }) => void) | undefined;
    onEventMock.mockImplementation((_kind: string, handler: (p: { phase: CaptionHitPhase }) => void) => {
      push = handler;
      return Promise.resolve(() => {});
    });
    const dispose = await osWindowStore.installSnapOverlay(METRICS);
    return { push: (phase) => push?.({ phase }), dispose };
  }

  it("reports the measured button before subscribing", async () => {
    const { dispose } = await install();
    expect(apiMock.reportCaptionButtonMetrics).toHaveBeenCalledWith(METRICS);
    expect(onEventMock).toHaveBeenCalledWith("titlebar:maximize-hit", expect.any(Function));
    dispose();
  });

  it("tracks hover and press from the overlay's phases", async () => {
    const { push, dispose } = await install();
    expect(osWindowStore.snapHovered()).toBe(false);

    push("enter");
    expect(osWindowStore.snapHovered()).toBe(true);

    push("press");
    expect(osWindowStore.snapPressed()).toBe(true);

    push("leave");
    expect(osWindowStore.snapHovered()).toBe(false);
    expect(osWindowStore.snapPressed()).toBe(false);
    dispose();
  });

  it("toggles the window on click and keeps the hover the cursor is still in", async () => {
    const { push, dispose } = await install();
    push("enter");
    push("press");
    push("click");

    expect(apiMock.toggleMaximizeWindow).toHaveBeenCalledTimes(1);
    expect(osWindowStore.snapPressed()).toBe(false);
    expect(osWindowStore.snapHovered()).toBe(true);
    dispose();
  });

  it("clears the visual state on teardown so a torn-down button is not left lit", async () => {
    const { push, dispose } = await install();
    push("enter");
    push("press");
    dispose();

    expect(osWindowStore.snapHovered()).toBe(false);
    expect(osWindowStore.snapPressed()).toBe(false);
  });
});

describe("osWindowStore focus sync", () => {
  it("focused() defaults to true", () => {
    expect(osWindowStore.focused()).toBe(true);
  });

  it("installFocusSync wires onWindowFocusChange to update the signal", async () => {
    let pushed: ((focused: boolean) => void) | undefined;
    apiMock.onWindowFocusChange.mockImplementation(
      (handler: (f: boolean) => void) => {
        pushed = handler;
        return Promise.resolve(() => {});
      },
    );

    const unlisten = await osWindowStore.installFocusSync();
    expect(typeof unlisten).toBe("function");
    expect(apiMock.onWindowFocusChange).toHaveBeenCalledTimes(1);

    pushed?.(false);
    expect(osWindowStore.focused()).toBe(false);

    pushed?.(true);
    expect(osWindowStore.focused()).toBe(true);
  });
});

// Window-geometry restore moved to Rust (`restore_main_window_geometry` /
// `window_state::place_window`, covered by window_state unit tests) so the
// window is sized while still hidden and the frontend never round-trips IPC to
// resize on the cold path. The store no longer owns restoreSize.
describe("osWindowStore reveal", () => {
  it("delegates to api.showWindow exactly once", async () => {
    await osWindowStore.reveal();
    expect(apiMock.showWindow).toHaveBeenCalledTimes(1);
  });
});

describe("osWindowStore installGeometryPersistence", () => {
  it("debounces resize/move events and writes position + size to configStore", async () => {
    vi.useFakeTimers();
    try {
      let resize: (() => void) | undefined;
      let move: (() => void) | undefined;
      apiMock.onWindowResized.mockImplementation((cb: () => void) => {
        resize = cb;
        return Promise.resolve(() => {});
      });
      apiMock.onWindowMoved.mockImplementation((cb: () => void) => {
        move = cb;
        return Promise.resolve(() => {});
      });
      configMock.config.mockReturnValue({ window: { width: 800, height: 600, x: 100, y: 100 } });
      apiMock.getLogicalWindowSize.mockResolvedValue({ width: 1024, height: 768 });
      apiMock.getLogicalWindowPosition.mockResolvedValue({ x: 300, y: 220 });

      const unlisten = await osWindowStore.installGeometryPersistence();
      expect(typeof unlisten).toBe("function");

      move?.();
      resize?.();

      await vi.advanceTimersByTimeAsync(499);
      expect(configMock.save).not.toHaveBeenCalled();

      await vi.advanceTimersByTimeAsync(1);
      await vi.runOnlyPendingTimersAsync();
      await Promise.resolve();

      expect(configMock.save).toHaveBeenCalledTimes(1);
      expect(configMock.save).toHaveBeenCalledWith(
        expect.objectContaining({ window: { width: 1024, height: 768, x: 300, y: 220 } }),
      );

      unlisten();
    } finally {
      vi.useRealTimers();
    }
  });

  it("does not write when measured geometry equals stored geometry", async () => {
    vi.useFakeTimers();
    try {
      let resize: (() => void) | undefined;
      apiMock.onWindowResized.mockImplementation((cb: () => void) => {
        resize = cb;
        return Promise.resolve(() => {});
      });
      apiMock.onWindowMoved.mockImplementation(() => Promise.resolve(() => {}));
      configMock.config.mockReturnValue({ window: { width: 800, height: 600, x: 100, y: 100 } });
      apiMock.getLogicalWindowSize.mockResolvedValue({ width: 800, height: 600 });
      apiMock.getLogicalWindowPosition.mockResolvedValue({ x: 100, y: 100 });

      const unlisten = await osWindowStore.installGeometryPersistence();
      resize?.();
      await vi.advanceTimersByTimeAsync(500);
      await Promise.resolve();

      expect(configMock.save).not.toHaveBeenCalled();
      unlisten();
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("osWindowStore flushGeometry (close-flush regression)", () => {
  it("writes the pending geometry immediately instead of waiting for the debounce", async () => {
    vi.useFakeTimers();
    try {
      let move: (() => void) | undefined;
      apiMock.onWindowResized.mockImplementation(() => Promise.resolve(() => {}));
      apiMock.onWindowMoved.mockImplementation((cb: () => void) => {
        move = cb;
        return Promise.resolve(() => {});
      });
      configMock.config.mockReturnValue({ window: { width: 800, height: 600, x: 100, y: 100 } });
      apiMock.getLogicalWindowSize.mockResolvedValue({ width: 800, height: 600 });
      apiMock.getLogicalWindowPosition.mockResolvedValue({ x: 640, y: 360 });

      const unlisten = await osWindowStore.installGeometryPersistence();

      move?.();
      await vi.advanceTimersByTimeAsync(100);
      expect(configMock.save).not.toHaveBeenCalled();

      await osWindowStore.flushGeometry();

      expect(configMock.save).toHaveBeenCalledTimes(1);
      expect(configMock.save).toHaveBeenCalledWith(
        expect.objectContaining({ window: { width: 800, height: 600, x: 640, y: 360 } }),
      );

      unlisten();
    } finally {
      vi.useRealTimers();
    }
  });

  it("cancels the pending debounce so a later tick does not double-write", async () => {
    vi.useFakeTimers();
    try {
      let move: (() => void) | undefined;
      apiMock.onWindowResized.mockImplementation(() => Promise.resolve(() => {}));
      apiMock.onWindowMoved.mockImplementation((cb: () => void) => {
        move = cb;
        return Promise.resolve(() => {});
      });
      configMock.config.mockReturnValue({ window: { width: 800, height: 600, x: 100, y: 100 } });
      apiMock.getLogicalWindowSize.mockResolvedValue({ width: 800, height: 600 });
      apiMock.getLogicalWindowPosition.mockResolvedValue({ x: 640, y: 360 });

      const unlisten = await osWindowStore.installGeometryPersistence();
      move?.();
      await osWindowStore.flushGeometry();
      expect(configMock.save).toHaveBeenCalledTimes(1);

      await vi.advanceTimersByTimeAsync(500);
      await vi.runOnlyPendingTimersAsync();
      await Promise.resolve();

      expect(configMock.save).toHaveBeenCalledTimes(1);
      unlisten();
    } finally {
      vi.useRealTimers();
    }
  });
});
