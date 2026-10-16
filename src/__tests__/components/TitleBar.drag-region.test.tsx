import { describe, it, expect, vi, beforeAll, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";

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
  titleBarDoubleClick: vi.fn(),
  startDragging: vi.fn(),
  minimize: vi.fn(),
  hide: vi.fn(),
  toggleFullscreen: vi.fn(),
}));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    editor: { isRemovedOnDisk: () => false },
    tabs: {
      activeTabId: mocks.activeTabId,
      setActiveTabId: mocks.setActiveTabId,
      closeTab: mocks.closeTab,
      closeOtherTabs: mocks.closeOtherTabs,
      closeAllTabs: mocks.closeAllTabs,
      createTab: mocks.createTab,
    },
    // No note is waiting on a sync provider in these cases.
    downloads: {
      pending: () => [],
      selectedPath: () => null,
      select: () => {},
      cancel: async () => {},
      close: () => {},
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
    maximized: () => false,
    snapHovered: () => false,
    snapPressed: () => false,
    installSnapOverlay: () => Promise.resolve(() => {}),
    toggleMaximize: mocks.toggleMaximize,
    titleBarDoubleClick: mocks.titleBarDoubleClick,
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
import { isInteractiveTarget } from "../../components/TitleBar/title-bar-region";

describe("TitleBar drag region", () => {
  afterEach(() => {
    mocks.toggleMaximize.mockClear();
    mocks.titleBarDoubleClick.mockClear();
    mocks.startDragging.mockClear();
    mocks.setActiveTabId.mockClear();
    cleanup();
  });

  it("carries no tab strip: it lives under the toolbar", () => {
    const { container } = render(() => <TitleBar />);
    expect(container.querySelector(".titlebar-tabs")).toBeNull();
    expect(container.querySelector(".tabbar")).toBeNull();
  });

  it("double-clicking the bare titlebar surface is answered once, after one drag", () => {
    const { container } = render(() => <TitleBar />);
    const bar = container.querySelector<HTMLElement>(".titlebar")!;

    for (const detail of [1, 2]) {
      fireEvent.mouseDown(bar, { button: 0, detail });
      fireEvent.mouseUp(bar, { button: 0, detail });
      fireEvent.click(bar, { button: 0, detail });
    }
    fireEvent.dblClick(bar, { button: 0, detail: 2 });

    expect(mocks.titleBarDoubleClick).toHaveBeenCalledTimes(1);
    expect(mocks.startDragging).toHaveBeenCalledTimes(1);
    expect(mocks.toggleMaximize).not.toHaveBeenCalled();
  });

  it("a double-click on the window chord does nothing to the window", () => {
    const { container } = render(() => <TitleBar />);
    const chord = container.querySelector<HTMLElement>(".titlebar-right")!;

    for (const detail of [1, 2]) {
      fireEvent.mouseDown(chord, { button: 0, detail });
      fireEvent.mouseUp(chord, { button: 0, detail });
    }
    fireEvent.dblClick(chord, { button: 0, detail: 2 });

    expect(mocks.titleBarDoubleClick).not.toHaveBeenCalled();
    expect(mocks.startDragging).not.toHaveBeenCalled();
  });
});

describe("isInteractiveTarget", () => {
  it("treats a detached node as interactive so the titlebar never drags or maximizes from it", () => {
    const detached = document.createElement("span");
    expect(detached.isConnected).toBe(false);
    expect(isInteractiveTarget(detached)).toBe(true);
  });

  it("treats a connected interactive ancestor (button) as interactive", () => {
    const button = document.createElement("button");
    const child = document.createElement("span");
    button.appendChild(child);
    document.body.appendChild(button);
    try {
      expect(isInteractiveTarget(child)).toBe(true);
    } finally {
      button.remove();
    }
  });

  it("treats an SVG glyph inside a button as interactive", () => {
    const button = document.createElement("button");
    const glyph = document.createElementNS("http://www.w3.org/2000/svg", "path");
    button.appendChild(glyph);
    document.body.appendChild(button);
    try {
      expect(glyph).not.toBeInstanceOf(HTMLElement);
      expect(isInteractiveTarget(glyph)).toBe(true);
    } finally {
      button.remove();
    }
  });

  it("treats a connected bare surface as non-interactive", () => {
    const bare = document.createElement("div");
    document.body.appendChild(bare);
    try {
      expect(isInteractiveTarget(bare)).toBe(false);
    } finally {
      bare.remove();
    }
  });

  it("treats a tab, a link and a focusable surface as interactive, as Tauri's drag script does", () => {
    const strip = document.createElement("div");
    strip.setAttribute("role", "tablist");
    const tab = document.createElement("div");
    tab.setAttribute("role", "tab");
    const link = document.createElement("a");
    const focusable = document.createElement("div");
    focusable.tabIndex = 0;
    strip.append(tab, link, focusable);
    document.body.appendChild(strip);
    try {
      expect(isInteractiveTarget(strip)).toBe(false);
      for (const el of [tab, link, focusable]) expect(isInteractiveTarget(el)).toBe(true);
    } finally {
      strip.remove();
    }
  });

  it("does not let a control above the region make the region itself a control", () => {
    const outer = document.createElement("div");
    outer.tabIndex = 0;
    const region = document.createElement("div");
    const bare = document.createElement("span");
    region.appendChild(bare);
    outer.appendChild(region);
    document.body.appendChild(outer);
    try {
      expect(isInteractiveTarget(bare)).toBe(true);
      expect(isInteractiveTarget(bare, region)).toBe(false);
    } finally {
      outer.remove();
    }
  });

  it("ignores non-element targets", () => {
    expect(isInteractiveTarget(null)).toBe(false);
    expect(isInteractiveTarget(new EventTarget())).toBe(false);
  });
});
