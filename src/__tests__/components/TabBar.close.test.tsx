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
  // The strip is hidden at one note, so the close control needs two.
  activeTabs: vi.fn(() => [
    { id: "buf-1", title: "alpha.md", filename: "alpha.md", source_path: null },
    { id: "buf-2", title: "beta.md", filename: "beta.md", source_path: null },
  ]),
  activeTabId: vi.fn(() => "buf-1"),
  setActiveTabId: vi.fn(),
  closeTab: vi.fn(),
  closeOtherTabs: vi.fn(),
  closeAllTabs: vi.fn(),
  createTab: vi.fn(),
  renameBuffer: vi.fn(() => Promise.resolve()),
  showContextMenu: vi.fn(),
  focusEditor: vi.fn(),
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
    editor: { focusEditor: mocks.focusEditor, isRemovedOnDisk: () => false },
    // No note is waiting on a sync provider in these cases.
    downloads: {
      pending: () => [],
      selectedPath: () => null,
      select: () => {},
      dismiss: async () => {},
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
    getActive: () => ({
      tabs: {
        activeTabId: mocks.activeTabId,
        closeTab: mocks.closeTab,
      },
    }),
  },
}));

vi.mock("../../components/ContextMenu/ContextMenu", () => ({
  showContextMenu: mocks.showContextMenu,
}));

import TabBar from "../../components/Editor/TabBar";
import { registerCommand, unregisterCommand } from "../../commands/registry";
import {
  installKeyboardHandler,
  rebuildKeyMap,
  uninstallKeyboardHandler,
} from "../../commands/keybindings";
import { windowRegistry } from "../../stores/global/window-registry";

describe("TabBar close button (#47)", () => {
  afterEach(() => {
    mocks.closeTab.mockClear();
    mocks.setActiveTabId.mockClear();
    cleanup();
  });

  it("is a button of its own, named for the note it closes", () => {
    const { container } = render(() => <TabBar />);
    const close = container.querySelector<HTMLButtonElement>(".tab-close");
    expect(close).not.toBeNull();
    expect(close!.tagName).toBe("BUTTON");
    expect(close!.getAttribute("type")).toBe("button");
    expect(close!.tabIndex).toBe(0);
    expect(close!.getAttribute("aria-label")).toBe("Close alpha.md");
  });

  it("clicking close invokes closeTab and does not re-select the tab", () => {
    const { container } = render(() => <TabBar />);
    const close = container.querySelector<HTMLElement>(".tab-close")!;
    fireEvent.click(close);
    expect(mocks.closeTab).toHaveBeenCalledWith("buf-1");
    expect(mocks.setActiveTabId).not.toHaveBeenCalled();
  });

  it("Enter and Space on the close element invoke closeTab", () => {
    const { container } = render(() => <TabBar />);
    const close = container.querySelector<HTMLElement>(".tab-close")!;
    fireEvent.keyDown(close, { key: "Enter" });
    fireEvent.keyDown(close, { key: " " });
    expect(mocks.closeTab).toHaveBeenCalledTimes(2);
    expect(mocks.closeTab).toHaveBeenNthCalledWith(1, "buf-1");
    expect(mocks.closeTab).toHaveBeenNthCalledWith(2, "buf-1");
  });

  it("lets Cmd+W through to the close-tab command while the close control holds focus", () => {
    registerCommand({
      id: "buffer.close",
      label: "Close tab",
      keybinding: "CmdOrCtrl+W",
      scope: "app",
      global: true,
      execute: () => {
        const w = windowRegistry.getActive();
        const id = w?.tabs.activeTabId();
        if (w && id) void w.tabs.closeTab(id);
      },
    });
    rebuildKeyMap();
    installKeyboardHandler();
    try {
      const { container } = render(() => <TabBar />);
      const close = container.querySelector<HTMLButtonElement>(".tab-close")!;
      close.focus();
      expect(document.activeElement).toBe(close);
      fireEvent.keyDown(close, { key: "w", metaKey: true });
      expect(mocks.closeTab).toHaveBeenCalledWith("buf-1");
    } finally {
      uninstallKeyboardHandler();
      unregisterCommand("buffer.close");
      rebuildKeyMap();
    }
  });
});
