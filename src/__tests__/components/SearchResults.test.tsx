import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import type { SearchHit } from "../../services/tauri";
import type { BufferDocument } from "../../types/buffer";
import type { ContentHit, GrepOutcome } from "../../types/search";
import type { SearchStatus } from "../../stores/window/sidebar-store";

const h = await vi.hoisted(async () => {
  const { createSignal } = await import("solid-js");
  const [hits, setHits] = createSignal<SearchHit[]>([]);
  const [total, setTotal] = createSignal(0);
  const [status, setStatus] = createSignal<SearchStatus>("idle");
  const [query, setQuery] = createSignal("");
  const [folder, setFolder] = createSignal<string | null>(null);
  const [folderHits, setFolderHits] = createSignal<ContentHit[]>([]);
  const [folderStatus, setFolderStatus] = createSignal<SearchStatus>("idle");
  const [folderOutcome, setFolderOutcome] = createSignal<GrepOutcome | null>(null);
  const [activeResult, setActiveResult] = createSignal(-1);
  const [active, setActive] = createSignal<BufferDocument[]>([]);
  const [history, setHistory] = createSignal<BufferDocument[]>([]);
  return {
    hits, setHits, total, setTotal, status, setStatus, query, setQuery,
    folder, setFolder, folderHits, setFolderHits, folderStatus, setFolderStatus,
    folderOutcome, setFolderOutcome, activeResult, setActiveResult,
    active, setActive, history, setHistory,
    setActiveTabId: vi.fn(), restoreFromHistory: vi.fn(), requestReveal: vi.fn(),
    openFile: vi.fn(), retrySearch: vi.fn(),
  };
});

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    sidebar: {
      searchHits: h.hits,
      searchTotal: h.total,
      searchStatus: h.status,
      searchQuery: h.query,
      searchedFolder: h.folder,
      folderHits: h.folderHits,
      folderStatus: h.folderStatus,
      folderOutcome: h.folderOutcome,
      activeResult: h.activeResult,
      setActiveResult: h.setActiveResult,
      retrySearch: h.retrySearch,
    },
    tabs: {
      activeTabId: () => null,
      setActiveTabId: h.setActiveTabId,
      restoreFromHistory: h.restoreFromHistory,
      openFile: h.openFile,
      closeTab: vi.fn(),
      closeOtherTabs: vi.fn(),
    },
    editor: { requestReveal: h.requestReveal },
  }),
}));
vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: { activeTabs: h.active, historyList: h.history, deleteFromHistory: vi.fn() },
}));
vi.mock("../../stores/global/notes", () => ({
  notesStore: { root: () => "/Users/me/Writ" },
}));
vi.mock("../../components/ContextMenu/ContextMenu", () => ({ showContextMenu: vi.fn() }));

import SearchResults from "../../components/Sidebar/SearchResults";

function buf(id: string, title: string, status: "active" | "history"): BufferDocument {
  return {
    id, title, filename: `${id}.md`, status, language: null, source_path: null,
    cursor_pos: 0, scroll_pos: 0, tab_order: 0,
    created_at: "", updated_at: "", closed_at: null, read_only: false, size_bytes: 0, line_ending: "lf" as const, encoding: "UTF-8",
  };
}

afterEach(() => {
  h.setHits([]); h.setTotal(0); h.setStatus("idle"); h.setQuery("");
  h.setFolder(null); h.setFolderHits([]); h.setFolderStatus("idle"); h.setFolderOutcome(null);
  h.setActiveResult(-1);
  h.setActive([]); h.setHistory([]);
  h.setActiveTabId.mockReset(); h.restoreFromHistory.mockReset(); h.requestReveal.mockReset();
  h.openFile.mockReset(); h.retrySearch.mockReset();
  cleanup();
});

