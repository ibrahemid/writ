import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";
import type { BufferDocument } from "../../types/buffer";
import type { FileHit, GrepOutcome, IndexStatus } from "../../types/search";

beforeAll(() => {
  if (!(Element.prototype as { scrollIntoView?: () => void }).scrollIntoView) {
    Element.prototype.scrollIntoView = function scrollIntoView() {};
  }
});

const h = vi.hoisted(() => ({
  usage: {} as Record<string, unknown>,
  recordCommandUse: vi.fn(),
  openSettings: vi.fn(),
  activeTabs: [] as unknown[],
  historyList: [] as unknown[],
  root: "/repo" as string | null,
  activeTabId: "a" as string | null,
  status: { file_count: 3, truncated: false, has_workspace: true } as IndexStatus,
  outcome: null as GrepOutcome | null,
  searchFiles: vi.fn(),
  streamContent: vi.fn(),
  refreshIndexStatus: vi.fn(),
  searchBuffers: vi.fn(),
  requestReveal: vi.fn(),
  setActiveTabId: vi.fn(),
  openFile: vi.fn(),
  restoreFromHistory: vi.fn(),
}));

vi.mock("../../stores/global/config", () => ({
  configStore: {
    config: () => ({ commands: { usage: h.usage } }),
    recordCommandUse: h.recordCommandUse,
  },
}));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({ editor: { focusEditor: vi.fn() } }),
}));

vi.mock("../../components/SettingsModal/SettingsModal", () => ({
  openSettings: h.openSettings,
  default: () => null,
}));

vi.mock("../../settings/availability", () => ({
  isSettingAvailable: () => true,
}));

vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    activeTabs: () => h.activeTabs,
    historyList: () => h.historyList,
  },
}));

vi.mock("../../stores/global/workspace", () => ({
  workspaceStore: { root: () => h.root },
}));

vi.mock("../../stores/global/workspace-search", () => ({
  workspaceSearchStore: {
    indexStatus: () => h.status,
    lastOutcome: () => h.outcome,
    refreshIndexStatus: h.refreshIndexStatus,
    searchFiles: h.searchFiles,
    streamContent: h.streamContent,
  },
}));

vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: {
    getActive: () => ({
      tabs: {
        activeTabId: () => h.activeTabId,
        setActiveTabId: h.setActiveTabId,
        restoreFromHistory: h.restoreFromHistory,
        openFile: h.openFile,
      },
      editor: { requestReveal: h.requestReveal },
    }),
  },
}));

vi.mock("../../services/tauri", () => ({
  searchBuffers: h.searchBuffers,
}));

import SearchPalette, {
  openSearchPalette,
  closeSearchPalette,
} from "../../components/SearchPalette/SearchPalette";
import { registerCommand, getAllCommands, unregisterCommand } from "../../commands/registry";

function doc(id: string, title: string, sourcePath: string | null = null): BufferDocument {
  return {
    id,
    title,
    filename: `${id}.md`,
    status: "active",
    language: null,
    source_path: sourcePath,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: "",
    updated_at: "",
    closed_at: null,
    read_only: false,
    size_bytes: 0,
  };
}

function fileHit(path: string): FileHit {
  return { path, name: path.slice(path.lastIndexOf("/") + 1), score: 10 };
}

function input(): HTMLInputElement {
  return document.querySelector<HTMLInputElement>(".palette-input")!;
}

function items(): HTMLElement[] {
  return Array.from(document.querySelectorAll<HTMLElement>(".palette-item"));
}

function labels(): string[] {
  return items().map((el) => el.querySelector(".palette-item-label")?.textContent ?? "");
}

function sectionLabels(kind: string): string[] {
  return Array.from(
    document.querySelectorAll(`.palette-section-${kind} .palette-item-label`),
  ).map((el) => el.textContent ?? "");
}

async function type(value: string) {
  fireEvent.input(input(), { target: { value } });
  await Promise.resolve();
}

async function open() {
  render(() => <SearchPalette />);
  openSearchPalette();
  await waitFor(() => expect(document.querySelector(".palette-input")).not.toBeNull());
}

