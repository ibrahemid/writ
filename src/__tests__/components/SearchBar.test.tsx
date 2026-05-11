import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";

vi.mock("../../stores/sidebar", () => ({
  sidebarStore: {
    searchQuery: () => "",
    setSearchQuery: vi.fn(),
  },
}));

import SearchBar, { focusSearchBar } from "../../components/Sidebar/SearchBar";

function waitForAnimationFrame(): Promise<void> {
  return new Promise((resolve) => requestAnimationFrame(() => resolve()));
}

describe("SearchBar focus", () => {
  afterEach(() => cleanup());

  it("focusSearchBar() focuses the rendered input element", async () => {
    const { container } = render(() => <SearchBar />);
    const input = container.querySelector<HTMLInputElement>("input.search-input");
    expect(input).not.toBeNull();

    focusSearchBar();
    await waitForAnimationFrame();

    expect(document.activeElement).toBe(input);
  });

  it("rebinds the ref after the component remounts", async () => {
    const first = render(() => <SearchBar />);
    first.unmount();

    const second = render(() => <SearchBar />);
    const input = second.container.querySelector<HTMLInputElement>("input.search-input");
    expect(input).not.toBeNull();

    focusSearchBar();
    await waitForAnimationFrame();

    expect(document.activeElement).toBe(input);
  });
});
