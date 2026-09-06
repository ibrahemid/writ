import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";

// The panel beside the note. Every section is a view of one read of one note,
// a section with nothing in it renders nothing at all, and nothing here edits
// what it shows.

const h = vi.hoisted(() => ({
  panel: { open: true, width: 240 },
  isOpen: true,
  collapsed: new Set<string>(),
  toggleSection: vi.fn<(section: string) => void>(),
  setWidth: vi.fn<(width: number) => void>(),
  setPanelWidth: vi.fn<(width: number) => void>(),
  requestReveal: vi.fn<(bufferId: string, line: number) => void>(),
  openFile: vi.fn<(path: string) => Promise<{ id: string } | null>>(),
  activeTabId: "buf-1" as string | null,
  tabs: [{ id: "buf-1", source_path: "/notes/Open.md" }] as {
    id: string;
    source_path: string | null;
  }[],
  backlinks: [] as unknown[],
  graph: { nodes: [], edges: [] } as {
    nodes: { path: string; name: string; folder: string }[];
    edges: { from_path: string; to_path: string; count: number }[];
  },
  facts: { links: [], properties: [], tags: [], headings: [] } as {
    links: unknown[];
    properties: { key: string; value_json: string }[];
    tags: unknown[];
    headings: { level: number; text: string; line: number; slug: string }[];
  },
}));

vi.mock("../../stores/global/config", async () => {
  const actual =
    await vi.importActual<typeof import("../../stores/global/config")>(
      "../../stores/global/config",
    );
  return {
    ...actual,
    configStore: { config: () => ({ panel: h.panel }), setPanelWidth: h.setPanelWidth },
  };
});

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    rightPanel: {
      isOpen: () => h.isOpen,
      width: () => h.panel.width,
      setWidth: h.setWidth,
      isCollapsed: (section: string) => h.collapsed.has(section),
      toggleSection: h.toggleSection,
    },
    tabs: { activeTabId: () => h.activeTabId, openFile: h.openFile },
    editor: { requestReveal: h.requestReveal },
  }),
}));

vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: { activeTabs: () => h.tabs },
}));

vi.mock("../../stores/global/backlinks", () => ({
  backlinksStore: { backlinksFor: () => () => h.backlinks, release: vi.fn() },
}));

vi.mock("../../stores/global/note-facts", () => ({
  noteFactsStore: {
    factsFor: () => () => h.facts,
    graph: () => () => h.graph,
    release: vi.fn(),
  },
}));

import RightPanel from "../../components/RightPanel/RightPanel";

function mount() {
  const { container } = render(() => <RightPanel />);
  return {
    container,
    panel: container.querySelector<HTMLElement>(".right-panel")!,
    handle: container.querySelector<HTMLElement>(".right-panel-resizer")!,
  };
}

function headings(container: HTMLElement): string[] {
  return [...container.querySelectorAll("h2")].map((el) => el.textContent?.trim() ?? "");
}

beforeEach(() => {
  h.panel = { open: true, width: 240 };
  h.isOpen = true;
  h.collapsed = new Set();
  h.activeTabId = "buf-1";
  h.tabs = [{ id: "buf-1", source_path: "/notes/Open.md" }];
  h.backlinks = [];
  h.facts = { links: [], properties: [], tags: [], headings: [] };
  h.toggleSection.mockClear();
  h.setWidth.mockClear();
  h.requestReveal.mockClear();
  h.openFile.mockReset().mockResolvedValue({ id: "buf-2" });
});

afterEach(cleanup);

describe("a note with nothing to show", () => {
  it("renders a panel with no section heading in it", () => {
    const { container, panel } = mount();
    expect(panel).not.toBeNull();
    expect(headings(container)).toEqual([]);
    expect(container.querySelector(".right-panel-section")).toBeNull();
    expect(container.textContent?.trim()).toBe("");
  });

  it("shows nothing when no note is open", () => {
    h.activeTabId = null;
    const { container } = mount();
    expect(headings(container)).toEqual([]);
  });

  it("shows nothing for a note that has never been written to a file", () => {
    h.tabs = [{ id: "buf-1", source_path: null }];
    const { container } = mount();
    expect(headings(container)).toEqual([]);
  });
});

