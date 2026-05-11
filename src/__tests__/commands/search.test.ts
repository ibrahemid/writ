import { describe, it, expect, vi, beforeEach } from "vitest";

const { focusSearchBarMock } = vi.hoisted(() => ({
  focusSearchBarMock: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  searchBuffers: vi.fn().mockResolvedValue([]),
  getConfig: vi.fn(),
  updateConfig: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../components/Sidebar/SearchBar", () => ({
  focusSearchBar: focusSearchBarMock,
}));

import { openContentSearch } from "../../commands/search";
import { sidebarStore } from "../../stores/sidebar";

describe("openContentSearch", () => {
  beforeEach(() => {
    focusSearchBarMock.mockClear();
    sidebarStore.hide();
  });

  it("shows the sidebar and focuses the search bar", () => {
    expect(sidebarStore.isOpen()).toBe(false);

    openContentSearch();

    expect(sidebarStore.isOpen()).toBe(true);
    expect(focusSearchBarMock).toHaveBeenCalledOnce();
  });

  it("is idempotent when the sidebar is already visible", () => {
    sidebarStore.show();

    openContentSearch();

    expect(sidebarStore.isOpen()).toBe(true);
    expect(focusSearchBarMock).toHaveBeenCalledOnce();
  });
});
