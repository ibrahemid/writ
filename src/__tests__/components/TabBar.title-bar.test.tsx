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
    { id: "buf-2", title: "beta.md", filename: "beta.md", source_path: null },
  ]),
  activeTabId: vi.fn(() => "buf-1"),
  setActiveTabId: vi.fn(),
  newNote: vi.fn(),
  startDragging: vi.fn(),
  titleBarDoubleClick: vi.fn(),
}));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    panes: {
      holds: () => true,
      activeIn: () => mocks.activeTabId(),
      isSplit: () => false,
      focused: () => "left",
      focus: () => {},
    },
    tabs: {
      selectIn: (_pane: string, id: string) => mocks.setActiveTabId(id),
      showDownloadPane: () => mocks.setActiveTabId(null),
      moveToOtherPane: () => {},
      activeTabId: mocks.activeTabId,
      setActiveTabId: mocks.setActiveTabId,
      closeTab: vi.fn(),
      newNote: mocks.newNote,
    },
    editor: { focusEditor: vi.fn(), isRemovedOnDisk: () => false },
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
    renameBuffer: vi.fn(() => Promise.resolve()),
  },
}));

vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: {
    getActive: () => ({ tabs: { activeTabId: mocks.activeTabId } }),
  },
}));

vi.mock("../../components/ContextMenu/ContextMenu", () => ({
  showContextMenu: vi.fn(),
  showAnchoredMenu: vi.fn(),
  isMenuOpenFor: () => false,
}));

vi.mock("../../stores/global/os-window", () => ({
  osWindowStore: {
    focused: () => true,
    startDragging: mocks.startDragging,
    titleBarDoubleClick: mocks.titleBarDoubleClick,
  },
}));

import TabBar from "../../components/Editor/TabBar";

function doubleClick(el: Element) {
  for (const detail of [1, 2]) {
    fireEvent.mouseDown(el, { button: 0, detail, clientX: 300, clientY: 8 });
    fireEvent.mouseUp(el, { button: 0, detail, clientX: 300, clientY: 8 });
    fireEvent.click(el, { button: 0, detail, clientX: 300, clientY: 8 });
  }
  fireEvent.dblClick(el, { button: 0, detail: 2, clientX: 300, clientY: 8 });
}

function pick(container: HTMLElement, selector: string): HTMLElement {
  const el = container.querySelector<HTMLElement>(selector);
  expect(el, selector).not.toBeNull();
  return el!;
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("the tab strip's empty stretch answers a double-click as the title bar does", () => {
  it.each([".tabbar-tabs", ".tabbar"])("a double-click on the empty %s is answered once", (selector) => {
    const { container } = render(() => <TabBar />);
    doubleClick(pick(container, selector));
    expect(mocks.titleBarDoubleClick).toHaveBeenCalledTimes(1);
  });

  // Tabs keep their press for moving between panes.
  it("a press on the empty strip does not move the window", () => {
    const { container } = render(() => <TabBar />);
    fireEvent.mouseDown(pick(container, ".tabbar-tabs"), { button: 0, detail: 1 });
    expect(mocks.startDragging).not.toHaveBeenCalled();
  });

  it("a double-click on a tab renames it and leaves the window alone", () => {
    const { container } = render(() => <TabBar />);
    doubleClick(pick(container, ".tab-label"));
    expect(container.querySelector(".tab-rename-input")).not.toBeNull();
    expect(mocks.titleBarDoubleClick).not.toHaveBeenCalled();
  });

  it("a double-click on the tab around its label leaves the window alone", () => {
    const { container } = render(() => <TabBar />);
    doubleClick(pick(container, ".tab"));
    expect(mocks.titleBarDoubleClick).not.toHaveBeenCalled();
  });

  it("a double-click on a tab's title text leaves the window alone", () => {
    const { container } = render(() => <TabBar />);
    doubleClick(pick(container, ".tab-title"));
    expect(mocks.titleBarDoubleClick).not.toHaveBeenCalled();
  });

  it.each([".tab-close", ".tab-add"])("a double-click on %s leaves the window alone", (selector) => {
    const { container } = render(() => <TabBar />);
    doubleClick(pick(container, selector));
    expect(mocks.titleBarDoubleClick).not.toHaveBeenCalled();
    expect(mocks.startDragging).not.toHaveBeenCalled();
  });

  it("a double-click on the rename field leaves the window alone", () => {
    const { container } = render(() => <TabBar />);
    fireEvent.dblClick(pick(container, ".tab-label"), { button: 0, detail: 2 });
    doubleClick(pick(container, ".tab-rename-input"));
    expect(mocks.titleBarDoubleClick).not.toHaveBeenCalled();
  });

  it("a single click on the empty strip changes nothing", () => {
    const { container } = render(() => <TabBar />);
    const strip = pick(container, ".tabbar-tabs");
    fireEvent.mouseDown(strip, { button: 0, detail: 1 });
    fireEvent.mouseUp(strip, { button: 0, detail: 1 });
    fireEvent.click(strip, { button: 0, detail: 1 });
    expect(mocks.titleBarDoubleClick).not.toHaveBeenCalled();
    expect(mocks.startDragging).not.toHaveBeenCalled();
  });
});