describe("the panel is a named landmark", () => {
  it("carries the concept's public name and is out of the tree when closed", () => {
    const { panel } = mount();
    expect(panel.tagName).toBe("ASIDE");
    expect(panel.getAttribute("aria-label")).toBe("Connections");
    expect(panel.classList.contains("is-open")).toBe(true);
    expect(panel.getAttribute("aria-hidden")).toBeNull();
    // inert is presence-based: an open panel carrying inert="false" would be
    // unclickable, and a closed one without it keeps its rows in the tab order.
    expect(panel.hasAttribute("inert")).toBe(false);

    cleanup();
    h.isOpen = false;
    const closed = mount();
    expect(closed.panel.getAttribute("aria-hidden")).toBe("true");
    expect(closed.panel.hasAttribute("inert")).toBe(true);
    expect(closed.panel.classList.contains("is-open")).toBe(false);
  });
});

describe("the outline", () => {
  beforeEach(() => {
    h.facts = {
      links: [],
      properties: [],
      tags: [],
      headings: [
        { level: 1, text: "Title", line: 1, slug: "title" },
        { level: 2, text: "Part", line: 7, slug: "part" },
        { level: 3, text: "Detail", line: 12, slug: "detail" },
      ],
    };
  });

  it("indents one level per heading level", () => {
    const { container } = mount();
    const rows = [...container.querySelectorAll<HTMLElement>(".right-panel-heading")];
    expect(rows.map((row) => row.textContent)).toEqual(["Title", "Part", "Detail"]);
    expect(rows.map((row) => row.style.paddingLeft)).toEqual([
      "calc(var(--writ-space-3) + 0px)",
      "calc(var(--writ-space-3) + 16px)",
      "calc(var(--writ-space-3) + 32px)",
    ]);
  });

  it("moves the caret to the heading's line through the editor store", () => {
    const { container } = mount();
    const rows = container.querySelectorAll<HTMLElement>(".right-panel-heading");
    fireEvent.click(rows[1]);
    expect(h.requestReveal).toHaveBeenCalledWith("buf-1", 7);
    fireEvent.click(rows[2]);
    expect(h.requestReveal).toHaveBeenCalledWith("buf-1", 12);
  });

  // The click path is the contract: the store owns the caret, so the section
  // reaches neither the document nor CodeMirror to move it.
  it("reaches the editor through the store and never through the document", () => {
    const source = readFileSync(
      resolve(process.cwd(), "src/components/RightPanel/OutlineSection.tsx"),
      "utf8",
    );
    expect(source).not.toContain("querySelector");
    expect(source).not.toContain("codemirror");
    expect(source).toContain("win.editor.requestReveal");
  });
});

describe("the properties", () => {
  beforeEach(() => {
    h.facts = {
      links: [],
      properties: [
        { key: "status", value_json: '"draft"' },
        { key: "tags", value_json: '["one","two"]' },
        { key: "rating", value_json: "4" },
      ],
      tags: [],
      headings: [],
    };
  });

  it("renders scalars as text and lists as pills", () => {
    const { container } = mount();
    expect(container.querySelector(".right-panel-properties")).not.toBeNull();
    const keys = [...container.querySelectorAll(".right-panel-property-key")].map(
      (el) => el.textContent,
    );
    expect(keys).toEqual(["status", "tags", "rating"]);
    const pills = [...container.querySelectorAll(".right-panel-pill")].map(
      (el) => el.textContent,
    );
    expect(pills).toEqual(["one", "two"]);
    const texts = [...container.querySelectorAll(".right-panel-property-text")].map(
      (el) => el.textContent,
    );
    expect(texts).toEqual(["draft", "4"]);
  });

  it("renders nothing that takes a value", () => {
    const { container } = mount();
    expect(container.querySelector("input")).toBeNull();
    expect(container.querySelector("textarea")).toBeNull();
    expect(container.querySelector("select")).toBeNull();
    expect(container.querySelector("[contenteditable]")).toBeNull();
  });
});

