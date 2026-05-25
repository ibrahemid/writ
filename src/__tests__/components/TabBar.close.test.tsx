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
  renameTab: vi.fn(),
  createTab: vi.fn(),
  showContextMenu: vi.fn(),
}));

vi.mock("../../stores/buffers", () => ({
  bufferStore: {
    activeTabs: mocks.activeTabs,
    activeTabId: mocks.activeTabId,
    setActiveTabId: mocks.setActiveTabId,
    closeTab: mocks.closeTab,
    closeOtherTabs: mocks.closeOtherTabs,
    closeAllTabs: mocks.closeAllTabs,
    renameTab: mocks.renameTab,
    createTab: mocks.createTab,
  },
}));

vi.mock("../../components/ContextMenu/ContextMenu", () => ({
  showContextMenu: mocks.showContextMenu,
}));

import TabBar from "../../components/Editor/TabBar";

describe("TabBar close button (#47)", () => {
  afterEach(() => {
    mocks.closeTab.mockClear();
    mocks.setActiveTabId.mockClear();
    cleanup();
  });

  it("exposes role=button, tabIndex=0, and aria-label='Close <title>'", () => {
    const { container } = render(() => <TabBar />);
    const close = container.querySelector<HTMLElement>(".tab-close");
    expect(close).not.toBeNull();
    expect(close!.getAttribute("role")).toBe("button");
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
});
