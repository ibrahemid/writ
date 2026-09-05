import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";

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
    { id: "buf-2", title: "beta.md", filename: "beta.md", source_path: null },
  ]),
  activeTabId: vi.fn(() => "buf-1"),
  setActiveTabId: vi.fn(),
  closeTab: vi.fn(),
  closeOtherTabs: vi.fn(),
  closeAllTabs: vi.fn(),
  createTab: vi.fn(),
  renameBuffer: vi.fn(),
  showContextMenu: vi.fn(),
  removed: new Set<string>(),
}));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    editor: { isRemovedOnDisk: (id: string) => mocks.removed.has(id) },
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

import TabBar from "../../components/Editor/TabBar";

beforeEach(() => {
  mocks.removed.clear();
});

afterEach(cleanup);

describe("a tab whose file is gone", () => {
  it("carries no mark while the file is there", () => {
    const { container } = render(() => <TabBar />);
    expect(container.querySelectorAll(".tab-removed")).toHaveLength(0);
  });

  it("marks the one tab that lost its file", () => {
    mocks.removed.add("buf-2");
    const { container } = render(() => <TabBar />);

    const marked = container.querySelectorAll<HTMLElement>(".tab-removed");
    expect(marked).toHaveLength(1);
    expect(marked[0].textContent).toContain("beta.md");
  });

  it("says so in the tab's own words, not in a colour alone", () => {
    // The strike-through is CSS; the name a screen reader reads is what
    // carries the state without it.
    mocks.removed.add("buf-1");
    const { container } = render(() => <TabBar />);

    const marked = container.querySelector<HTMLElement>('.tab-removed [role="tab"]')!;
    expect(marked.getAttribute("aria-label")).toBe("alpha.md (deleted)");
  });
});