describe("backlinks", () => {
  it("marks a link that could name another note, and leaves a settled one bare", () => {
    h.backlinks = [
      {
        from_path: "/notes/One.md",
        from_name: "One",
        to_target: "Open",
        alias: null,
        kind: "wikilink",
        line: 3,
        col: 0,
        context: "see Open for the rest",
        certainty: "resolved",
        candidates: [],
      },
      {
        from_path: "/notes/Two.md",
        from_name: "Two",
        to_target: "Open",
        alias: null,
        kind: "wikilink",
        line: 9,
        col: 4,
        context: "also Open",
        certainty: "ambiguous",
        candidates: ["/notes/archive/Open.md"],
      },
    ];
    const { container } = mount();
    expect(headings(container)).toEqual(["Links to this note"]);
    const markers = [...container.querySelectorAll(".right-panel-row-marker")].map(
      (el) => el.textContent,
    );
    expect(markers).toEqual(["Could also mean archive/Open"]);
    const names = [...container.querySelectorAll(".right-panel-row-name")].map(
      (el) => el.textContent,
    );
    expect(names).toEqual(["One", "Two"]);
  });

  it("names every note an ambiguous link could mean instead", () => {
    h.backlinks = [
      {
        from_path: "/notes/One.md",
        from_name: "One",
        to_target: "Open",
        alias: null,
        kind: "wikilink",
        line: 3,
        col: 0,
        context: "see Open",
        certainty: "ambiguous",
        candidates: ["/notes/archive/Open.md", "/notes/team/Open.md"],
      },
    ];
    const { container } = mount();
    expect(container.querySelector(".right-panel-row-marker")?.textContent?.trim()).toBe(
      "Could also mean archive/Open or team/Open",
    );
  });

  it("opens the linking note at the line the link is on", async () => {
    h.backlinks = [
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
        candidates: [],
      },
    ];
    const { container } = mount();
    fireEvent.click(container.querySelector<HTMLElement>(".right-panel-row")!);
    await Promise.resolve();
    await Promise.resolve();
    expect(h.openFile).toHaveBeenCalledWith("/notes/One.md");
    expect(h.requestReveal).toHaveBeenCalledWith("buf-2", 3);
  });
});

describe("sections are headed and reachable", () => {
  beforeEach(() => {
    h.facts = {
      links: [],
      properties: [{ key: "status", value_json: '"draft"' }],
      tags: [],
      headings: [{ level: 1, text: "Title", line: 1, slug: "title" }],
    };
  });

  it("names each section under a heading that is also its disclosure", () => {
    const { container } = mount();
    expect(headings(container)).toEqual(["Outline", "Properties"]);
    const toggles = [...container.querySelectorAll<HTMLElement>(".right-panel-section-toggle")];
    expect(toggles.map((el) => el.getAttribute("aria-expanded"))).toEqual(["true", "true"]);
    for (const section of container.querySelectorAll("section.right-panel-section")) {
      const labelled = section.getAttribute("aria-labelledby");
      expect(labelled).toBeTruthy();
      expect(container.querySelector(`#${labelled}`)?.tagName).toBe("H2");
    }
  });

  it("folds a section away without unheading it", () => {
    h.collapsed = new Set(["outline"]);
    const { container } = mount();
    expect(headings(container)).toEqual(["Outline", "Properties"]);
    expect(container.querySelector(".right-panel-heading")).toBeNull();
    const toggles = [...container.querySelectorAll<HTMLElement>(".right-panel-section-toggle")];
    expect(toggles[0].getAttribute("aria-expanded")).toBe("false");
    fireEvent.click(toggles[0]);
    expect(h.toggleSection).toHaveBeenCalledWith("outline");
  });
});

