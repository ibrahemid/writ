import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";

// The panel follows Writ's own writes. The watcher drops them, so a save, a
// rename, a create or a delete reaches the panel as `notes:indexed`, sent once
// the index holds the new rows; the panel reads its note again on it.

const NOTE = "/notes/Open.md";

const h = vi.hoisted(() => ({
  requestReveal: vi.fn(),
  openFile: vi.fn(),
  handlers: new Map<string, ((payload: unknown) => void)[]>(),
}));

vi.mock("../../services/tauri", () => ({
  noteFacts: vi.fn(),
  noteAllTags: vi.fn(),
  noteGraph: vi.fn(),
  noteBacklinks: vi.fn(),
  resolveNoteLink: vi.fn(),
}));

vi.mock("../../services/events", () => ({
  onEvent: vi.fn(async (kind: string, handler: (payload: unknown) => void) => {
    const list = h.handlers.get(kind) ?? [];
    list.push(handler);
    h.handlers.set(kind, list);
    return () => {};
  }),
}));

vi.mock("../../stores/global/config", async () => {
  const actual =
    await vi.importActual<typeof import("../../stores/global/config")>(
      "../../stores/global/config",
    );
  return {
    ...actual,
    configStore: {
      config: () => ({ panel: { open: true, width: 240 } }),
      setPanelWidth: vi.fn(),
      isAppOn: () => true,
    },
  };
});

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    rightPanel: {
      isOpen: () => true,
      width: () => 240,
      setWidth: vi.fn(),
      isCollapsed: () => false,
      toggleSection: vi.fn(),
    },
    tabs: { activeTabId: () => "buf-1", openFile: h.openFile },
    editor: { requestReveal: h.requestReveal, cursorLine: () => 1 },
  }),
}));

vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: { activeTabs: () => [{ id: "buf-1", source_path: NOTE }] },
}));

vi.mock("../../components/Graph/GraphCanvas", () => ({
  default: (props: { nodes: { path: string; name: string }[] }) => (
    <p class="drawn">{props.nodes.map((node) => node.name).join(", ")}</p>
  ),
}));

import RightPanel from "../../components/RightPanel/RightPanel";
import { indexCacheStore } from "../../stores/global/index-cache";
import * as api from "../../services/tauri";
import type { Backlink } from "../../services/tauri";

const mockedApi = vi.mocked(api);

function backlink(from: string): Backlink {
  return {
    from_path: from,
    from_name: from.slice(from.lastIndexOf("/") + 1, -3),
    to_target: "Open",
    alias: null,
    kind: "wikilink",
    line: 3,
    col: 0,
    context: "",
    certainty: "resolved",
    candidates: [],
  };
}

function heading(text: string, line: number) {
  return { level: 2, text, line, slug: text.toLowerCase() };
}

/** Delivers `notes:indexed` the way the bridge does after an index write. */
function indexed(paths: string[]): void {
  const listeners = h.handlers.get("notes:indexed") ?? [];
  expect(listeners.length, "nothing on the panel listens for notes:indexed").toBeGreaterThan(0);
  for (const handler of listeners) handler({ paths });
}

async function settle(): Promise<void> {
  for (let i = 0; i < 6; i += 1) await Promise.resolve();
}

beforeEach(async () => {
  await indexCacheStore.reset();
  vi.clearAllMocks();
  h.handlers = new Map();
  mockedApi.noteFacts.mockResolvedValue({
    links: [],
    properties: [],
    tags: [],
    headings: [heading("Title", 1)],
  });
  mockedApi.noteGraph.mockResolvedValue({ nodes: [], edges: [] });
  mockedApi.noteBacklinks.mockResolvedValue([backlink("/notes/One.md")]);
});

afterEach(cleanup);

describe("the panel after Writ's own writes", () => {
  it("shows a heading a save added once the save's index event arrives", async () => {
    const { container } = render(() => <RightPanel />);
    await settle();
    expect(container.textContent).not.toContain("Plan");

    mockedApi.noteFacts.mockResolvedValue({
      links: [],
      properties: [],
      tags: [],
      headings: [heading("Title", 1), heading("Plan", 3)],
    });
    indexed([NOTE]);
    await settle();

    expect(container.textContent).toContain("Plan");
  });

  it("re-reads the backlinks and the graph after a rename's index event", async () => {
    const { container } = render(() => <RightPanel />);
    await settle();
    mockedApi.noteBacklinks.mockClear();
    mockedApi.noteGraph.mockClear();

    mockedApi.noteBacklinks.mockResolvedValue([
      backlink("/notes/One.md"),
      backlink("/notes/Renamed.md"),
    ]);
    indexed(["/notes/Two.md", "/notes/Renamed.md"]);
    await settle();

    expect(mockedApi.noteBacklinks).toHaveBeenCalledTimes(1);
    expect(mockedApi.noteGraph).toHaveBeenCalledTimes(1);
    expect(container.textContent).toContain("Renamed");
  });
});
