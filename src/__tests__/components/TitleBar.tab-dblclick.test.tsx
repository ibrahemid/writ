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
  toggleMaximize: vi.fn(),
  startDragging: vi.fn(),
  minimize: vi.fn(),
  hide: vi.fn(),
  toggleFullscreen: vi.fn(),
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
}));

vi.mock("../../stores/global/os-window", () => ({
  osWindowStore: {
    focused: () => true,
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

import TitleBar, { isInteractiveTarget } from "../../components/TitleBar/TitleBar";

describe("TitleBar tab double-click (#125)", () => {
  afterEach(() => {
    mocks.toggleMaximize.mockClear();
    mocks.setActiveTabId.mockClear();
    cleanup();
  });

  it("double-clicking a tab title enters rename, stops propagation, and does not maximize", () => {
    const { container } = render(() => <TitleBar />);
    const title = container.querySelector<HTMLElement>(".tab-title");
    expect(title).not.toBeNull();
    expect(title!.textContent).toBe("alpha.md");

    // Spy on stopPropagation to pin the primary fix: the tab's onDblClick must
    // halt the event before Solid's delegated walk reaches the titlebar handler.
    const ev = new MouseEvent("dblclick", { bubbles: true, cancelable: true });
    const stopProp = vi.spyOn(ev, "stopPropagation");
    title!.dispatchEvent(ev);

    expect(container.querySelector(".tab-rename-input")).not.toBeNull();
    expect(stopProp).toHaveBeenCalled();
    expect(mocks.toggleMaximize).not.toHaveBeenCalled();
  });

  it("double-clicking the bare titlebar surface still toggles maximize", () => {
    const { container } = render(() => <TitleBar />);
    const bar = container.querySelector<HTMLElement>(".titlebar")!;

    fireEvent.dblClick(bar);

    expect(mocks.toggleMaximize).toHaveBeenCalledTimes(1);
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

  it("treats a connected bare surface as non-interactive", () => {
    const bare = document.createElement("div");
    document.body.appendChild(bare);
    try {
      expect(isInteractiveTarget(bare)).toBe(false);
    } finally {
      bare.remove();
    }
  });

  it("ignores non-element targets", () => {
    expect(isInteractiveTarget(null)).toBe(false);
    expect(isInteractiveTarget(new EventTarget())).toBe(false);
  });
});
