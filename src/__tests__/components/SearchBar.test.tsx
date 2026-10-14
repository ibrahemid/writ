import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import type { SearchHit } from "../../services/tauri";
import type { BufferDocument } from "../../types/buffer";
import type { SearchStatus } from "../../stores/window/sidebar-store";

const h = await vi.hoisted(async () => {
  const { createSignal } = await import("solid-js");
  const [query, setQuery] = createSignal("");
  const [hits, setHits] = createSignal<SearchHit[]>([]);
  const [status, setStatus] = createSignal<SearchStatus>("idle");
  const [activeResult, setActiveResult] = createSignal(-1);
  const [active, setActive] = createSignal<BufferDocument[]>([]);
  return {
    query, setQuery, hits, setHits, status, setStatus, activeResult, setActiveResult,
    active, setActive,
    setSearchQuery: vi.fn((next: string) => setQuery(next)),
    retrySearch: vi.fn(),
    focusEditor: vi.fn(),
    setActiveTabId: vi.fn(),
    requestReveal: vi.fn(),
  };
});

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    sidebar: {
      searchQuery: h.query,
      setSearchQuery: h.setSearchQuery,
      searchHits: h.hits,
      searchStatus: h.status,
      searchedFolder: () => null,
      folderHits: () => [],
      folderStatus: () => "idle",
      activeResult: h.activeResult,
      setActiveResult: h.setActiveResult,
      retrySearch: h.retrySearch,
    },
    tabs: { setActiveTabId: h.setActiveTabId },
    editor: { focusEditor: h.focusEditor, requestReveal: h.requestReveal },
  }),
}));

vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    activeTabs: h.active,
    historyList: () => [],
  },
}));

import SearchBar, { focusSearchBar } from "../../components/Sidebar/SearchBar";

function waitForAnimationFrame(): Promise<void> {
  return new Promise((resolve) => requestAnimationFrame(() => resolve()));
}

function buf(id: string, title: string): BufferDocument {
  return {
    id, title, filename: `${id}.md`, status: "active", language: null, source_path: null,
    cursor_pos: 0, scroll_pos: 0, tab_order: 0,
    created_at: "", updated_at: "", closed_at: null, read_only: false, size_bytes: 0,
    line_ending: "lf" as const, encoding: "UTF-8",
  };
}

afterEach(() => {
  h.setQuery("");
  h.setHits([]);
  h.setStatus("idle");
  h.setActiveResult(-1);
  h.setActive([]);
  vi.clearAllMocks();
  cleanup();
});

describe("SearchBar focus", () => {
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

describe("SearchBar wording", () => {
  it("names what the field searches, and carries no count of its own (NV-29)", () => {
    h.setQuery("plan");
    const { container } = render(() => <SearchBar />);
    const input = container.querySelector<HTMLInputElement>("input.search-input");
    expect(input?.placeholder).toBe("Search your files");
    expect(container.querySelector(".search-count")).toBeNull();
    expect(container.textContent).toBe("");
  });
});

describe("SearchBar as a combobox (NV-06)", () => {
  function withTwoResults() {
    h.setActive([buf("a", "alpha.md"), buf("b", "beta.md")]);
    h.setQuery("x");
    h.setHits([
      { buffer_id: "a", title: "alpha.md", line: 3, snippet: [] },
      { buffer_id: "b", title: "beta.md", line: 9, snippet: [] },
    ]);
    h.setStatus("done");
  }

  it("controls the result list and points at the option the arrows reached", () => {
    withTwoResults();
    const { container } = render(() => <SearchBar />);
    const input = container.querySelector<HTMLInputElement>("input.search-input")!;
    expect(input.getAttribute("role")).toBe("combobox");
    expect(input.getAttribute("aria-controls")).toBe("sidebar-search-results");
    expect(input.getAttribute("aria-expanded")).toBe("true");
    expect(input.hasAttribute("aria-activedescendant")).toBe(false);

    fireEvent.keyDown(input, { key: "ArrowDown" });
    expect(h.activeResult()).toBe(0);
    expect(input.getAttribute("aria-activedescendant")).toBe("sidebar-search-option-0");
    fireEvent.keyDown(input, { key: "ArrowDown" });
    fireEvent.keyDown(input, { key: "ArrowDown" });
    expect(h.activeResult()).toBe(1);
    fireEvent.keyDown(input, { key: "ArrowUp" });
    fireEvent.keyDown(input, { key: "ArrowUp" });
    expect(h.activeResult()).toBe(-1);
  });

  it("opens the option reached with Enter, or the first with none reached", () => {
    withTwoResults();
    const { container } = render(() => <SearchBar />);
    const input = container.querySelector<HTMLInputElement>("input.search-input")!;

    fireEvent.keyDown(input, { key: "Enter" });
    expect(h.setActiveTabId).toHaveBeenLastCalledWith("a");
    expect(h.requestReveal).toHaveBeenLastCalledWith("a", 3);

    fireEvent.keyDown(input, { key: "ArrowDown" });
    fireEvent.keyDown(input, { key: "ArrowDown" });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(h.setActiveTabId).toHaveBeenLastCalledWith("b");
    expect(h.requestReveal).toHaveBeenLastCalledWith("b", 9);
  });

  it("leaves an Enter that confirms an input method's text alone", () => {
    withTwoResults();
    const { container } = render(() => <SearchBar />);
    const input = container.querySelector<HTMLInputElement>("input.search-input")!;
    fireEvent.keyDown(input, { key: "Enter", keyCode: 229 });
    expect(h.setActiveTabId).not.toHaveBeenCalled();
  });

  it("runs a failed search again with Enter when nothing is listed", () => {
    h.setQuery("x");
    h.setStatus("failed");
    const { container } = render(() => <SearchBar />);
    fireEvent.keyDown(container.querySelector("input.search-input")!, { key: "Enter" });
    expect(h.retrySearch).toHaveBeenCalledOnce();
  });

  it("clears the query and returns to the editor on Escape", () => {
    withTwoResults();
    const { container } = render(() => <SearchBar />);
    fireEvent.keyDown(container.querySelector("input.search-input")!, { key: "Escape" });
    expect(h.setSearchQuery).toHaveBeenCalledWith("");
    expect(h.focusEditor).toHaveBeenCalledOnce();
  });
});
