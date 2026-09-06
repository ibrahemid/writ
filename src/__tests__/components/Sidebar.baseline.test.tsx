import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import type { BufferDocument } from "../../types/buffer";

// The sidebar on the accepted baseline: sentence-case section headers named as
// plain nouns, rows on the row tokens, selection as a neutral fill with an
// accent icon, and no accent rail anywhere.

const h = vi.hoisted(() => ({
  active: [] as BufferDocument[],
  history: [] as BufferDocument[],
  activeId: null as string | null,
  root: null as string | null,
}));

vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    activeTabs: () => h.active,
    historyList: () => h.history,
    historyTotal: () => h.history.length,
    deleteFromHistory: vi.fn(),
    clearAllHistory: vi.fn(),
  },
}));
vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    sidebar: {
      isOpen: () => true,
      searchQuery: () => "",
      setSearchQuery: vi.fn(),
      searchHits: () => [],
      searchTotal: () => 0,
      searchMs: () => null,
      selectedTag: () => null,
    },
    tabs: {
      activeTabId: () => h.activeId,
      setActiveTabId: vi.fn(),
      closeTab: vi.fn(),
      closeOtherTabs: vi.fn(),
      restoreFromHistory: vi.fn(),
      openFile: vi.fn(),
    },
    editor: { requestReveal: vi.fn() },
  }),
}));
vi.mock("../../stores/global/workspace", () => ({
  workspaceStore: {
    root: () => h.root,
    closeFolder: vi.fn(),
    entriesFor: () => [],
    loadDir: vi.fn(),
  },
}));
vi.mock("../../components/ContextMenu/ContextMenu", () => ({ showContextMenu: vi.fn() }));

import ActiveSection from "../../components/Sidebar/ActiveSection";
import HistorySection from "../../components/Sidebar/HistorySection";
import FilesSection from "../../components/Sidebar/FilesSection";

const TAB_ITEM_CSS = readFileSync(
  resolve(process.cwd(), "src/components/Sidebar/TabItem.css"),
  "utf8",
);
const ACTIVE_CSS = readFileSync(
  resolve(process.cwd(), "src/components/Sidebar/ActiveSection.css"),
  "utf8",
);
const SIDEBAR_CSS = readFileSync(
  resolve(process.cwd(), "src/components/Sidebar/Sidebar.css"),
  "utf8",
);
const SEARCH_RESULTS_CSS = readFileSync(
  resolve(process.cwd(), "src/components/Sidebar/SearchResults.css"),
  "utf8",
);
const SIDEBAR_EMPTY_CSS = readFileSync(
  resolve(process.cwd(), "src/components/Sidebar/SidebarEmpty.css"),
  "utf8",
);

