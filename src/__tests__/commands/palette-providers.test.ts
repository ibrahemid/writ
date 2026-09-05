import { describe, it, expect, vi, beforeEach } from "vitest";
import type { BufferDocument } from "../../types/buffer";

const h = vi.hoisted(() => ({
  activeTabs: [] as BufferDocument[],
  historyList: [] as BufferDocument[],
  root: null as string | null,
  searchFiles: vi.fn(),
  streamContent: vi.fn(),
  searchBuffers: vi.fn(),
  setActiveTabId: vi.fn(),
  restoreFromHistory: vi.fn(),
  openFile: vi.fn(),
  requestReveal: vi.fn(),
  activeTabId: null as string | null,
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

import { createFilesProvider } from "../../commands/providers/files-provider";
import { createContentProvider } from "../../commands/providers/content-provider";
import { createGotoLineProvider } from "../../commands/providers/goto-line-provider";

function doc(over: Partial<BufferDocument> & { id: string; title: string }): BufferDocument {
  return {
    filename: `${over.id}.md`,
    status: "active",
    language: null,
    source_path: null,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: "",
    updated_at: "",
    closed_at: null,
    read_only: false,
    size_bytes: 0,
    line_ending: "lf",
    ...over,
  };
}

const signal = () => new AbortController().signal;

beforeEach(() => {
  vi.clearAllMocks();
  h.activeTabs = [];
  h.historyList = [];
  h.root = null;
  h.activeTabId = null;
  h.searchFiles.mockResolvedValue([]);
  h.streamContent.mockResolvedValue(undefined);
  h.searchBuffers.mockResolvedValue({ hits: [], total: 0 });
  h.restoreFromHistory.mockResolvedValue(undefined);
  h.openFile.mockResolvedValue(doc({ id: "opened", title: "opened" }));
  console.error = vi.fn();
});

describe("files provider", () => {
  it("lists open tabs and history on an empty query", async () => {
    h.activeTabs = [doc({ id: "a", title: "Alpha" })];
    h.historyList = [doc({ id: "b", title: "Bravo", status: "history" })];
    const results = await createFilesProvider().query("", signal(), "all");
    expect(results.map((r) => r.label)).toEqual(["Alpha", "Bravo"]);
    expect(h.searchFiles).not.toHaveBeenCalled();
  });

  it("matches buffers on title and on path", async () => {
    h.activeTabs = [
      doc({ id: "a", title: "Alpha", source_path: "/repo/docs/alpha.md" }),
      doc({ id: "b", title: "Bravo", source_path: "/repo/src/main.rs" }),
    ];
    const results = await createFilesProvider().query("main", signal(), "all");
    expect(results.map((r) => r.label)).toEqual(["Bravo"]);
  });

  it("unions workspace hits after the buffers", async () => {
    h.root = "/repo";
    h.activeTabs = [doc({ id: "a", title: "main.md", source_path: "/repo/docs/main.md" })];
    h.searchFiles.mockResolvedValue([{ path: "src/main.rs", name: "main.rs", score: 9 }]);
    const results = await createFilesProvider().query("main", signal(), "all");
    expect(results.map((r) => r.label)).toEqual(["main.md", "main.rs"]);
  });

  it("drops a workspace hit that is already an open buffer", async () => {
    h.root = "/repo";
    h.activeTabs = [doc({ id: "a", title: "main.rs", source_path: "/repo/src/main.rs" })];
    h.searchFiles.mockResolvedValue([{ path: "src/main.rs", name: "main.rs", score: 9 }]);
    const results = await createFilesProvider().query("main", signal(), "all");
    expect(results).toHaveLength(1);
    expect(results[0].id).toBe("file:buffer:a");
  });

  it("drops a workspace hit that is a history buffer", async () => {
    h.root = "/repo";
    h.historyList = [
      doc({ id: "h", title: "main.rs", status: "history", source_path: "/repo/src/main.rs" }),
    ];
    h.searchFiles.mockResolvedValue([{ path: "src/main.rs", name: "main.rs", score: 9 }]);
    const results = await createFilesProvider().query("main", signal(), "all");
    expect(results).toHaveLength(1);
    expect(results[0].id).toBe("file:buffer:h");
  });

  it("dedupes across separator styles", async () => {
    h.root = "C:\\repo";
    h.activeTabs = [
      doc({ id: "a", title: "main.rs", source_path: "C:\\repo\\src\\main.rs" }),
    ];
    h.searchFiles.mockResolvedValue([{ path: "src/main.rs", name: "main.rs", score: 9 }]);
    const results = await createFilesProvider().query("main", signal(), "all");
    expect(results).toHaveLength(1);
  });

  it("activates an open tab", async () => {
    h.activeTabs = [doc({ id: "a", title: "Alpha" })];
    const results = await createFilesProvider().query("alpha", signal(), "all");
    results[0].execute();
    expect(h.setActiveTabId).toHaveBeenCalledWith("a");
  });

  it("restores a history entry", async () => {
    h.historyList = [doc({ id: "b", title: "Bravo", status: "history" })];
    const results = await createFilesProvider().query("bravo", signal(), "all");
    results[0].execute();
    expect(h.restoreFromHistory).toHaveBeenCalledWith("b");
  });

  it("opens a workspace file by absolute path", async () => {
    h.root = "/repo";
    h.searchFiles.mockResolvedValue([{ path: "src/main.rs", name: "main.rs", score: 9 }]);
    const results = await createFilesProvider().query("main", signal(), "all");
    results[0].execute();
    expect(h.openFile).toHaveBeenCalledWith("/repo/src/main.rs");
  });
});

describe("content provider", () => {
  it("returns buffer hits with their line and snippet", async () => {
    h.activeTabs = [doc({ id: "a", title: "Alpha" })];
    h.searchBuffers.mockResolvedValue({
      hits: [
        { buffer_id: "a", title: "Alpha", line: 12, snippet: [{ text: "todo", matched: true }] },
      ],
      total: 1,
    });
    const results = await createContentProvider().query("todo", signal(), "all");
    expect(results).toHaveLength(1);
    expect(results[0].line).toBe(12);
    expect(results[0].snippet).toEqual([{ text: "todo", matched: true }]);
  });

  it("reveals the line of an open buffer hit", async () => {
    h.activeTabs = [doc({ id: "a", title: "Alpha" })];
    h.searchBuffers.mockResolvedValue({
      hits: [{ buffer_id: "a", title: "Alpha", line: 12, snippet: [] }],
      total: 1,
    });
    const results = await createContentProvider().query("todo", signal(), "all");
    results[0].execute();
    expect(h.setActiveTabId).toHaveBeenCalledWith("a");
    expect(h.requestReveal).toHaveBeenCalledWith("a", 12);
  });

  it("restores then reveals a history hit", async () => {
    h.historyList = [doc({ id: "b", title: "Bravo", status: "history" })];
    h.searchBuffers.mockResolvedValue({
      hits: [{ buffer_id: "b", title: "Bravo", line: 4, snippet: [] }],
      total: 1,
    });
    const results = await createContentProvider().query("todo", signal(), "all");
    results[0].execute();
    await Promise.resolve();
    expect(h.restoreFromHistory).toHaveBeenCalledWith("b");
    expect(h.requestReveal).toHaveBeenCalledWith("b", 4);
  });

  it("ignores buffer hits for buffers that are neither open nor in history", async () => {
    h.searchBuffers.mockResolvedValue({
      hits: [{ buffer_id: "gone", title: "Gone", line: 1, snippet: [] }],
      total: 1,
    });
    expect(await createContentProvider().query("todo", signal(), "all")).toEqual([]);
  });

  it("opens a note hit that has no tab by its path", async () => {
    h.searchBuffers.mockResolvedValue({
      hits: [
        {
          buffer_id: "",
          title: "Notes",
          line: 3,
          snippet: [],
          path: "/notes/notes.md",
        },
      ],
      total: 1,
    });
    const results = await createContentProvider().query("todo", signal(), "all");
    expect(results).toHaveLength(1);
    results[0].execute();
    await Promise.resolve();
    expect(h.openFile).toHaveBeenCalledWith("/notes/notes.md");
  });

  it("streams workspace hits and reveals the line on the opened buffer", async () => {
    h.root = "/repo";
    h.streamContent.mockImplementation(
      async (_q: string, onBatch: (b: { hits: unknown[]; outcome: null }) => void) => {
        onBatch({
          hits: [{ path: "src/main.rs", line: 7, snippet: [{ text: "todo", matched: true }] }],
          outcome: null,
        });
      },
    );
    const batches: unknown[][] = [];
    await createContentProvider().stream!("todo", (rows) => batches.push(rows), signal());
    expect(batches).toHaveLength(1);
    const row = batches[0][0] as { label: string; line: number; execute: () => void };
    expect(row.label).toBe("main.rs");
    expect(row.line).toBe(7);
    row.execute();
    await Promise.resolve();
    expect(h.openFile).toHaveBeenCalledWith("/repo/src/main.rs");
    expect(h.requestReveal).toHaveBeenCalledWith("opened", 7);
  });

  it("drops a streamed hit for a file that is an open buffer", async () => {
    h.root = "/repo";
    h.activeTabs = [doc({ id: "a", title: "main.rs", source_path: "/repo/src/main.rs" })];
    h.streamContent.mockImplementation(
      async (_q: string, onBatch: (b: { hits: unknown[]; outcome: null }) => void) => {
        onBatch({
          hits: [
            { path: "src/main.rs", line: 7, snippet: [] },
            { path: "src/other.rs", line: 3, snippet: [] },
          ],
          outcome: null,
        });
      },
    );
    const batches: { label: string }[][] = [];
    await createContentProvider().stream!(
      "todo",
      (rows) => batches.push(rows as { label: string }[]),
      signal(),
    );
    expect(batches[0].map((r) => r.label)).toEqual(["other.rs"]);
  });

  it("does not grep without a workspace root", async () => {
    await createContentProvider().stream!("todo", vi.fn(), signal());
    expect(h.streamContent).not.toHaveBeenCalled();
  });
});

describe("go to line provider", () => {
  it("offers the jump for a numeric query in the active buffer", () => {
    h.activeTabId = "a";
    h.activeTabs = [doc({ id: "a", title: "Alpha" })];
    const results = createGotoLineProvider().query("42", signal(), "all") as { label: string; execute: () => void }[];
    expect(results[0].label).toBe("Go to line 42");
    results[0].execute();
    expect(h.requestReveal).toHaveBeenCalledWith("a", 42);
  });

  it("contributes nothing for a non-numeric query", () => {
    h.activeTabId = "a";
    expect(createGotoLineProvider().query("abc", signal(), "all")).toEqual([]);
  });

  it("contributes nothing without an active buffer", () => {
    expect(createGotoLineProvider().query("42", signal(), "all")).toEqual([]);
  });

  it("rejects line zero", () => {
    h.activeTabId = "a";
    expect(createGotoLineProvider().query("0", signal(), "all")).toEqual([]);
  });
});
