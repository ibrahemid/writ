import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import type { Platform } from "../../lib/platform";

beforeAll(() => {
  Object.defineProperty(window, "matchMedia", {
    writable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  });
  if (!Element.prototype.scrollIntoView) {
    Element.prototype.scrollIntoView = vi.fn();
  }
});

const mocks = vi.hoisted(() => ({
  platform: { current: "win" as Platform },
  maximized: { current: false },
  activeTabs: vi.fn(() => [
    { id: "buf-1", title: "alpha.md", filename: "alpha.md", source_path: null },
  ]),
  activeTabId: vi.fn(() => "buf-1"),
  setActiveTabId: vi.fn(),
  closeTab: vi.fn(),
  closeOtherTabs: vi.fn(),
  closeAllTabs: vi.fn(),
  createTab: vi.fn(),
  renameBuffer: vi.fn(),
  showContextMenu: vi.fn(),
  showAnchoredMenu: vi.fn(),
  toggleMaximize: vi.fn(),
  startDragging: vi.fn(),
  minimize: vi.fn(),
  hide: vi.fn(),
  toggleFullscreen: vi.fn(),
  installSnapOverlay: vi.fn(),
  snapHovered: { current: false },
  snapPressed: { current: false },
}));

vi.mock("../../lib/platform", () => ({
  detectPlatform: () => mocks.platform.current,
  resolvePlatform: () => mocks.platform.current,
  IS_MAC: false,
}));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    tabs: {
      activeTabId: mocks.activeTabId,
      setActiveTabId: mocks.setActiveTabId,
      closeTab: mocks.closeTab,
      closeOtherTabs: mocks.closeOtherTabs,
      closeAllTabs: mocks.closeAllTabs,
      createTab: mocks.createTab,
    },
  }),
}));

vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    activeTabs: mocks.activeTabs,
    renameBuffer: mocks.renameBuffer,
  },
}));

vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: {
    getActive: () => ({ tabs: { activeTabId: mocks.activeTabId } }),
  },
}));

vi.mock("../../components/ContextMenu/ContextMenu", () => ({
  showContextMenu: mocks.showContextMenu,
  showAnchoredMenu: mocks.showAnchoredMenu,
}));

vi.mock("../../stores/global/os-window", () => ({
  osWindowStore: {
    focused: () => true,
    maximized: () => mocks.maximized.current,
    snapHovered: () => mocks.snapHovered.current,
    snapPressed: () => mocks.snapPressed.current,
    toggleMaximize: mocks.toggleMaximize,
    startDragging: mocks.startDragging,
    minimize: mocks.minimize,
    hide: mocks.hide,
    toggleFullscreen: mocks.toggleFullscreen,
    installSnapOverlay: mocks.installSnapOverlay,
  },
}));

vi.mock("../../stores/global/config", () => ({
  configStore: {
    config: () => ({ hotkey: { toggle: "CmdOrCtrl+Shift+Space" } }),
  },
}));

import TitleBar from "../../components/TitleBar/TitleBar";
import { registerCommand, unregisterCommand } from "../../commands/registry";
import type { MenuItem } from "../../components/ContextMenu/ContextMenu";

const MENU_COMMANDS = [
  { id: "note.new", label: "New note", keybinding: "CmdOrCtrl+N" },
  { id: "file.open", label: "Open file", keybinding: "CmdOrCtrl+O" },
  { id: "note.rename", label: "Rename note…", keybinding: "F2" },
  { id: "note.saveCopy", label: "Save a copy…" },
  { id: "buffer.close", label: "Close tab", keybinding: "CmdOrCtrl+W" },
  { id: "palette.open", label: "Command palette", keybinding: "Shift+Shift" },
  { id: "app.check_updates", label: "Check for Updates" },
];

const executed: string[] = [];

function renderOn(platform: Platform) {
  mocks.platform.current = platform;
  return render(() => <TitleBar />);
}

function openedMenuItems(): MenuItem[] {
  return mocks.showAnchoredMenu.mock.calls[0][1] as MenuItem[];
}

beforeEach(() => {
  executed.length = 0;
  mocks.installSnapOverlay.mockResolvedValue(() => {});
  for (const cmd of MENU_COMMANDS) {
    registerCommand({
      ...cmd,
      scope: "app",
      execute: () => {
        executed.push(cmd.id);
      },
    });
  }
});