/** The declarations of one rule, by selector, comments stripped. */
function ruleFor(css: string, selector: string): Map<string, string> {
  const body = new RegExp(`\\${selector}\\s*\\{([^}]*)\\}`).exec(
    css.replace(/\/\*[\s\S]*?\*\//g, ""),
  );
  if (!body) throw new Error(`no rule for ${selector}`);
  const declarations = new Map<string, string>();
  for (const line of body[1].split(";")) {
    const [property, ...rest] = line.split(":");
    if (rest.length === 0) continue;
    declarations.set(property.trim(), rest.join(":").trim());
  }
  return declarations;
}

function doc(id: string, title: string, sourcePath: string | null): BufferDocument {
  return {
    id,
    title,
    filename: title,
    status: sourcePath ? "active" : "active",
    language: null,
    source_path: sourcePath,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: "2026-08-25T10:00:00.000Z",
    updated_at: "2026-08-25T10:00:00.000Z",
    closed_at: null,
    read_only: false,
    size_bytes: 0,
    line_ending: "lf",
  };
}

afterEach(() => {
  h.active = [];
  h.history = [];
  h.activeId = null;
  h.root = null;
  cleanup();
});

describe("sidebar section headers", () => {
  it("names the open section Open, in sentence case, with its count", () => {
    h.active = [doc("a", "Meeting notes", null), doc("b", "Pricing draft", null)];
    const { container } = render(() => <ActiveSection />);
    const head = container.querySelector(".sidebar-section-title")!;
    expect(head.textContent).toBe("Open2");
    expect(head.querySelector(".sidebar-section-count")!.textContent).toBe("2");
  });

  it("names the history section Recent, with its count", () => {
    h.history = [
      { ...doc("h1", "Kitchen rebuild", null), status: "history", closed_at: "2026-08-25T09:00:00.000Z" },
    ];
    const { container } = render(() => <HistorySection />);
    const head = container.querySelector(".sidebar-section-title")!;
    expect(head.textContent).toBe("Recent1");
  });

  it("names the folder section after the folder itself", () => {
    h.root = "/Users/me/Documents/Writ";
    const { container } = render(() => <FilesSection />);
    expect(container.querySelector(".sidebar-section-title")!.textContent).toBe("Writ");
  });

  it("is absent when no folder is open", () => {
    const { container } = render(() => <FilesSection />);
    expect(container.querySelector(".files-section")).toBeNull();
  });

  it("keeps section headers unshouted and untracked", () => {
    expect(SIDEBAR_CSS).toMatch(/text-transform:\s*none/);
    expect(SIDEBAR_CSS).toMatch(/letter-spacing:\s*var\(--writ-ui-tracking\)/);
    expect(SIDEBAR_CSS).toMatch(/font-size:\s*var\(--writ-ui-sm\)/);
    expect(SIDEBAR_CSS).not.toMatch(/text-transform:\s*uppercase/);
  });
});

describe("sidebar rows", () => {
  it("marks only the selected row, and draws no rail", () => {
    h.active = [doc("a", "Meeting notes", null), doc("b", "Pricing draft", null)];
    h.activeId = "b";
    const { container } = render(() => <ActiveSection />);
    const rows = Array.from(container.querySelectorAll(".tab-item"));
    expect(rows).toHaveLength(2);
    expect(rows.map((row) => row.classList.contains("tab-item-active"))).toEqual([false, true]);
    expect(container.querySelector(".tab-item-pill")).toBeNull();
    expect(TAB_ITEM_CSS).not.toContain(".tab-item-active::before");
  });

  it("carries selection as a neutral fill at weight 500", () => {
    expect(TAB_ITEM_CSS).toMatch(
      /\.tab-item-active\s*\{[^}]*background:\s*var\(--writ-bg-selected\)[^}]*font-weight:\s*500/,
    );
    expect(TAB_ITEM_CSS).toMatch(/\.tab-item:hover\s*\{\s*background:\s*var\(--writ-bg-hover\)/);
  });

  it("spends the accent on the selected row's icon and nowhere else on the row", () => {
    expect(TAB_ITEM_CSS).toMatch(
      /\.tab-item-active \.writ-icon\s*\{[^}]*color:\s*var\(--writ-accent\)/,
    );
    const rowRule = /\.tab-item\s*\{[^}]*\}/.exec(TAB_ITEM_CSS)![0];
    expect(rowRule).not.toContain("--writ-accent");
  });

  it("gives a search result the same box as a note row", () => {
    // A width beside the margins pushed the row past the scroller and clipped
    // the line number: both rows fill their parent minus 6px a side.
    const row = ruleFor(TAB_ITEM_CSS, ".tab-item");
    const result = ruleFor(SEARCH_RESULTS_CSS, ".search-result");
    expect(result.get("margin")).toBe(row.get("margin"));
    expect(result.get("width") ?? "auto").toBe("auto");
    expect(row.get("width") ?? "auto").toBe("auto");
    expect(result.get("border-radius")).toBe(row.get("border-radius"));
  });

  it("lets the empty card follow a narrowed sidebar", () => {
    const card = ruleFor(SIDEBAR_EMPTY_CSS, ".sidebar-empty");
    expect(card.get("width")).toBe("auto");
    expect(card.get("max-width")).toBe("212px");
    expect(card.get("margin")).toBe("var(--writ-space-3)");
    expect(ruleFor(SIDEBAR_EMPTY_CSS, ".sidebar-empty-line").get("flex-wrap")).toBe("wrap");
  });

  it("sits on the row pitch and radius tokens", () => {
    expect(TAB_ITEM_CSS).toMatch(/height:\s*var\(--writ-sidebar-row-fill\)/);
    expect(TAB_ITEM_CSS).toMatch(/border-radius:\s*var\(--writ-r-row\)/);
    expect(TAB_ITEM_CSS).toMatch(/margin:\s*1px 6px/);
  });

  it("gives every row a note icon", () => {
    h.active = [doc("a", "Meeting notes", null)];
    const { container } = render(() => <ActiveSection />);
    expect(container.querySelector(".tab-item use")!.getAttribute("href")).toBe("#ph-file-text");
  });

  it("indents a child row 16px past the parent label", () => {
    // The group head's label starts at 26px; a child's at 42px.
    expect(ACTIVE_CSS).toMatch(/\.active-group-items \.tab-item\s*\{\s*padding-left:\s*42px/);
    expect(ACTIVE_CSS).toMatch(
      /\[data-platform="win"\] \.active-group-items \.tab-item\s*\{\s*padding-left:\s*32px/,
    );
  });

  it("lists notes with no file behind them under Open with no group label", () => {
    h.active = [doc("a", "Meeting notes", null)];
    const { container } = render(() => <ActiveSection />);
    expect(container.querySelector(".active-group-head")).toBeNull();
    expect(container.textContent).not.toContain("Scratch");
  });

  it("still groups rows that share a folder, under that folder's name", () => {
    h.active = [
      doc("a", "Meeting notes", "/Users/me/Writ/Meeting notes.md"),
      doc("b", "Pricing draft", "/Users/me/Writ/Pricing draft.md"),
    ];
    const { container } = render(() => <ActiveSection />);
    const head = container.querySelector(".active-group-head")!;
    expect(head.querySelector(".active-group-name")!.textContent).toBe("Writ");
    expect(head.querySelector(".active-group-count")!.textContent).toBe("2");
  });
});
