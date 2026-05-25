import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";

const mocks = vi.hoisted(() => ({
  createTab: vi.fn(),
  setActiveTabId: vi.fn(),
  closeTab: vi.fn(),
  closeOtherTabs: vi.fn(),
  closeAllTabs: vi.fn(),
  renameTab: vi.fn(),
  showContextMenu: vi.fn(),
}));

vi.mock("../../stores/buffers", () => ({
  bufferStore: {
    activeTabs: () => [],
    activeTabId: () => null,
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

describe("TabBar new-tab button (#46)", () => {
  afterEach(() => {
    mocks.createTab.mockClear();
    cleanup();
  });

  it("exposes accessible name 'New tab'", () => {
    const { container } = render(() => <TabBar />);
    const newTab = container.querySelector<HTMLButtonElement>(".tabbar-new");
    expect(newTab).not.toBeNull();
    expect(newTab!.getAttribute("aria-label")).toBe("New tab");
    expect(newTab!.getAttribute("type")).toBe("button");
  });

  it("clicking invokes createTab", () => {
    const { container } = render(() => <TabBar />);
    const newTab = container.querySelector<HTMLButtonElement>(".tabbar-new")!;
    fireEvent.click(newTab);
    expect(mocks.createTab).toHaveBeenCalledTimes(1);
  });
});
