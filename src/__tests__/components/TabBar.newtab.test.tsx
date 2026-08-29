import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";

const mocks = vi.hoisted(() => ({
  newNote: vi.fn(),
  setActiveTabId: vi.fn(),
  closeTab: vi.fn(),
  closeOtherTabs: vi.fn(),
  closeAllTabs: vi.fn(),
  renameBuffer: vi.fn(),
  activeTabId: vi.fn(() => null),
  activeTabs: vi.fn(() => []),
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
      newNote: mocks.newNote,
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
      tabs: { activeTabId: mocks.activeTabId },
    }),
  },
}));

vi.mock("../../components/ContextMenu/ContextMenu", () => ({
  showContextMenu: mocks.showContextMenu,
}));

import TabBar from "../../components/Editor/TabBar";

describe("TabBar new-note button (#46)", () => {
  afterEach(() => {
    mocks.newNote.mockClear();
    cleanup();
  });

  it("exposes accessible name 'New note'", () => {
    const { container } = render(() => <TabBar />);
    const newTab = container.querySelector<HTMLButtonElement>(".tabbar-new");
    expect(newTab).not.toBeNull();
    expect(newTab!.getAttribute("aria-label")).toBe("New note");
    expect(newTab!.getAttribute("type")).toBe("button");
  });

  it("clicking creates a note, which is a file in the notes folder", () => {
    const { container } = render(() => <TabBar />);
    const newTab = container.querySelector<HTMLButtonElement>(".tabbar-new")!;
    fireEvent.click(newTab);
    expect(mocks.newNote).toHaveBeenCalledTimes(1);
  });
});