describe("the panel's edge", () => {
  it("is a separator reporting the panel's own range", () => {
    const { handle } = mount();
    expect(handle.getAttribute("role")).toBe("separator");
    expect(handle.getAttribute("aria-label")).toBe("Connections width");
    expect(handle.getAttribute("aria-valuemin")).toBe("200");
    expect(handle.getAttribute("aria-valuemax")).toBe("320");
    expect(handle.getAttribute("aria-valuenow")).toBe("240");
    expect(handle.tabIndex).toBe(0);
  });

  it("widens as the pointer moves towards the sidebar and commits on release", () => {
    const { panel, handle } = mount();
    fireEvent.pointerDown(handle, { button: 0, pointerId: 1, clientX: 800 });
    fireEvent.pointerMove(handle, { pointerId: 1, clientX: 760 });
    expect(panel.style.getPropertyValue("--writ-panel-live-width")).toBe("280px");
    fireEvent.pointerUp(handle, { pointerId: 1, clientX: 760 });
    expect(h.setWidth).toHaveBeenCalledWith(280);
  });

  it("clamps the drag at 200 and at 320", () => {
    const { panel, handle } = mount();
    fireEvent.pointerDown(handle, { button: 0, pointerId: 1, clientX: 800 });
    fireEvent.pointerMove(handle, { pointerId: 1, clientX: 400 });
    expect(panel.style.getPropertyValue("--writ-panel-live-width")).toBe("320px");
    fireEvent.pointerMove(handle, { pointerId: 1, clientX: 1200 });
    expect(panel.style.getPropertyValue("--writ-panel-live-width")).toBe("200px");
  });

  it("steps with the arrow keys, left widening the panel", () => {
    const { handle } = mount();
    fireEvent.keyDown(handle, { key: "ArrowLeft" });
    expect(h.setWidth).toHaveBeenCalledWith(248);
    fireEvent.keyDown(handle, { key: "ArrowRight" });
    expect(h.setWidth).toHaveBeenCalledWith(232);
  });

  it("opens at the width the last session left, across a remount", () => {
    h.panel = { open: true, width: 305 };
    const first = mount();
    expect(first.panel.style.getPropertyValue("--writ-panel-live-width")).toBe("305px");
    cleanup();
    const second = mount();
    expect(second.panel.style.getPropertyValue("--writ-panel-live-width")).toBe("305px");
    expect(second.handle.getAttribute("aria-valuenow")).toBe("305");
  });
});

// The toggle is one command: the toolbar control, the palette row and the
// chord all run it, so the palette entry is the registration rather than a
// second table to keep in step.
describe("the toggle command", () => {
  const APP_TSX = readFileSync(resolve(process.cwd(), "src/App.tsx"), "utf8");

  it("is registered once, named for the concept, and on the sidebar's partner chord", () => {
    expect(APP_TSX).toContain('id: "panel.toggle"');
    expect(APP_TSX).toContain('label: "Toggle connections"');
    expect(APP_TSX).toContain('keybinding: "CmdOrCtrl+Shift+\\\\"');
    expect(APP_TSX).toContain("rightPanel.toggle()");
    // Cmd+\ stays the sidebar's.
    expect(APP_TSX).toContain('id: "sidebar.toggle"');
    expect(APP_TSX).toContain('keybinding: "CmdOrCtrl+\\\\"');
  });

  it("mounts the panel after the editor, inside the window body", () => {
    const body = APP_TSX.slice(APP_TSX.indexOf('<div class="app-body">'));
    expect(body.indexOf("<EditorArea />")).toBeLessThan(body.indexOf("<RightPanel />"));
    expect(body.indexOf("<RightPanel />")).toBeGreaterThan(-1);
  });
});
