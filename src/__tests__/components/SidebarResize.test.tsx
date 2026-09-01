import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";

// The sidebar's right edge is a separator: drag it, step it with the arrow
// keys, or double-click it back to the default. The settled width persists
// through the config store, never through a stylesheet edit.

const h = vi.hoisted(() => ({ width: 240, setSidebarWidth: vi.fn<(w: number) => void>() }));

vi.mock("../../stores/global/config", async () => {
  const actual =
    await vi.importActual<typeof import("../../stores/global/config")>(
      "../../stores/global/config",
    );
  return {
    ...actual,
    configStore: {
      config: () => ({ sidebar: { width: h.width } }),
      setSidebarWidth: h.setSidebarWidth,
    },
  };
});
vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    sidebar: {
      isOpen: () => true,
      searchQuery: () => "",
      setSearchQuery: vi.fn(),
      searchHits: () => [],
      searchTotal: () => 0,
      searchMs: () => null,
    },
    tabs: { activeTabId: () => null, setActiveTabId: vi.fn(), closeTab: vi.fn(), restoreFromHistory: vi.fn() },
  }),
}));
vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: { activeTabs: () => [], historyList: () => [], historyTotal: () => 0 },
}));
vi.mock("../../stores/global/workspace", () => ({ workspaceStore: { root: () => null } }));
vi.mock("../../stores/global/inbox", () => ({ inboxStore: { path: () => null } }));
vi.mock("../../components/Sidebar/SearchBar", () => ({
  default: () => <input class="sidebar-search-input" />,
  focusSearchBar: vi.fn(),
}));
vi.mock("../../components/Sidebar/ActiveSection", () => ({ default: () => null }));
vi.mock("../../components/Sidebar/FilesSection", () => ({ default: () => null }));
vi.mock("../../components/Sidebar/InboxSection", () => ({ default: () => null }));
vi.mock("../../components/Sidebar/HistorySection", () => ({ default: () => null }));
vi.mock("../../components/Sidebar/SearchResults", () => ({ default: () => null }));
vi.mock("../../components/Sidebar/SidebarEmpty", () => ({ default: () => <div class="sidebar-empty" /> }));

import Sidebar from "../../components/Sidebar/Sidebar";

const SIDEBAR_CSS = readFileSync(
  resolve(process.cwd(), "src/components/Sidebar/Sidebar.css"),
  "utf8",
);
const SIDEBAR_TSX = readFileSync(
  resolve(process.cwd(), "src/components/Sidebar/Sidebar.tsx"),
  "utf8",
);

function mount() {
  const { container } = render(() => <Sidebar />);
  return {
    container,
    sidebar: container.querySelector<HTMLElement>(".sidebar")!,
    handle: container.querySelector<HTMLElement>(".sidebar-resizer")!,
  };
}

function drag(handle: HTMLElement, from: number, to: number) {
  fireEvent.pointerDown(handle, { button: 0, pointerId: 1, clientX: from });
  fireEvent.pointerMove(handle, { pointerId: 1, clientX: to });
}

function liveWidth(sidebar: HTMLElement): string {
  return sidebar.style.getPropertyValue("--writ-sidebar-live-width");
}

beforeEach(() => {
  h.width = 240;
  h.setSidebarWidth.mockReset();
});

afterEach(() => cleanup());

