import { describe, it, expect, vi, beforeEach } from "vitest";

const { focusSearchBarMock, showSidebar } = vi.hoisted(() => ({
  focusSearchBarMock: vi.fn(),
  showSidebar: vi.fn(),
}));

let sidebarOpen = false;

vi.mock("../../services/tauri", () => ({
  searchBuffers: vi.fn().mockResolvedValue([]),
  getConfig: vi.fn(),
  updateConfig: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../components/Sidebar/SearchBar", () => ({
  focusSearchBar: focusSearchBarMock,
}));

vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: {
    getActive: () => ({
      sidebar: {
        isOpen: () => sidebarOpen,
        show: () => {
          sidebarOpen = true;
          showSidebar();
        },
      },
    }),
  },
}));

import { openContentSearch } from "../../commands/search";

describe("openContentSearch", () => {
  beforeEach(() => {
    focusSearchBarMock.mockClear();
    showSidebar.mockClear();
    sidebarOpen = false;
  });

  it("shows the sidebar and focuses the search bar", () => {
    expect(sidebarOpen).toBe(false);

    openContentSearch();

    expect(sidebarOpen).toBe(true);
    expect(showSidebar).toHaveBeenCalledOnce();
    expect(focusSearchBarMock).toHaveBeenCalledOnce();
  });

  it("is idempotent when the sidebar is already visible", () => {
    sidebarOpen = true;

    openContentSearch();

    expect(sidebarOpen).toBe(true);
    expect(focusSearchBarMock).toHaveBeenCalledOnce();
  });
});
