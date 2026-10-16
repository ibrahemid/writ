import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import type { JSX } from "solid-js";
import type { Platform } from "../../lib/platform";

const h = vi.hoisted(() => ({
  platform: "mac" as Platform,
  sidebarOpen: true,
  startDragging: vi.fn(),
  titleBarDoubleClick: vi.fn(),
}));

vi.mock("../../lib/platform", () => ({
  resolvePlatform: () => h.platform,
  detectPlatform: () => h.platform,
  IS_MAC: true,
}));
vi.mock("../../stores/global/os-window", () => ({
  osWindowStore: {
    focused: () => true,
    hide: vi.fn(),
    minimize: vi.fn(),
    toggleFullscreen: vi.fn(),
    startDragging: h.startDragging,
    titleBarDoubleClick: h.titleBarDoubleClick,
  },
}));
vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    sidebar: {
      isOpen: () => h.sidebarOpen,
      searchQuery: () => "",
      setSearchQuery: vi.fn(),
      searchHits: () => [],
      searchTotal: () => 0,
      searchStatus: () => "idle",
      folderHits: () => [],
      folderOutcome: () => null,
      folderStatus: () => "idle",
      searchedFolder: () => null,
      activeResult: () => -1,
      setActiveResult: () => undefined,
      retrySearch: () => undefined,
    },
    tabs: {
      activeTabId: () => null,
      setActiveTabId: vi.fn(),
      closeTab: vi.fn(),
      restoreFromHistory: vi.fn(),
    },
    rightPanel: { isOpen: () => false },
  }),
}));
vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: { activeTabs: () => [], historyList: () => [], historyTotal: () => 0 },
}));
vi.mock("../../stores/global/workspace", () => ({ workspaceStore: { root: () => null } }));
vi.mock("../../stores/global/inbox", () => ({ inboxStore: { files: () => [] } }));
vi.mock("../../components/Sidebar/FilesSection", () => ({ default: () => null }));
vi.mock("../../components/Sidebar/InboxSection", () => ({ default: () => null }));
vi.mock("../../components/Sidebar/HistorySection", () => ({ default: () => null }));
vi.mock("../../components/Sidebar/SearchResults", () => ({ default: () => null }));
vi.mock("../../components/Sidebar/SidebarEmpty", () => ({ default: () => null }));

import Sidebar from "../../components/Sidebar/Sidebar";
import Toolbar from "../../components/Toolbar/Toolbar";
import WindowLights from "../../components/TitleBar/WindowLights";

const ATTR = "data-tauri-drag-region";

type Point = { x: number; y: number };
const AT: Point = { x: 20, y: 12 };

function press(el: Element, detail: number, down: Point = AT, up: Point = down) {
  fireEvent.mouseDown(el, { button: 0, detail, clientX: down.x, clientY: down.y });
  fireEvent.mouseUp(el, { button: 0, detail, clientX: up.x, clientY: up.y });
  fireEvent.click(el, { button: 0, detail, clientX: up.x, clientY: up.y });
}

// The sequence a browser delivers for one double-click: two presses, the
// second counted 2, then the dblclick at the second release.
function doubleClick(el: Element, release: Point = AT) {
  press(el, 1);
  press(el, 2, AT, release);
  fireEvent.dblClick(el, { button: 0, detail: 2, clientX: release.x, clientY: release.y });
}

function pick(container: HTMLElement, selector: string): HTMLElement {
  const el = container.querySelector<HTMLElement>(selector);
  expect(el, selector).not.toBeNull();
  return el!;
}

afterEach(() => {
  cleanup();
  h.platform = "mac";
  h.sidebarOpen = true;
  vi.clearAllMocks();
});

const BARE_CHROME: Array<[string, () => JSX.Element, string]> = [
  ["the toolbar's own surface", () => <Toolbar />, ".writ-toolbar"],
  ["the gap the toolbar divider sits in", () => <Toolbar />, ".writ-toolbar-divider"],
  ["the sidebar head", () => <Sidebar />, ".sidebar-head"],
  ["the lights layer", () => <WindowLights />, ".window-lights-layer"],
  ["the gaps in the lights row", () => <WindowLights />, ".window-lights-layer > .window-lights"],
];