describe("SearchResults", () => {
  it("renders rows with file, highlighted hit, and line, and one count in the footer (NV-29)", () => {
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
    h.setStatus("done");

    const { container } = render(() => <SearchResults />);
    const row = container.querySelector(".search-result")!;
    expect(row.querySelector(".search-result-file")!.textContent).toBe("report.md");
    expect(row.querySelector(".search-result-line")!.textContent).toBe("line 5");
    expect(row.querySelector(".search-result-hit .is-match")!.textContent).toBe("rerank");
    const footers = container.querySelectorAll(".search-footer");
    expect(footers).toHaveLength(1);
    expect(footers[0].textContent).toBe("1 of 12 results");
    expect(container.textContent).not.toMatch(/\bms\b/);
  });

  it("names the folder it searched above its results (NV-03)", () => {
    h.setQuery("plan");
    h.setStatus("done");
    const { container } = render(() => <SearchResults />);
    expect(container.querySelector(".search-group-heading")!.textContent).toBe("Writ");
  });

  it("opens an active hit and reveals its line", () => {
    h.setActive([buf("o", "report.md", "active")]);
    h.setQuery("x");
    h.setHits([{ buffer_id: "o", title: "report.md", line: 8, snippet: [] }]);
    h.setStatus("done");

    const { container } = render(() => <SearchResults />);
    fireEvent.click(container.querySelector(".search-result")!);
    expect(h.setActiveTabId).toHaveBeenCalledWith("o");
    expect(h.requestReveal).toHaveBeenCalledWith("o", 8);
  });

  it("restores a history hit on click", () => {
    h.restoreFromHistory.mockResolvedValue(null);
    h.setHistory([buf("hh", "old.md", "history")]);
    h.setQuery("x");
    h.setHits([{ buffer_id: "hh", title: "old.md", line: null, snippet: [] }]);
    h.setStatus("done");

    const { container } = render(() => <SearchResults />);
    fireEvent.click(container.querySelector(".search-result")!);
    expect(h.restoreFromHistory).toHaveBeenCalledWith("hh");
    expect(h.requestReveal).not.toHaveBeenCalled();
  });

  it("reveals the hit's line once the history entry reopened", async () => {
    h.restoreFromHistory.mockResolvedValue(buf("hh", "old.md", "active"));
    h.setHistory([buf("hh", "old.md", "history")]);
    h.setQuery("x");
    h.setHits([{ buffer_id: "hh", title: "old.md", line: 12, snippet: [] }]);
    h.setStatus("done");

    const { container } = render(() => <SearchResults />);
    fireEvent.click(container.querySelector(".search-result")!);
    await vi.waitFor(() => expect(h.requestReveal).toHaveBeenCalledWith("hh", 12));
  });

  // Cancel on the size question, or a refusal, reopens nothing. A reveal left
  // queued would land on the line the next time the file opens another way,
  // which reuses this entry's id.
  it("reveals nothing for a history hit that did not reopen", async () => {
    h.restoreFromHistory.mockResolvedValue(null);
    h.setHistory([buf("hh", "big.log", "history")]);
    h.setQuery("x");
    h.setHits([{ buffer_id: "hh", title: "big.log", line: 12, snippet: [] }]);
    h.setStatus("done");

    const { container } = render(() => <SearchResults />);
    fireEvent.click(container.querySelector(".search-result")!);
    await vi.waitFor(() => expect(h.restoreFromHistory).toHaveBeenCalledWith("hh"));
    await Promise.resolve();
    await Promise.resolve();
    expect(h.requestReveal).not.toHaveBeenCalled();
  });

  it("shows No matches once a search has answered with none", () => {
    h.setQuery("zzz");
    h.setStatus("running");
    const { container } = render(() => <SearchResults />);
    expect(container.textContent).not.toContain("No matches");

    h.setStatus("done");
    expect(container.querySelector(".search-group-line")!.textContent).toBe("No matches");
  });

  it("says a failed search failed, apart from No matches, with Try again (NV-05)", () => {
    h.setQuery("zzz");
    h.setStatus("failed");
    const { container } = render(() => <SearchResults />);

    expect(container.textContent).toContain("Search failed");
    expect(container.textContent).not.toContain("No matches");
    fireEvent.click(container.querySelector<HTMLButtonElement>(".search-retry")!);
    expect(h.retrySearch).toHaveBeenCalledOnce();
  });

  it("lists the open folder's matches under its own name and opens one at its line", async () => {
    h.setQuery("render");
    h.setStatus("done");
    h.setFolder("/Users/me/code/app");
    h.setFolderStatus("done");
    h.setFolderHits([
      { path: "src/main.rs", line: 14, snippet: [{ text: "render", matched: true }] },
    ]);
    h.setFolderOutcome({
      hit_count: 3,
      files_scanned: 10,
      truncated: false,
      cancelled: false,
      skipped_not_downloaded: 0,
    });
    h.openFile.mockResolvedValue(buf("m", "main.rs", "active"));

    const { container } = render(() => <SearchResults />);
    const headings = Array.from(container.querySelectorAll(".search-group-heading")).map(
      (el) => el.textContent,
    );
    expect(headings).toEqual(["Writ", "app"]);
    const row = container.querySelector<HTMLElement>(".search-result")!;
    expect(row.querySelector(".search-result-file")!.textContent).toBe("src/main.rs");
    expect(container.querySelectorAll(".search-footer")[0].textContent).toBe("1 of 3 results");

    fireEvent.click(row);
    expect(h.openFile).toHaveBeenCalledWith("/Users/me/code/app/src/main.rs");
    await vi.waitFor(() => expect(h.requestReveal).toHaveBeenCalledWith("m", 14));
  });

  it("is a listbox whose options the field's arrows mark (NV-06)", () => {
    h.setActive([buf("a", "a.md", "active"), buf("b", "b.md", "active")]);
    h.setQuery("x");
    h.setHits([
      { buffer_id: "a", title: "a.md", line: 1, snippet: [] },
      { buffer_id: "b", title: "b.md", line: 2, snippet: [] },
    ]);
    h.setStatus("done");

    const { container } = render(() => <SearchResults />);
    const list = container.querySelector('[role="listbox"]')!;
    expect(list.id).toBe("sidebar-search-results");
    const options = Array.from(container.querySelectorAll<HTMLElement>('[role="option"]'));
    expect(options.map((o) => o.id)).toEqual([
      "sidebar-search-option-0",
      "sidebar-search-option-1",
    ]);
    expect(options.map((o) => o.getAttribute("aria-selected"))).toEqual(["false", "false"]);

    h.setActiveResult(1);
    expect(options.map((o) => o.getAttribute("aria-selected"))).toEqual(["false", "true"]);
  });
});
