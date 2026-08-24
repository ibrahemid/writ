import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { createSignal } from "solid-js";

const h = vi.hoisted(() => ({
  isOpen: (() => false) as () => boolean,
}));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    sidebar: {
      isOpen: () => h.isOpen(),
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

afterEach(() => {
  cleanup();
  h.isOpen = () => false;
});

describe("Sidebar collapsed state", () => {
  it("is inert and hidden from assistive tech while closed", async () => {
    const Sidebar = (await import("../../components/Sidebar/Sidebar")).default;
    const { container } = render(() => <Sidebar />);
    const el = container.querySelector<HTMLElement>(".sidebar")!;
    expect(el.classList.contains("is-open")).toBe(false);
    // Solid sets the `inert` property; browsers reflect it to the attribute,
    // jsdom does not, so the property is what the test can see.
    expect((el as HTMLElement & { inert: boolean }).inert).toBe(true);
    expect(el.getAttribute("aria-hidden")).toBe("true");
  });

  it("drops inert and aria-hidden once open", async () => {
    const Sidebar = (await import("../../components/Sidebar/Sidebar")).default;
    const [open, setOpen] = createSignal(false);
    h.isOpen = open;
    const { container } = render(() => <Sidebar />);
    setOpen(true);
    const el = container.querySelector<HTMLElement>(".sidebar")!;
    expect(el.classList.contains("is-open")).toBe(true);
    expect((el as HTMLElement & { inert: boolean }).inert).toBe(false);
    expect(el.hasAttribute("aria-hidden")).toBe(false);
  });
});