describe("SearchPalette", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    for (const cmd of [...getAllCommands()]) unregisterCommand(cmd.id);
    h.activeTabs = [];
    h.historyList = [];
    h.root = "/repo";
    h.activeTabId = "a";
    h.status = { file_count: 3, truncated: false, has_workspace: true };
    h.outcome = null;
    h.searchFiles.mockResolvedValue([]);
    h.streamContent.mockResolvedValue(undefined);
    h.searchBuffers.mockResolvedValue({ hits: [], total: 0 });
    h.openFile.mockResolvedValue(doc("opened", "opened"));
    h.restoreFromHistory.mockResolvedValue(undefined);
    registerCommand({
      id: "cmd.zebra",
      label: "Zebra command",
      scope: "app",
      keybinding: "CmdOrCtrl+Z",
      execute: vi.fn(),
    });
    registerCommand({
      id: "search.openEverywhere",
      label: "Search Everywhere",
      scope: "app",
      execute: vi.fn(),
    });
  });

  afterEach(() => {
    closeSearchPalette();
    cleanup();
    while (document.body.firstChild) document.body.removeChild(document.body.firstChild);
  });

  it("refreshes the index status once per open", async () => {
    await open();
    expect(h.refreshIndexStatus).toHaveBeenCalledTimes(1);
  });

  it("never offers its own opener", async () => {
    await open();
    await type("search");
    await waitFor(() => expect(labels()).not.toContain("Search Everywhere"));
  });

  it("composes commands, files and content in one list", async () => {
    h.activeTabs = [doc("a", "zebra.md", "/repo/zebra.md")];
    h.searchFiles.mockResolvedValue([fileHit("src/zebra.rs")]);
    h.streamContent.mockImplementation(
      async (_q: string, onBatch: (b: { hits: unknown[]; outcome: null }) => void) => {
        onBatch({
          hits: [{ path: "src/other.rs", line: 3, snippet: [{ text: "zebra", matched: true }] }],
          outcome: null,
        });
      },
    );
    await open();
    await type("zebra");
    await waitFor(() => {
      expect(labels()).toEqual(["Zebra command", "zebra.md", "zebra.rs", "other.rs"]);
    });
  });

  it("keeps streamed hits that arrive before the buffer search resolves", async () => {
    h.activeTabs = [doc("a", "zebra.md", "/repo/zebra.md")];
    h.streamContent.mockImplementation(
      async (_q: string, onBatch: (b: { hits: unknown[]; outcome: null }) => void) => {
        onBatch({ hits: [{ path: "src/early.rs", line: 1, snippet: [] }], outcome: null });
      },
    );
    let resolveBuffers: (r: unknown) => void = () => {};
    h.searchBuffers.mockImplementation(
      () => new Promise((resolve) => { resolveBuffers = resolve; }),
    );

    await open();
    await type("zebra");
    await waitFor(() => expect(sectionLabels("content")).toEqual(["early.rs"]));

    resolveBuffers({
      hits: [{ buffer_id: "a", title: "zebra.md", line: 9, snippet: [] }],
      total: 1,
    });
    await waitFor(() => expect(sectionLabels("content")).toEqual(["zebra.md", "early.rs"]));
  });

  describe("prefix routing", () => {
    it("'>' runs only the command provider", async () => {
      h.searchFiles.mockResolvedValue([fileHit("src/zebra.rs")]);
      await open();
      await type(">zebra");
      await waitFor(() => expect(labels()).toEqual(["Zebra command"]));
      await new Promise((r) => setTimeout(r, 200));
      expect(h.searchFiles).not.toHaveBeenCalled();
      expect(h.streamContent).not.toHaveBeenCalled();
    });

    it("'#' runs only the content provider", async () => {
      h.searchFiles.mockResolvedValue([fileHit("src/zebra.rs")]);
      h.streamContent.mockImplementation(
        async (_q: string, onBatch: (b: { hits: unknown[]; outcome: null }) => void) => {
          onBatch({ hits: [{ path: "src/other.rs", line: 3, snippet: [] }], outcome: null });
        },
      );
      await open();
      await type("#zebra");
      await waitFor(() => expect(labels()).toEqual(["other.rs"]));
      expect(h.searchFiles).not.toHaveBeenCalled();
    });

    it("':' offers the go-to-line jump alone", async () => {
      h.activeTabs = [doc("a", "zebra.md")];
      await open();
      await type(":42");
      await waitFor(() => expect(labels()).toEqual(["Go to line 42"]));
      fireEvent.keyDown(input(), { key: "Enter" });
      expect(h.requestReveal).toHaveBeenCalledWith("a", 42);
    });
  });

  it("debounces the async providers and leaves the sync ones instant", async () => {
    await open();
    await type("zebra");
    expect(labels()).toEqual(["Zebra command"]);
    expect(h.searchFiles).not.toHaveBeenCalled();
    await waitFor(() => expect(h.searchFiles).toHaveBeenCalledWith("zebra"));
  });

  it("discards results from a superseded query", async () => {
    let resolveFirst: (hits: FileHit[]) => void = () => {};
    h.searchFiles
      .mockImplementationOnce(
        () => new Promise<FileHit[]>((resolve) => { resolveFirst = resolve; }),
      )
      .mockResolvedValue([fileHit("src/second.rs")]);

    await open();
    await type("first");
    await waitFor(() => expect(h.searchFiles).toHaveBeenCalledWith("first"));

    await type("second");
    await waitFor(() => expect(h.searchFiles).toHaveBeenCalledWith("second"));
    await waitFor(() => expect(labels()).toContain("second.rs"));

    resolveFirst([fileHit("src/first.rs")]);
    await new Promise((r) => setTimeout(r, 20));
    expect(labels()).not.toContain("first.rs");
  });

  it("drops streamed batches that arrive after the query changed", async () => {
    const pending: ((b: { hits: unknown[]; outcome: null }) => void)[] = [];
    h.streamContent.mockImplementation(
      async (_q: string, onBatch: (b: { hits: unknown[]; outcome: null }) => void) => {
        pending.push(onBatch);
        await new Promise((r) => setTimeout(r, 500));
      },
    );
    await open();
    await type("first");
    await waitFor(() => expect(pending).toHaveLength(1));
    await type("second");
    await waitFor(() => expect(pending).toHaveLength(2));

    pending[0]({ hits: [{ path: "stale.rs", line: 1, snippet: [] }], outcome: null });
    await new Promise((r) => setTimeout(r, 20));
    expect(labels()).not.toContain("stale.rs");
  });

  it("caps a provider and states the count instead of truncating silently", async () => {
    h.searchFiles.mockResolvedValue(
      Array.from({ length: 20 }, (_, i) => fileHit(`src/zebra${i}.rs`)),
    );
    await open();
    await type("zebra");
    await waitFor(() => {
      const more = document.querySelector(".palette-section-files .palette-section-more");
      expect(more?.textContent).toBe("Showing 8 of 20");
    });
  });

  it("states index truncation", async () => {
    h.status = { file_count: 200000, truncated: true, has_workspace: true };
    await open();
    await waitFor(() =>
      expect(document.querySelector(".palette-notice")?.textContent).toBe(
        `File index capped at ${(200000).toLocaleString()} files`,
      ),
    );
  });

  it("states a capped content search", async () => {
    h.outcome = { hit_count: 500, files_scanned: 900, truncated: true, cancelled: false };
    await open();
    await waitFor(() =>
      expect(document.querySelector(".palette-notice")?.textContent).toBe(
        "Content search stopped at 500 matches",
      ),
    );
  });

  describe("keyboard and ARIA", () => {
    beforeEach(() => {
      h.activeTabs = [doc("a", "zebra.md", "/repo/zebra.md")];
      h.searchFiles.mockResolvedValue([fileHit("src/zebra.rs")]);
    });

    it("marks the list as a listbox and every row as an option", async () => {
      await open();
      await type("zebra");
      await waitFor(() => expect(items().length).toBeGreaterThan(1));
      const list = document.querySelector(".palette-results")!;
      expect(list.getAttribute("role")).toBe("listbox");
      for (const item of items()) {
        expect(item.getAttribute("role")).toBe("option");
        expect(item.id).not.toBe("");
      }
    });

    it("names each section group for assistive tech", async () => {
      await open();
      await type("zebra");
      await waitFor(() => expect(items().length).toBeGreaterThan(1));
      const groups = Array.from(document.querySelectorAll('[role="group"]'));
      expect(groups.map((g) => g.getAttribute("aria-label"))).toEqual(["Commands", "Files"]);
    });

    it("tracks the selection with aria-activedescendant", async () => {
      await open();
      await type("zebra");
      await waitFor(() => expect(items().length).toBeGreaterThan(1));
      const rows = items();
      expect(input().getAttribute("aria-activedescendant")).toBe(rows[0].id);
      expect(rows[0].getAttribute("aria-selected")).toBe("true");

      fireEvent.keyDown(input(), { key: "ArrowDown" });
      expect(input().getAttribute("aria-activedescendant")).toBe(rows[1].id);
      expect(rows[1].getAttribute("aria-selected")).toBe("true");
      expect(rows[0].getAttribute("aria-selected")).toBe("false");
    });

    it("moves the selection across a section boundary", async () => {
      await open();
      await type("zebra");
      await waitFor(() => expect(labels()).toEqual(["Zebra command", "zebra.md", "zebra.rs"]));
      fireEvent.keyDown(input(), { key: "ArrowDown" });
      fireEvent.keyDown(input(), { key: "ArrowDown" });
      const selected = document.querySelector(".palette-item.is-selected");
      expect(selected?.querySelector(".palette-item-label")?.textContent).toBe("zebra.rs");
    });

    it("keeps the option rows out of the tab order", async () => {
      await open();
      await type("zebra");
      await waitFor(() => expect(items().length).toBeGreaterThan(1));
      for (const item of items()) expect(item.getAttribute("tabindex")).toBe("-1");
    });

    it("announces the result count in a live region", async () => {
      await open();
      await type("zebra");
      await waitFor(() =>
        expect(document.querySelector(".palette-live")?.textContent).toBe("3 results"),
      );
      const live = document.querySelector(".palette-live")!;
      expect(live.getAttribute("aria-live")).toBe("polite");
    });

    it("opens the selected workspace file on Enter", async () => {
      await open();
      await type("zebra");
      await waitFor(() => expect(labels()).toHaveLength(3));
      fireEvent.keyDown(input(), { key: "ArrowDown" });
      fireEvent.keyDown(input(), { key: "ArrowDown" });
      fireEvent.keyDown(input(), { key: "Enter" });
      expect(h.openFile).toHaveBeenCalledWith("/repo/src/zebra.rs");
      expect(document.querySelector(".palette")).toBeNull();
    });
  });
});
