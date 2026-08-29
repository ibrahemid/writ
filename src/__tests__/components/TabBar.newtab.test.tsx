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
  createTab: vi.fn(),
  setActiveTabId: vi.fn(),
  closeTab: vi.fn(),
  closeOtherTabs: vi.fn(),
  closeAllTabs: vi.fn(),
  renameBuffer: vi.fn(() => Promise.resolve()),
  focusEditor: vi.fn(),
  activeTabId: vi.fn(() => "buf-1"),
  // The add button rides the strip, and the strip needs two notes to appear.
  activeTabs: vi.fn(() => [
    { id: "buf-1", title: "alpha.md", filename: "alpha.md", source_path: null },
    { id: "buf-2", title: "beta.md", filename: "beta.md", source_path: null },
  ]),
  showContextMenu: vi.fn(),
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
    editor: { focusEditor: mocks.focusEditor },
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
      tabs: { activeTabId: mocks.activeTabId },
    }),
  },
}));

vi.mock("../../components/ContextMenu/ContextMenu", () => ({
  showContextMenu: mocks.showContextMenu,
}));

import TabBar from "../../components/Editor/TabBar";

describe("TabBar new-tab button (#46)", () => {
  afterEach(() => {
    mocks.createTab.mockClear();
    cleanup();
  });

  it("exposes accessible name 'New tab'", () => {
    const { container } = render(() => <TabBar />);
    const newTab = container.querySelector<HTMLButtonElement>(".tab-add");
    expect(newTab).not.toBeNull();
    expect(newTab!.getAttribute("aria-label")).toBe("New tab");
    expect(newTab!.getAttribute("type")).toBe("button");
  });

  it("clicking invokes createTab", () => {
    const { container } = render(() => <TabBar />);
    const newTab = container.querySelector<HTMLButtonElement>(".tab-add")!;
    fireEvent.click(newTab);
    expect(mocks.createTab).toHaveBeenCalledTimes(1);
  });
});
