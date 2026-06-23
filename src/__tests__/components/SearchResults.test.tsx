import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import type { SearchHit } from "../../services/tauri";
import type { BufferDocument } from "../../types/buffer";

const h = await vi.hoisted(async () => {
  const { createSignal } = await import("solid-js");
  const [hits, setHits] = createSignal<SearchHit[]>([]);
  const [total, setTotal] = createSignal(0);
  const [ms, setMs] = createSignal<number | null>(null);
  const [query, setQuery] = createSignal("");
  const [active, setActive] = createSignal<BufferDocument[]>([]);
  const [history, setHistory] = createSignal<BufferDocument[]>([]);
  return {
    hits, setHits, total, setTotal, ms, setMs, query, setQuery,
    active, setActive, history, setHistory,
    setActiveTabId: vi.fn(), restoreFromHistory: vi.fn(), requestReveal: vi.fn(),
  };
});

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    sidebar: { searchHits: h.hits, searchTotal: h.total, searchMs: h.ms, searchQuery: h.query },
    tabs: {
      activeTabId: () => null,
      setActiveTabId: h.setActiveTabId,
      restoreFromHistory: h.restoreFromHistory,
      closeTab: vi.fn(),
      closeOtherTabs: vi.fn(),
    },
    editor: { requestReveal: h.requestReveal },
  }),
}));
vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: { activeTabs: h.active, historyList: h.history, deleteFromHistory: vi.fn() },
}));
vi.mock("../../components/ContextMenu/ContextMenu", () => ({ showContextMenu: vi.fn() }));

import SearchResults from "../../components/Sidebar/SearchResults";

function buf(id: string, title: string, status: "active" | "history"): BufferDocument {
  return {
    id, title, filename: `${id}.md`, status, language: null, source_path: null,
    cursor_pos: 0, scroll_pos: 0, tab_order: 0,
    created_at: "", updated_at: "", closed_at: null, read_only: false, size_bytes: 0,
  };
}

afterEach(() => {
  h.setHits([]); h.setTotal(0); h.setMs(null); h.setQuery("");
  h.setActive([]); h.setHistory([]);
  h.setActiveTabId.mockReset(); h.restoreFromHistory.mockReset(); h.requestReveal.mockReset();
  cleanup();
});

describe("SearchResults", () => {
  it("renders rows with file, highlighted hit, and line, plus an N of M footer", () => {
    h.setActive([buf("o", "report.md", "active")]);
    h.setQuery("rerank");
    h.setHits([
      {
        buffer_id: "o",
        title: "report.md",
        line: 5,
        snippet: [
          { text: "index ", matched: false },
          { text: "rerank", matched: true },
        ],
      },
    ]);
    h.setTotal(12);
    h.setMs(3);

    const { container } = render(() => <SearchResults />);
    const row = container.querySelector(".search-result")!;
    expect(row.querySelector(".search-result-file")!.textContent).toBe("report.md");
    expect(row.querySelector(".search-result-line")!.textContent).toBe("L5");
    expect(row.querySelector(".search-result-hit .is-match")!.textContent).toBe("rerank");
    expect(container.querySelector(".search-footer")!.textContent).toContain("1 of 12");
    expect(container.querySelector(".search-footer")!.textContent).toContain("3 ms");
  });

  it("opens an active hit and reveals its line", () => {
    h.setActive([buf("o", "report.md", "active")]);
    h.setQuery("x");
    h.setHits([{ buffer_id: "o", title: "report.md", line: 8, snippet: [] }]);
    h.setMs(1);

    const { container } = render(() => <SearchResults />);
    fireEvent.click(container.querySelector(".search-result")!);
    expect(h.setActiveTabId).toHaveBeenCalledWith("o");
    expect(h.requestReveal).toHaveBeenCalledWith("o", 8);
  });

  it("restores a history hit on click", () => {
    h.setHistory([buf("hh", "old.md", "history")]);
    h.setQuery("x");
    h.setHits([{ buffer_id: "hh", title: "old.md", line: null, snippet: [] }]);
    h.setMs(1);

    const { container } = render(() => <SearchResults />);
    fireEvent.click(container.querySelector(".search-result")!);
    expect(h.restoreFromHistory).toHaveBeenCalledWith("hh");
    expect(h.requestReveal).not.toHaveBeenCalled();
  });

  it("shows the empty state once a search has run with no matches", () => {
    h.setQuery("zzz");
    h.setHits([]);
    h.setMs(2);
    const { container } = render(() => <SearchResults />);
    expect(container.querySelector(".tab-list-empty")!.textContent).toBe("No matches");
  });
});