describe("sidebar resize handle", () => {
  it("names itself as a vertical separator carrying the current width", () => {
    const { handle } = mount();
    expect(handle.getAttribute("role")).toBe("separator");
    expect(handle.getAttribute("aria-orientation")).toBe("vertical");
    expect(handle.getAttribute("aria-label")).toBe("Sidebar width");
    expect(handle.getAttribute("aria-valuemin")).toBe("200");
    expect(handle.getAttribute("aria-valuemax")).toBe("320");
    expect(handle.getAttribute("aria-valuenow")).toBe("240");
    expect(handle.tabIndex).toBe(0);
  });

  it("takes its width from the persisted value, with the token as the fallback", () => {
    h.width = 300;
    const { sidebar } = mount();
    expect(liveWidth(sidebar)).toBe("300px");
    expect(SIDEBAR_CSS).toContain(
      "width: var(--writ-sidebar-live-width, var(--writ-sidebar-width));",
    );
  });

  it("never floors the outer element's width, so expand doesn't pop to a min-width", () => {
    const openBlock = SIDEBAR_CSS.slice(
      SIDEBAR_CSS.indexOf(".sidebar.is-open {"),
      SIDEBAR_CSS.indexOf("}", SIDEBAR_CSS.indexOf(".sidebar.is-open {")),
    );
    expect(openBlock).not.toContain("min-width");
    expect(openBlock).not.toContain("max-width");
  });

  it("wraps the content in an inner element fixed at the live width", () => {
    const { sidebar } = mount();
    const inner = sidebar.querySelector<HTMLElement>(".sidebar-inner")!;
    expect(inner).not.toBeNull();
    // The head/search/scroll children live inside it, not as direct siblings
    // of the resizer, so they never reflow while the outer box animates.
    expect(inner.querySelector(".sidebar-scroll")).not.toBeNull();
    expect(sidebar.querySelector(":scope > .sidebar-resizer")).not.toBeNull();
    expect(SIDEBAR_CSS).toContain(
      "width: var(--writ-sidebar-live-width, var(--writ-sidebar-width));\n  display: flex;",
    );
  });

  it("follows the pointer during a drag without writing settings per frame", () => {
    const { sidebar, handle } = mount();
    drag(handle, 240, 280);
    expect(liveWidth(sidebar)).toBe("280px");
    expect(sidebar.classList.contains("is-resizing")).toBe(true);
    expect(h.setSidebarWidth).not.toHaveBeenCalled();
  });

  it("commits the settled width on release", () => {
    const { sidebar, handle } = mount();
    drag(handle, 240, 290);
    fireEvent.pointerUp(handle, { pointerId: 1, clientX: 290 });
    expect(h.setSidebarWidth).toHaveBeenCalledWith(290);
    expect(sidebar.classList.contains("is-resizing")).toBe(false);
  });

  it("clamps a drag at the lower bound", () => {
    const { sidebar, handle } = mount();
    drag(handle, 240, 10);
    expect(liveWidth(sidebar)).toBe("200px");
    fireEvent.pointerUp(handle, { pointerId: 1, clientX: 10 });
    expect(h.setSidebarWidth).toHaveBeenCalledWith(200);
  });

  it("clamps a drag at the upper bound", () => {
    const { sidebar, handle } = mount();
    drag(handle, 240, 900);
    expect(liveWidth(sidebar)).toBe("320px");
    fireEvent.pointerUp(handle, { pointerId: 1, clientX: 900 });
    expect(h.setSidebarWidth).toHaveBeenCalledWith(320);
  });

  it("ignores a pointer move that never started on the handle", () => {
    const { sidebar, handle } = mount();
    fireEvent.pointerMove(handle, { pointerId: 1, clientX: 900 });
    expect(liveWidth(sidebar)).toBe("240px");
  });

  it("steps 8px per arrow key while the separator is focused", () => {
    const { handle } = mount();
    fireEvent.keyDown(handle, { key: "ArrowRight" });
    expect(h.setSidebarWidth).toHaveBeenLastCalledWith(248);
    fireEvent.keyDown(handle, { key: "ArrowLeft" });
    expect(h.setSidebarWidth).toHaveBeenLastCalledWith(232);
  });

  it("leaves other keys alone", () => {
    const { handle } = mount();
    fireEvent.keyDown(handle, { key: "ArrowUp" });
    fireEvent.keyDown(handle, { key: "a" });
    expect(h.setSidebarWidth).not.toHaveBeenCalled();
  });

  it("resets to 240 on a double-click", () => {
    h.width = 312;
    const { handle } = mount();
    fireEvent.dblClick(handle);
    expect(h.setSidebarWidth).toHaveBeenCalledWith(240);
  });

  it("drags off pointer capture, not document listeners", () => {
    const spy = vi.spyOn(document, "addEventListener");
    const { handle } = mount();
    drag(handle, 240, 280);
    fireEvent.pointerUp(handle, { pointerId: 1, clientX: 280 });
    expect(spy).not.toHaveBeenCalled();
    spy.mockRestore();
    expect(SIDEBAR_TSX).not.toContain("document.addEventListener");
    expect(SIDEBAR_TSX).not.toContain("document.querySelector");
    expect(SIDEBAR_TSX).toContain("setPointerCapture");
  });
});