afterEach(() => {
  for (const cmd of MENU_COMMANDS) unregisterCommand(cmd.id);
  mocks.platform.current = "win";
  mocks.maximized.current = false;
  mocks.snapHovered.current = false;
  mocks.snapPressed.current = false;
  vi.clearAllMocks();
  cleanup();
});

describe("titlebar menu affordance carries the platforms with no menu bar", () => {
  it.each(["win", "linux"] as const)("renders the Writ menu button on %s", (platform) => {
    const { container } = renderOn(platform);
    const button = container.querySelector<HTMLButtonElement>(".titlebar-appmenu");
    expect(button).not.toBeNull();
    expect(button!.tagName).toBe("BUTTON");
    expect(button!.getAttribute("aria-label")).toBe("Writ menu");
    expect(button!.getAttribute("aria-haspopup")).toBe("menu");
  });

  it("omits the menu button on macOS, which has the native menu bar", () => {
    const { container } = renderOn("mac");
    expect(container.querySelector(".titlebar-appmenu")).toBeNull();
  });

  it("opens the same menu from the Linux titlebar", () => {
    const { container } = renderOn("linux");
    fireEvent.click(container.querySelector(".titlebar-appmenu")!);

    expect(mocks.showAnchoredMenu).toHaveBeenCalledTimes(1);
    expect(openedMenuItems().map((item) => item.label)).toEqual([
      "New note",
      "Open file",
      "Rename note…",
      "Save a copy…",
      "Close tab",
      "Command palette",
      "Check for Updates",
    ]);
  });

  // GNOME's button-layout is 'appmenu:close': one control, not three.
  it("leaves Linux a single close control in its header bar", () => {
    const { container } = renderOn("linux");
    expect(container.querySelector(".titlebar-linux")).not.toBeNull();
    expect(container.querySelector(".headerbar")).not.toBeNull();
    expect(container.querySelectorAll(".gnomectrl")).toHaveLength(1);
    expect(container.querySelector(".gnomectrl-close")!.getAttribute("aria-label")).toBe(
      "Hide window",
    );
    expect(container.querySelector(".winctrl")).toBeNull();
  });

  it("centres the window title in the GNOME header bar", () => {
    const { container } = renderOn("linux");
    expect(container.querySelector(".headerbar-title")!.textContent).toBe("Writ");
  });

  it("moves New note into the GNOME header bar, ahead of the menu", () => {
    const { container } = renderOn("linux");
    const compose = container.querySelector(".headerbar-compose");
    expect(compose).not.toBeNull();
    expect(compose!.textContent).toContain("New note");
    expect(
      compose!.compareDocumentPosition(container.querySelector(".titlebar-appmenu")!) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("keeps New note out of the chrome on the shells with a toolbar", () => {
    expect(renderOn("win").container.querySelector(".headerbar-compose")).toBeNull();
    cleanup();
    expect(renderOn("mac").container.querySelector(".headerbar-compose")).toBeNull();
  });

  it("renders no title bar at all on macOS: the toolbar is the top row", () => {
    const { container } = renderOn("mac");
    expect(container.querySelector(".titlebar")).toBeNull();
    expect(container.querySelector(".winctrl")).toBeNull();
    expect(container.querySelector(".gnomectrl")).toBeNull();
    expect(container.querySelector(".maclight")).toBeNull();
  });
});

describe("Windows window controls", () => {
  it("orders the controls minimize, maximize, close", () => {
    const { container } = renderOn("win");
    const classes = Array.from(container.querySelectorAll(".winctrl")).map(
      (el) => el.className.split(" ")[1],
    );
    expect(classes).toEqual(["winctrl-min", "winctrl-max", "winctrl-close"]);
  });

  it("routes minimize to the store", () => {
    const { container } = renderOn("win");
    fireEvent.click(container.querySelector(".winctrl-min")!);
    expect(mocks.minimize).toHaveBeenCalledTimes(1);
  });

  it("routes maximize to the store", () => {
    const { container } = renderOn("win");
    fireEvent.click(container.querySelector(".winctrl-max")!);
    expect(mocks.toggleMaximize).toHaveBeenCalledTimes(1);
  });

  it("routes close to the store's hide path rather than destroying the window", () => {
    const { container } = renderOn("win");
    fireEvent.click(container.querySelector(".winctrl-close")!);
    expect(mocks.hide).toHaveBeenCalledTimes(1);
  });

  it("names the close button for what it does, which is hide", () => {
    const { container } = renderOn("win");
    const button = container.querySelector<HTMLButtonElement>(".winctrl-close")!;
    expect(button.getAttribute("title")).toBe("Hide");
    expect(button.getAttribute("aria-label")).toBe("Hide window");
  });

  it("clicks through a press on the glyph instead of dragging the window", () => {
    const { container } = renderOn("win");
    // The glyph is an SVG element, not an HTMLElement: if the titlebar's
    // interactive-target check misses it, pressing the middle of a caption
    // button starts a window drag and the button never fires.
    const glyph = container.querySelector(".winctrl-min svg")!;
    fireEvent.mouseDown(glyph, { button: 0 });
    expect(mocks.startDragging).not.toHaveBeenCalled();
  });
});

describe("maximize button reflects window state", () => {
  it("offers Maximize while the window is restored", () => {
    mocks.maximized.current = false;
    const { container } = renderOn("win");
    const button = container.querySelector<HTMLButtonElement>(".winctrl-max")!;
    expect(button.getAttribute("aria-label")).toBe("Maximize window");
    expect(button.getAttribute("title")).toBe("Maximize");
    expect(button.querySelectorAll("svg rect")).toHaveLength(1);
  });

  it("offers Restore while the window is maximized", () => {
    mocks.maximized.current = true;
    const { container } = renderOn("win");
    const button = container.querySelector<HTMLButtonElement>(".winctrl-max")!;
    expect(button.getAttribute("aria-label")).toBe("Restore window");
    expect(button.getAttribute("title")).toBe("Restore");
    expect(button.querySelector("svg path")).not.toBeNull();
  });
});

describe("snap-layout overlay geometry", () => {
  // jsdom has no ResizeObserver and zeroes every rect, which is also the state
  // the real button is in while the window is still hidden. Driving both by
  // hand is what lets the hidden-window case be tested at all.
  let observers: StubResizeObserver[] = [];
  let measured: DOMRect | null = null;

  class StubResizeObserver {
    disconnected = false;
    constructor(private callback: () => void) {
      observers.push(this);
    }
    observe(): void {}
    disconnect(): void {
      this.disconnected = true;
    }
    // A disconnected observer never calls back, so the stub must not either.
    layout(rect: DOMRect | null): void {
      if (this.disconnected) return;
      measured = rect;
      this.callback();
    }
  }

  beforeEach(() => {
    observers = [];
    measured = null;
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 1200 });
    vi.stubGlobal("ResizeObserver", StubResizeObserver);
    const original = Element.prototype.getBoundingClientRect;
    Element.prototype.getBoundingClientRect = function (this: Element): DOMRect {
      if (!this.classList.contains("winctrl-max") || !measured) return original.call(this);
      return measured;
    };
    return () => {
      Element.prototype.getBoundingClientRect = original;
      vi.unstubAllGlobals();
    };
  });

  const REAL_LAYOUT = { top: 0, right: 1154, width: 46, height: 36 } as DOMRect;

  it("reports the button as a distance from the window's right edge", () => {
    renderOn("win");
    observers[0].layout(REAL_LAYOUT);

    expect(mocks.installSnapOverlay).toHaveBeenCalledTimes(1);
    expect(mocks.installSnapOverlay.mock.calls[0][0]).toEqual({
      offsetFromRight: 46,
      top: 0,
      width: 46,
      height: 36,
    });
  });

  // The window is created hidden and shown after first paint. A zero-sized
  // report is rejected by Rust and never retried, which would leave the app
  // with no snap layouts for the rest of the session.
  it("reports nothing while the button still measures zero", () => {
    renderOn("win");
    observers[0].layout({ top: 0, right: 0, width: 0, height: 0 } as DOMRect);

    expect(mocks.installSnapOverlay).not.toHaveBeenCalled();
    expect(observers[0].disconnected).toBe(false);
  });

  it("reports once the button gets a real layout, and only once", () => {
    renderOn("win");
    observers[0].layout({ top: 0, right: 0, width: 0, height: 0 } as DOMRect);
    observers[0].layout(REAL_LAYOUT);
    observers[0].layout({ ...REAL_LAYOUT, right: 900 } as DOMRect);

    expect(mocks.installSnapOverlay).toHaveBeenCalledTimes(1);
    expect(mocks.installSnapOverlay.mock.calls[0][0].offsetFromRight).toBe(46);
    expect(observers[0].disconnected).toBe(true);
  });

  it("stops observing when the titlebar unmounts before any real layout", () => {
    renderOn("win");
    cleanup();

    expect(observers[0].disconnected).toBe(true);
    observers[0].layout(REAL_LAYOUT);
    expect(mocks.installSnapOverlay).not.toHaveBeenCalled();
  });

  it.each(["mac", "linux"] as const)("reports nothing on %s, which has no snap layouts", (platform) => {
    renderOn(platform);
    expect(observers).toHaveLength(0);
    expect(mocks.installSnapOverlay).not.toHaveBeenCalled();
  });

  it("lights the maximize button from the overlay's hover, which CSS cannot see", () => {
    mocks.snapHovered.current = true;
    const { container } = renderOn("win");
    const button = container.querySelector<HTMLButtonElement>(".winctrl-max")!;
    expect(button.classList.contains("is-snap-hovered")).toBe(true);
    expect(button.classList.contains("is-snap-pressed")).toBe(false);
  });

  it("presses the maximize button from the overlay's press", () => {
    mocks.snapPressed.current = true;
    const { container } = renderOn("win");
    const button = container.querySelector<HTMLButtonElement>(".winctrl-max")!;
    expect(button.classList.contains("is-snap-pressed")).toBe(true);
  });

  it("leaves the other caption buttons on their own CSS state", () => {
    mocks.snapHovered.current = true;
    const { container } = renderOn("win");
    for (const cls of [".winctrl-min", ".winctrl-close"]) {
      expect(container.querySelector(cls)!.classList.contains("is-snap-hovered")).toBe(false);
    }
  });
});

describe("Writ menu contents", () => {
  it("lists the macOS menu actions plus the palette, in order", () => {
    const { container } = renderOn("win");
    fireEvent.click(container.querySelector(".titlebar-appmenu")!);

    expect(mocks.showAnchoredMenu).toHaveBeenCalledTimes(1);
    expect(openedMenuItems().map((item) => item.label)).toEqual([
      "New note",
      "Open file",
      "Rename note…",
      "Save a copy…",
      "Close tab",
      "Command palette",
      "Check for Updates",
    ]);
  });

  it("takes labels and shortcuts from the command registry, not a second table", () => {
    const { container } = renderOn("win");
    fireEvent.click(container.querySelector(".titlebar-appmenu")!);

    const items = openedMenuItems();
    expect(items[0].kbd).toBe("Ctrl+N");
    expect(items[1].kbd).toBe("Ctrl+O");
    expect(items[4].kbd).toBe("Ctrl+W");
    expect(items[6].kbd).toBeUndefined();
  });

  it("dispatches each entry through the command registry", () => {
    const { container } = renderOn("win");
    fireEvent.click(container.querySelector(".titlebar-appmenu")!);

    for (const item of openedMenuItems()) item.action();

    expect(executed).toEqual([
      "note.new",
      "file.open",
      "note.rename",
      "note.saveCopy",
      "buffer.close",
      "palette.open",
      "app.check_updates",
    ]);
  });

  it("anchors the menu to the button and hands it back as the focus trigger", () => {
    const { container } = renderOn("win");
    const button = container.querySelector<HTMLButtonElement>(".titlebar-appmenu")!;
    // jsdom hands back a zeroed rect for everything, so pin identity by object,
    // not by coordinates: the anchor must be this button's own measurement.
    const rect = { top: 0, bottom: 36, left: 8, right: 60, width: 52, height: 36 } as DOMRect;
    button.getBoundingClientRect = () => rect;
    fireEvent.click(button);

    const [anchor, , trigger] = mocks.showAnchoredMenu.mock.calls[0];
    expect(anchor).toBe(rect);
    expect(trigger).toBe(button);
  });

  it("drops entries whose command is not registered instead of listing dead rows", () => {
    unregisterCommand("app.check_updates");
    const { container } = renderOn("win");
    fireEvent.click(container.querySelector(".titlebar-appmenu")!);

    expect(openedMenuItems().map((item) => item.label)).not.toContain("Check for Updates");
  });

  it("never opens a group divider on the first entry", () => {
    for (const cmd of MENU_COMMANDS) unregisterCommand(cmd.id);
    registerCommand({
      id: "buffer.close",
      label: "Close tab",
      scope: "app",
      execute: () => {},
    });
    const { container } = renderOn("win");
    fireEvent.click(container.querySelector(".titlebar-appmenu")!);

    const items = openedMenuItems();
    expect(items).toHaveLength(1);
    expect(items[0].separator).toBe(false);
  });
});
