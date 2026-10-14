import { describe, it, expect, vi, afterEach } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";

// The crash screen is the last thing a user sees, so what it shows has to be
// readable and worth sending on, and the window has to stay movable and
// closable with nothing else drawn in it.

const h = vi.hoisted(() => ({
  copyText: vi.fn(),
  showToast: vi.fn(),
  platform: "win" as "mac" | "win" | "linux",
  startDragging: vi.fn(),
}));

vi.mock("../../stores/global/clipboard", () => ({
  clipboardStore: { copyText: h.copyText },
}));

vi.mock("../../stores/global/notifications", () => ({
  showToast: h.showToast,
}));

vi.mock("../../lib/platform", () => ({
  resolvePlatform: () => h.platform,
  detectPlatform: () => h.platform,
  IS_MAC: false,
}));

vi.mock("../../stores/global/os-window", () => ({
  osWindowStore: {
    maximized: () => false,
    focused: () => true,
    startDragging: h.startDragging,
    minimize: vi.fn(),
    toggleMaximize: vi.fn(),
    close: vi.fn(),
    installSnapOverlay: vi.fn(),
    snapHovered: () => false,
    snapPressed: () => false,
    hide: vi.fn(),
    toggleFullscreen: vi.fn(),
  },
}));

import ErrorBoundary from "../../components/ErrorBoundary/ErrorBoundary";
import { APP_VERSION } from "../../lib/app-info";

const LONG = "E".repeat(400);

function Throws(): never {
  throw new Error(LONG);
}

afterEach(() => {
  h.copyText.mockReset();
  h.showToast.mockReset();
  h.startDragging.mockReset();
  h.platform = "win";
  vi.restoreAllMocks();
  cleanup();
});

describe("the crash screen", () => {
  it("hands the details to the clipboard with the version and platform", async () => {
    h.copyText.mockResolvedValue(undefined);
    const screen = render(() => (
      <ErrorBoundary>
        <Throws />
      </ErrorBoundary>
    ));

    fireEvent.click(screen.container.querySelector("[data-action='copy-crash-details']")!);

    await waitFor(() => expect(h.copyText).toHaveBeenCalledTimes(1));
    const copied: string = h.copyText.mock.calls[0][0];
    expect(copied).toContain(LONG);
    expect(copied).toContain(`Writ ${APP_VERSION}`);
    expect(copied).toContain("Platform: win");
  });

  it("says so when the clipboard refuses", async () => {
    h.copyText.mockRejectedValue(new Error("no"));
    const screen = render(() => (
      <ErrorBoundary>
        <Throws />
      </ErrorBoundary>
    ));

    fireEvent.click(screen.container.querySelector("[data-action='copy-crash-details']")!);

    await waitFor(() => expect(h.showToast).toHaveBeenCalled());
    expect(h.showToast).toHaveBeenCalledWith("Could not copy the details", "error");
  });

  it("logs the crash", () => {
    const log = vi.spyOn(console, "error").mockImplementation(() => {});
    render(() => (
      <ErrorBoundary>
        <Throws />
      </ErrorBoundary>
    ));
    expect(log.mock.calls.some(([line]) => String(line).startsWith("[writ] "))).toBe(true);
  });

  it("draws the caption buttons on Windows and drags the window from its bar", () => {
    const screen = render(() => (
      <ErrorBoundary>
        <Throws />
      </ErrorBoundary>
    ));
    expect(screen.container.querySelector(".titlebar-controls-win")).not.toBeNull();
    const bar = screen.container.querySelector(".failure-screen-bar")!;
    fireEvent.mouseDown(bar, { button: 0 });
    expect(h.startDragging).toHaveBeenCalledTimes(1);
  });

  it("draws the lights on macOS", () => {
    h.platform = "mac";
    const screen = render(() => (
      <ErrorBoundary>
        <Throws />
      </ErrorBoundary>
    ));
    expect(screen.container.querySelector(".window-lights-layer")).not.toBeNull();
  });
});

describe("the crash screen's own sheet", () => {
  const css = readFileSync(
    resolve(process.cwd(), "src/components/FailureScreen/FailureScreen.css"),
    "utf8",
  );

  function rule(selector: string): string {
    const match = css.match(new RegExp(`^\\${selector}\\s*\\{([^}]*)\\}`, "m"));
    expect(match, `${selector} is declared`).toBeTruthy();
    return match![1];
  }

  it("paints its own surface on a transparent window", () => {
    expect(rule(".failure-screen")).toMatch(/background:\s*var\(--writ-bg-canvas\)/);
  });

  it("wraps a single-token error rather than scrolling it sideways", () => {
    const body = rule(".failure-screen-details");
    expect(body).toMatch(/white-space:\s*pre-wrap/);
    expect(body).toMatch(/overflow-wrap:\s*anywhere/);
    expect(body).toMatch(/max-width:\s*\d+ch/);
  });

  it("leaves the destructive red to a destructive action", () => {
    expect(rule(".failure-screen-title")).not.toContain("--writ-danger");
  });
});
