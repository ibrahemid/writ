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
}));

vi.mock("../../lib/platform", () => ({
  detectPlatform: () => mocks.platform.current,
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
    toggleMaximize: mocks.toggleMaximize,
    startDragging: mocks.startDragging,
    minimize: mocks.minimize,
    hide: mocks.hide,
    toggleFullscreen: mocks.toggleFullscreen,
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
  { id: "file.open", label: "Open File", keybinding: "CmdOrCtrl+O" },
  { id: "buffer.new", label: "New Tab", keybinding: "CmdOrCtrl+T" },
  { id: "buffer.close", label: "Close Tab", keybinding: "CmdOrCtrl+W" },
  { id: "palette.open", label: "Command Palette", keybinding: "Shift+Shift" },
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
  vi.clearAllMocks();
  cleanup();
});

describe("titlebar menu affordance is Windows-only", () => {
  it("renders the Writ menu button on Windows", () => {
    const { container } = renderOn("win");
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

  it("omits the menu button on Linux", () => {
    const { container } = renderOn("linux");
    expect(container.querySelector(".titlebar-appmenu")).toBeNull();
  });

  it("leaves the macOS titlebar on its traffic-light branch with no window controls added", () => {
    const { container } = renderOn("mac");
    expect(container.querySelector(".titlebar-controls-mac")).not.toBeNull();
    expect(container.querySelector(".titlebar-controls-win")).toBeNull();
    expect(container.querySelector(".winctrl")).toBeNull();
    const labels = Array.from(container.querySelectorAll(".maclight")).map((el) =>
      el.getAttribute("aria-label"),
    );
    expect(labels).toEqual(["Hide window", "Minimize window", "Toggle full screen"]);
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

describe("Writ menu contents", () => {
  it("lists the macOS menu actions plus the palette, in order", () => {
    const { container } = renderOn("win");
    fireEvent.click(container.querySelector(".titlebar-appmenu")!);

    expect(mocks.showAnchoredMenu).toHaveBeenCalledTimes(1);
    expect(openedMenuItems().map((item) => item.label)).toEqual([
      "Open File",
      "New Tab",
      "Close Tab",
      "Command Palette",
      "Check for Updates",
    ]);
  });

  it("takes labels and shortcuts from the command registry, not a second table", () => {
    const { container } = renderOn("win");
    fireEvent.click(container.querySelector(".titlebar-appmenu")!);

    const items = openedMenuItems();
    expect(items[0].kbd).toBe("Ctrl+O");
    expect(items[2].kbd).toBe("Ctrl+W");
    expect(items[4].kbd).toBeUndefined();
  });

  it("dispatches each entry through the command registry", () => {
    const { container } = renderOn("win");
    fireEvent.click(container.querySelector(".titlebar-appmenu")!);

    for (const item of openedMenuItems()) item.action();

    expect(executed).toEqual([
      "file.open",
      "buffer.new",
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
      label: "Close Tab",
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
