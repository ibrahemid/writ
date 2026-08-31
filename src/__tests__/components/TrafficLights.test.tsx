import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import type { Platform } from "../../lib/platform";

const h = vi.hoisted(() => ({
  platform: "mac" as Platform,
  sidebarOpen: true,
  hide: vi.fn(),
  minimize: vi.fn(),
  toggleFullscreen: vi.fn(),
  focused: true,
}));

vi.mock("../../lib/platform", () => ({
  resolvePlatform: () => h.platform,
  detectPlatform: () => h.platform,
  IS_MAC: true,
}));
vi.mock("../../stores/global/os-window", () => ({
  osWindowStore: {
    focused: () => h.focused,
    hide: h.hide,
    minimize: h.minimize,
    toggleFullscreen: h.toggleFullscreen,
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

afterEach(() => {
  cleanup();
  h.platform = "mac";
  h.sidebarOpen = true;
  h.focused = true;
  vi.clearAllMocks();
});

describe("macOS lights in the sidebar head", () => {
  it("renders the three lights in the head while the sidebar is open", () => {
    const { container } = render(() => <Sidebar />);
    const head = container.querySelector(".sidebar-head");
    expect(head).not.toBeNull();
    const labels = Array.from(head!.querySelectorAll(".maclight")).map((el) =>
      el.getAttribute("aria-label"),
    );
    expect(labels).toEqual(["Hide window", "Minimize window", "Toggle full screen"]);
  });

  it("dims the lights when the window is not focused", () => {
    h.focused = false;
    const { container } = render(() => <Sidebar />);
    expect(container.querySelector(".window-lights")!.classList.contains("is-blurred")).toBe(true);
  });

  it("gives the head no lights once the sidebar closes", () => {
    h.sidebarOpen = false;
    const { container } = render(() => <Sidebar />);
    expect(container.querySelector(".sidebar-head")).toBeNull();
  });

  it.each(["win", "linux"] as const)("builds no head on %s, which has caption buttons", (p) => {
    h.platform = p;
    const { container } = render(() => <Sidebar />);
    expect(container.querySelector(".sidebar-head")).toBeNull();
    expect(container.querySelector(".maclight")).toBeNull();
  });
});

describe("macOS lights fall back to the toolbar", () => {
  // A closed sidebar is zero-width, clipped and inert. With no native
  // decorations, lights left in its head would leave no way to hide the window.
  it("moves the lights to the toolbar lead when the sidebar is closed", () => {
    h.sidebarOpen = false;
    const { container } = render(() => <Toolbar />);
    const lights = container.querySelector(".writ-toolbar > .window-lights");
    expect(lights).not.toBeNull();
    expect(lights!.querySelectorAll(".maclight")).toHaveLength(3);
  });

  it("leaves the toolbar clear while the head is carrying them", () => {
    const { container } = render(() => <Toolbar />);
    expect(container.querySelector(".window-lights")).toBeNull();
  });

  // The lights are window chrome, not note actions: the bar's roving group
  // never claims them, so they keep their own tab stops and the bar keeps one.
  it("keeps the lights out of the toolbar's roving tab order", () => {
    h.sidebarOpen = false;
    const { container } = render(() => <Toolbar />);
    for (const light of container.querySelectorAll<HTMLButtonElement>(".maclight")) {
      expect(light.hasAttribute("tabindex")).toBe(false);
    }
    const roving = container.querySelectorAll<HTMLButtonElement>("button[tabindex]");
    expect(roving.length).toBeGreaterThan(0);
    expect(Array.from(roving).filter((el) => el.tabIndex === 0)).toHaveLength(1);
    expect(Array.from(roving).some((el) => el.classList.contains("maclight"))).toBe(false);
  });

  it.each(["win", "linux"] as const)("draws no lights in the %s toolbar", (p) => {
    h.platform = p;
    h.sidebarOpen = false;
    const { container } = render(() => <Toolbar />);
    expect(container.querySelector(".window-lights")).toBeNull();
  });
});
