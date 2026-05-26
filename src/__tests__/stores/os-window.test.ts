import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  hideWindow: vi.fn().mockResolvedValue(undefined),
  minimizeWindow: vi.fn().mockResolvedValue(undefined),
  toggleMaximizeWindow: vi.fn().mockResolvedValue(undefined),
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
}));

vi.mock("../../stores/global/config", () => ({
  configStore: {
    config: vi.fn(),
    save: vi.fn().mockResolvedValue(undefined),
  },
}));

import * as api from "../../services/tauri";
import { configStore } from "../../stores/global/config";
import { osWindowStore } from "../../stores/global/os-window";

const apiMock = api as unknown as Record<string, ReturnType<typeof vi.fn>>;
const configMock = configStore as unknown as {
  config: ReturnType<typeof vi.fn>;
  save: ReturnType<typeof vi.fn>;
};

beforeEach(() => {
  for (const fn of Object.values(apiMock)) {
    if (typeof fn?.mockReset === "function") fn.mockReset();
  }
  apiMock.hideWindow.mockResolvedValue(undefined);
  apiMock.minimizeWindow.mockResolvedValue(undefined);
  apiMock.toggleMaximizeWindow.mockResolvedValue(undefined);
  apiMock.startDraggingWindow.mockResolvedValue(undefined);
  apiMock.setLogicalWindowSize.mockResolvedValue(undefined);
  apiMock.setLogicalWindowPosition.mockResolvedValue(undefined);
  apiMock.centerWindow.mockResolvedValue(undefined);
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

  it("startDragging delegates to api.startDraggingWindow exactly once", async () => {
    await osWindowStore.startDragging();
    expect(apiMock.startDraggingWindow).toHaveBeenCalledTimes(1);
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

describe("osWindowStore restoreSize", () => {
  it("no-ops when config has no window entry", async () => {
    configMock.config.mockReturnValue({ window: null });
    await osWindowStore.restoreSize();
    expect(apiMock.getLogicalWindowSize).not.toHaveBeenCalled();
    expect(apiMock.setLogicalWindowSize).not.toHaveBeenCalled();
  });

  it("no-ops when config dimensions are non-positive", async () => {
    configMock.config.mockReturnValue({ window: { width: 0, height: 600 } });
    await osWindowStore.restoreSize();
    expect(apiMock.setLogicalWindowSize).not.toHaveBeenCalled();
  });

  it("no-ops when current size already matches config", async () => {
    configMock.config.mockReturnValue({ window: { width: 800, height: 600 } });
    apiMock.getLogicalWindowSize.mockResolvedValue({ width: 800, height: 600 });
    await osWindowStore.restoreSize();
    expect(apiMock.setLogicalWindowSize).not.toHaveBeenCalled();
  });

  it("calls setLogicalWindowSize when current differs from config", async () => {
    configMock.config.mockReturnValue({ window: { width: 800, height: 600 } });
    apiMock.getLogicalWindowSize.mockResolvedValue({ width: 1024, height: 768 });
    await osWindowStore.restoreSize();
    expect(apiMock.setLogicalWindowSize).toHaveBeenCalledWith(800, 600);
  });

  it("restores a clamped saved position via setLogicalWindowPosition", async () => {
    configMock.config.mockReturnValue({ window: { width: 800, height: 600, x: 200, y: 150 } });
    apiMock.getLogicalWindowSize.mockResolvedValue({ width: 800, height: 600 });
    apiMock.computeWindowPlacement.mockResolvedValue({ x: 200, y: 150 });
    apiMock.getLogicalWindowPosition.mockResolvedValue({ x: 0, y: 0 });

    await osWindowStore.restoreSize();

    expect(apiMock.computeWindowPlacement).toHaveBeenCalledWith(200, 150, 800, 600);
    expect(apiMock.setLogicalWindowPosition).toHaveBeenCalledWith(200, 150);
    expect(apiMock.centerWindow).not.toHaveBeenCalled();
  });

  it("centers when the saved position is off-screen (placement is null)", async () => {
    configMock.config.mockReturnValue({ window: { width: 800, height: 600, x: 9000, y: 9000 } });
    apiMock.getLogicalWindowSize.mockResolvedValue({ width: 800, height: 600 });
    apiMock.computeWindowPlacement.mockResolvedValue(null);

    await osWindowStore.restoreSize();

    expect(apiMock.setLogicalWindowPosition).not.toHaveBeenCalled();
    expect(apiMock.centerWindow).toHaveBeenCalledTimes(1);
  });

  it("skips reposition when the current position already matches the clamp", async () => {
    configMock.config.mockReturnValue({ window: { width: 800, height: 600, x: 200, y: 150 } });
    apiMock.getLogicalWindowSize.mockResolvedValue({ width: 800, height: 600 });
    apiMock.computeWindowPlacement.mockResolvedValue({ x: 200, y: 150 });
    apiMock.getLogicalWindowPosition.mockResolvedValue({ x: 200, y: 150 });

    await osWindowStore.restoreSize();

    expect(apiMock.setLogicalWindowPosition).not.toHaveBeenCalled();
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