describe("the macOS top chrome acts as the window's title bar", () => {
  // Tauri's drag script answers any region carrying the attribute. Its
  // double-click never maximizes an undecorated macOS window, and next to
  // Writ's own handler it would answer the same gesture a second time.
  it("leaves the gesture to Writ alone: no Tauri drag region remains", () => {
    h.sidebarOpen = false;
    for (const view of [() => <Toolbar />, () => <Sidebar />, () => <WindowLights />]) {
      const { container } = render(view);
      expect(container.querySelector(`[${ATTR}]`)).toBeNull();
      cleanup();
    }
  });

  it.each(BARE_CHROME)("a press on %s moves the window", (_name, view, selector) => {
    h.sidebarOpen = false;
    const { container } = render(view);
    press(pick(container, selector), 1);
    expect(h.startDragging).toHaveBeenCalledTimes(1);
    expect(h.titleBarDoubleClick).not.toHaveBeenCalled();
  });

  it.each(BARE_CHROME)("a double-click on %s is answered once", (_name, view, selector) => {
    h.sidebarOpen = false;
    const { container } = render(view);
    doubleClick(pick(container, selector));
    expect(h.titleBarDoubleClick).toHaveBeenCalledTimes(1);
    // Only the first press drags: a drag on the second would take its release.
    expect(h.startDragging).toHaveBeenCalledTimes(1);
  });

  it("a double-click whose second press travels is not one", () => {
    const { container } = render(() => <Toolbar />);
    doubleClick(pick(container, ".writ-toolbar"), { x: AT.x + 12, y: AT.y });
    expect(h.titleBarDoubleClick).not.toHaveBeenCalled();
  });

  it("a dblclick without a second press on the chrome is not one", () => {
    const { container } = render(() => <Toolbar />);
    fireEvent.dblClick(pick(container, ".writ-toolbar"), { button: 0, detail: 2 });
    expect(h.titleBarDoubleClick).not.toHaveBeenCalled();
  });

  it("a right-button press neither drags nor counts", () => {
    const { container } = render(() => <Toolbar />);
    fireEvent.mouseDown(pick(container, ".writ-toolbar"), { button: 2, detail: 1 });
    expect(h.startDragging).not.toHaveBeenCalled();
  });

  it("leaves the toolbar controls clickable: no drag, no double-click", () => {
    h.sidebarOpen = false;
    const { container } = render(() => <Toolbar />);
    const controls = container.querySelectorAll<HTMLElement>("button, input");
    expect(controls.length).toBeGreaterThan(0);
    for (const control of controls) {
      doubleClick(control);
    }
    expect(h.startDragging).not.toHaveBeenCalled();
    expect(h.titleBarDoubleClick).not.toHaveBeenCalled();
  });

  it("leaves the lights clickable: no drag, no double-click", () => {
    const { container } = render(() => <WindowLights />);
    const lights = container.querySelectorAll<HTMLElement>(".maclight, .maclight svg, .maclight path");
    expect(lights.length).toBeGreaterThanOrEqual(3);
    for (const light of lights) doubleClick(light);
    expect(h.startDragging).not.toHaveBeenCalled();
    expect(h.titleBarDoubleClick).not.toHaveBeenCalled();
  });

  // The field's icon and its own padding are the interior of a text control,
  // not window chrome.
  it("stops at the search field rather than acting from inside it", () => {
    const { container } = render(() => <Toolbar />);
    for (const selector of [".search-bar", ".search-field", ".search-icon", ".search-input"]) {
      doubleClick(pick(container, selector));
    }
    expect(h.startDragging).not.toHaveBeenCalled();
    expect(h.titleBarDoubleClick).not.toHaveBeenCalled();
  });

  it.each(["win", "linux"] as const)("leaves the %s toolbar to its own title bar", (p) => {
    h.platform = p;
    const { container } = render(() => <Toolbar />);
    doubleClick(pick(container, ".writ-toolbar"));
    expect(h.startDragging).not.toHaveBeenCalled();
    expect(h.titleBarDoubleClick).not.toHaveBeenCalled();
  });
});
