import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import type { Platform } from "../../lib/platform";

const h = vi.hoisted(() => ({
  platform: "mac" as Platform,
  sidebarOpen: true,
}));

vi.mock("../../lib/platform", () => ({
  resolvePlatform: () => h.platform,
  detectPlatform: () => h.platform,
  IS_MAC: true,
}));
vi.mock("../../stores/global/os-window", () => ({
  osWindowStore: {
    focused: () => true,
    hide: vi.fn(),
    minimize: vi.fn(),
    toggleFullscreen: vi.fn(),
  },
}));
vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    sidebar: {
      isOpen: () => h.sidebarOpen,
      searchQuery: () => "",
      setSearchQuery: vi.fn(),
      searchHits: () => [],
      searchTotal: () => 0,
      searchMs: () => null,
    },
    tabs: {
      activeTabId: () => null,
      setActiveTabId: vi.fn(),
      closeTab: vi.fn(),
      restoreFromHistory: vi.fn(),
    },
    editor: { activeFormats: () => ({}) },
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
vi.mock("../../components/Sidebar/SidebarEmpty", () => ({ default: () => null }));

import Sidebar from "../../components/Sidebar/Sidebar";
import Toolbar from "../../components/Toolbar/Toolbar";

const ATTR = "data-tauri-drag-region";

// Ported from tauri 2.11.5 `src/window/scripts/drag.js` so the assertions are
// about what the runtime does with the markup, not about the markup alone: a
// bare attribute drags on direct hits only, `deep` drags anywhere in the
// subtree, and a clickable element carrying no attribute of its own stops the
// walk either way.
const CLICKABLE_TAGS = new Set(["A", "BUTTON", "INPUT", "SELECT", "TEXTAREA", "LABEL", "SUMMARY"]);
const INTERACTIVE_ROLES = new Set([
  "button",
  "link",
  "menuitem",
  "tab",
  "checkbox",
  "radio",
  "switch",
  "option",
]);

function isClickableElement(el: HTMLElement): boolean {
  return (
    CLICKABLE_TAGS.has(el.tagName) ||
    (el.hasAttribute("contenteditable") && el.getAttribute("contenteditable") !== "false") ||
    (el.hasAttribute("tabindex") && el.getAttribute("tabindex") !== "-1") ||
    INTERACTIVE_ROLES.has(el.getAttribute("role") ?? "")
  );
}

function dragsFrom(target: HTMLElement): boolean {
  const path: HTMLElement[] = [];
  for (let el: HTMLElement | null = target; el; el = el.parentElement) path.push(el);
  for (const el of path) {
    const attr = el.getAttribute(ATTR);
    if (isClickableElement(el) && attr === null) return false;
    if (attr === null) continue;
    if (attr === "false") return false;
    if (attr === "deep") return true;
    if (attr === "" || attr === "true") return el === path[0];
  }
  return false;
}

function pick(container: HTMLElement, selector: string): HTMLElement {
  const el = container.querySelector<HTMLElement>(selector);
  expect(el, selector).not.toBeNull();
  return el!;
}

afterEach(() => {
  cleanup();
  h.platform = "mac";
  h.sidebarOpen = true;
  vi.clearAllMocks();
});

describe("the macOS drag region covers the whole chrome row", () => {
  it("marks the toolbar as a subtree drag region", () => {
    const { container } = render(() => <Toolbar />);
    expect(pick(container, ".writ-toolbar").getAttribute(ATTR)).toBe("deep");
  });

  it("marks the sidebar head as a subtree drag region", () => {
    const { container } = render(() => <Sidebar />);
    expect(pick(container, ".sidebar-head").getAttribute(ATTR)).toBe("deep");
  });

  // The 44px band beside the lights is the strip every macOS window is moved
  // by. A bare attribute drags on direct hits only, so these wrappers were
  // dead zones.
  it("drags from the wrappers inside the toolbar", () => {
    h.sidebarOpen = false;
    const { container } = render(() => <Toolbar />);
    for (const selector of [".writ-toolbar", ".window-lights", ".writ-toolbar-cluster"]) {
      expect(dragsFrom(pick(container, selector)), selector).toBe(true);
    }
  });

  it("drags from the sidebar head and the lights wrapper it holds", () => {
    const { container } = render(() => <Sidebar />);
    expect(dragsFrom(pick(container, ".sidebar-head"))).toBe(true);
    expect(dragsFrom(pick(container, ".sidebar-head .window-lights"))).toBe(true);
  });

  it("leaves the controls clickable rather than draggable", () => {
    h.sidebarOpen = false;
    const { container } = render(() => <Toolbar />);
    const controls = container.querySelectorAll<HTMLElement>("button, input");
    expect(controls.length).toBeGreaterThan(0);
    for (const control of controls) {
      expect(control.hasAttribute(ATTR), control.className).toBe(false);
      expect(dragsFrom(control), control.className).toBe(false);
    }
  });

  it("leaves the lights clickable rather than draggable", () => {
    const { container } = render(() => <Sidebar />);
    const lights = container.querySelectorAll<HTMLElement>(".maclight");
    expect(lights).toHaveLength(3);
    for (const light of lights) {
      expect(light.hasAttribute(ATTR)).toBe(false);
      expect(dragsFrom(light)).toBe(false);
    }
  });

  it.each(["win", "linux"] as const)("leaves the %s toolbar to its own title bar", (p) => {
    h.platform = p;
    const { container } = render(() => <Toolbar />);
    expect(pick(container, ".writ-toolbar").hasAttribute(ATTR)).toBe(false);
    expect(dragsFrom(pick(container, ".writ-toolbar"))).toBe(false);
  });
});
