import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";

// Three sections, one read. The outline and the properties are two views of
// one `note_facts` answer, so a note changing on disk costs one call for the
// facts and one for the backlinks — not one per section.

const h = vi.hoisted(() => ({
  requestReveal: vi.fn(),
  openFile: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  noteFacts: vi.fn(),
  noteAllTags: vi.fn(),
  noteGraph: vi.fn(),
  noteBacklinks: vi.fn(),
}));

vi.mock("../../services/events", () => ({
  onEvent: vi.fn().mockResolvedValue(() => {}),
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
    editor: { requestReveal: h.requestReveal },
  }),
}));

vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: { activeTabs: () => [{ id: "buf-1", source_path: NOTE }] },
}));

const NOTE = "/notes/Open.md";

import RightPanel from "../../components/RightPanel/RightPanel";
import { noteFactsStore } from "../../stores/global/note-facts";
import { backlinksStore } from "../../stores/global/backlinks";
import * as api from "../../services/tauri";
import * as events from "../../services/events";

const mockedApi = vi.mocked(api);
const mockedEvents = vi.mocked(events);

/** Every handler the two stores gave `onEvent`, so the test can deliver one. */
function notesChangedHandlers(): ((payload: { path: string; removed: boolean }) => void)[] {
  return mockedEvents.onEvent.mock.calls
    .filter(([kind]) => kind === "notes:changed")
    .map(([, handler]) => handler as (payload: { path: string; removed: boolean }) => void);
}

async function settle(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

beforeEach(async () => {
  await noteFactsStore.reset();
  await backlinksStore.reset();
  vi.clearAllMocks();
  mockedEvents.onEvent.mockResolvedValue(() => {});
  mockedApi.noteFacts.mockResolvedValue({
    links: [],
    properties: [{ key: "status", value_json: '"draft"' }],
    tags: [],
    headings: [{ level: 1, text: "Title", line: 1, slug: "title" }],
  });
  mockedApi.noteBacklinks.mockResolvedValue([
    {
      from_path: "/notes/One.md",
      from_name: "One",
      to_target: "Open",
      alias: null,
      kind: "wikilink",
      line: 3,
      col: 0,
      context: "",
      certainty: "resolved",
    },
  ]);
});

afterEach(cleanup);

describe("the panel reads one note once", () => {
  it("asks for the facts once for three sections", async () => {
    render(() => <RightPanel />);
    await settle();
    expect(mockedApi.noteFacts).toHaveBeenCalledTimes(1);
    expect(mockedApi.noteFacts).toHaveBeenCalledWith(NOTE);
    expect(mockedApi.noteBacklinks).toHaveBeenCalledTimes(1);
  });

  it("re-reads once per section on one notes:changed, not once per section each", async () => {
    const { container } = render(() => <RightPanel />);
    await settle();
    expect(container.querySelectorAll("h2")).toHaveLength(3);

    mockedApi.noteFacts.mockClear();
    mockedApi.noteBacklinks.mockClear();
    for (const handler of notesChangedHandlers()) handler({ path: NOTE, removed: false });
    await settle();

    expect(mockedApi.noteFacts).toHaveBeenCalledTimes(1);
    expect(mockedApi.noteBacklinks).toHaveBeenCalledTimes(1);
  });
});
