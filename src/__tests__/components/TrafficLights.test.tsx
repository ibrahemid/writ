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
import WindowLights from "../../components/TitleBar/WindowLights";

/** The chrome row as the window builds it, so the count is the real one. */
function Chrome() {
  return (
    <div class="app-body">
      <Sidebar />
      <Toolbar />
      <WindowLights />
    </div>
  );
}

afterEach(() => {
  cleanup();
  h.platform = "mac";
  h.sidebarOpen = true;
  h.focused = true;
  vi.clearAllMocks();
});

describe("the macOS lights have one host", () => {
  it("renders the three lights in the window layer", () => {
    const { container } = render(() => <WindowLights />);
    const layer = container.querySelector(".window-lights-layer");
    expect(layer).not.toBeNull();
    const labels = Array.from(layer!.querySelectorAll(".maclight")).map((el) =>
      el.getAttribute("aria-label"),
    );
    expect(labels).toEqual(["Hide window", "Minimize window", "Toggle full screen"]);
  });

  it("dims the lights when the window is not focused", () => {
    h.focused = false;
    const { container } = render(() => <WindowLights />);
    expect(container.querySelector(".window-lights")!.classList.contains("is-blurred")).toBe(true);
  });

  // A slot that swapped with the sidebar state moved the lights on the first
  // frame of the width animation. One host, owned by neither animated box, is
  // what holds them still.
  it.each([true, false])("keeps that host alone and outside both panes (open: %s)", (open) => {
    h.sidebarOpen = open;
    const { container } = render(() => <Chrome />);
    const lights = container.querySelectorAll(".window-lights");
    expect(lights).toHaveLength(1);
    expect(lights[0].closest(".sidebar")).toBeNull();
    expect(lights[0].closest(".writ-toolbar")).toBeNull();
    expect(lights[0].closest(".window-lights-layer")).not.toBeNull();
  });

  it.each(["win", "linux"] as const)("draws no lights on %s, which has caption buttons", (p) => {
    h.platform = p;
    const { container } = render(() => <Chrome />);
    expect(container.querySelector(".window-lights-layer")).toBeNull();
    expect(container.querySelector(".maclight")).toBeNull();
  });
});

describe("the panes the lights sit over", () => {
  // Unmounting the head on close would drop the sidebar's content 44px while
  // the sidebar is still visibly sliding out.
  it.each([true, false])("keeps the macOS sidebar head in both states (open: %s)", (open) => {
    h.sidebarOpen = open;
    const { container } = render(() => <Sidebar />);
    expect(container.querySelector(".sidebar-head")).not.toBeNull();
    expect(container.querySelector(".maclight")).toBeNull();
  });

  it.each(["win", "linux"] as const)("builds no head on %s", (p) => {
    h.platform = p;
    const { container } = render(() => <Sidebar />);
    expect(container.querySelector(".sidebar-head")).toBeNull();
  });

  it("reserves the toolbar's lead only while the sidebar is closed", () => {
    h.sidebarOpen = false;
    const closed = render(() => <Toolbar />);
    expect(closed.container.querySelector(".writ-toolbar")!.classList).toContain("leads-lights");
    cleanup();
    h.sidebarOpen = true;
    const open = render(() => <Toolbar />);
    expect(open.container.querySelector(".writ-toolbar")!.classList).not.toContain("leads-lights");
  });

  it.each(["win", "linux"] as const)("reserves no lead on %s", (p) => {
    h.platform = p;
    h.sidebarOpen = false;
    const { container } = render(() => <Toolbar />);
    expect(container.querySelector(".writ-toolbar")!.classList).not.toContain("leads-lights");
  });

  // The lights are window chrome, not note actions: the bar's roving group
  // never claimed them, and now they are not in the bar at all.
  it("leaves the toolbar's roving tab order to the bar's own controls", () => {
    h.sidebarOpen = false;
    const { container } = render(() => <Toolbar />);
    const roving = container.querySelectorAll<HTMLButtonElement>("button[tabindex]");
    expect(roving.length).toBeGreaterThan(0);
    expect(Array.from(roving).filter((el) => el.tabIndex === 0)).toHaveLength(1);
  });

  it("gives the lights no tab stop of their own", () => {
    const { container } = render(() => <WindowLights />);
    for (const light of container.querySelectorAll<HTMLButtonElement>(".maclight")) {
      expect(light.hasAttribute("tabindex")).toBe(false);
    }
  });
});
