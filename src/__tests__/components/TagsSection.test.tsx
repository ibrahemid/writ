import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { createRoot } from "solid-js";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";

vi.mock("../../services/tauri", () => ({
  noteFacts: vi.fn(),
  noteAllTags: vi.fn(),
  noteGraph: vi.fn(),
  searchBuffers: vi.fn().mockResolvedValue({ hits: [], total: 0 }),
  getConfig: vi.fn(),
  updateConfig: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../services/autosave", () => ({
  flushAutosave: vi.fn().mockResolvedValue({ ok: true, failures: [] }),
  onAutosaveStart: vi.fn(() => () => {}),
  onAutosaveSuccess: vi.fn(() => () => {}),
  onAutosaveError: vi.fn(() => () => {}),
}));

vi.mock("../../services/events", () => ({
  onEvent: vi.fn().mockResolvedValue(() => {}),
}));

const h = vi.hoisted(() => ({ sidebar: null as unknown }));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({ sidebar: h.sidebar }),
}));

import TagsSection from "../../components/Sidebar/TagsSection";
import { createSidebarStore, type SidebarStore } from "../../stores/window/sidebar-store";
import { noteFactsStore, type TagCount } from "../../stores/global/note-facts";
import * as api from "../../services/tauri";
import * as events from "../../services/events";

const mockedApi = vi.mocked(api);
const mockedEvents = vi.mocked(events);

/** The handler the store gave `onEvent`, so a test can deliver the event. */
function notesChanged(): (payload: { path: string; removed: boolean }) => void {
  const call = mockedEvents.onEvent.mock.calls.find(([kind]) => kind === "notes:changed");
  expect(call, "the tag list never subscribed to notes:changed").toBeDefined();
  return call![1] as (payload: { path: string; removed: boolean }) => void;
}

/** Lets the store's fire-and-forget reads settle. */
async function settle(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

function tags(...rows: [string, number][]): TagCount[] {
  return rows.map(([tag, count]) => ({ tag, count }));
}

async function mount(rows: TagCount[]) {
  mockedApi.noteAllTags.mockResolvedValue(rows);
  const view = render(() => <TagsSection />);
  await settle();
  return view;
}

function rows(container: HTMLElement): HTMLElement[] {
  return Array.from(container.querySelectorAll<HTMLElement>(".tags-row"));
}

function rowLabels(container: HTMLElement): string[] {
  return rows(container).map(
    (row) => (row.querySelector(".tags-row-name") as HTMLElement).textContent!.trim(),
  );
}

function rowNamed(container: HTMLElement, name: string): HTMLElement {
  const row = rows(container).find(
    (candidate) => candidate.querySelector(".tags-row-name")!.textContent === name,
  );
  expect(row, `no row named ${name}`).toBeDefined();
  return row!;
}

function countFor(container: HTMLElement, name: string): string | null {
  return rowNamed(container, name).querySelector(".tags-row-count")?.textContent ?? null;
}

let sidebar: SidebarStore;
// The store's effects outlive a test unless its root is disposed, and one left
// holding a selected tag would read the tag list again on the next reset.
let disposeSidebar: () => void = () => {};

beforeEach(async () => {
  await noteFactsStore.reset();
  vi.clearAllMocks();
  mockedEvents.onEvent.mockResolvedValue(() => {});
  disposeSidebar = createRoot((dispose) => {
    sidebar = createSidebarStore();
    return dispose;
  });
  h.sidebar = sidebar;
});

afterEach(() => {
  cleanup();
  disposeSidebar();
});

describe("the tags section", () => {
  it("is absent from the tree when the folder has no tags", async () => {
    const { container } = await mount([]);
    expect(container.querySelector(".tags-section")).toBeNull();
    expect(container.textContent).toBe("");
  });

  it("names the section Tags and lists a tag with the notes carrying it", async () => {
    const { container } = await mount(tags(["idea", 3], ["draft", 1]));
    expect(container.querySelector(".sidebar-section-heading")!.textContent).toBe("Tags");
    expect(rowLabels(container)).toEqual(["idea", "draft"]);
    expect(countFor(container, "idea")).toBe("3");
    expect(countFor(container, "draft")).toBe("1");
    expect(container.querySelector(".tags-row-hash")!.textContent).toBe("#");
  });

  it("counts a tag used twice in one note once", async () => {
    // What `NotesIndex::all_tags` answers: `COUNT(DISTINCT path)`, pinned in
    // `the_tag_list_counts_notes_not_mentions`. One note writing `#idea` twice
    // is one note carrying `#idea`.
    const { container } = await mount(tags(["idea", 1]));
    expect(countFor(container, "idea")).toBe("1");
  });

  it("lists project once, its children under it by their last segment", async () => {
    const { container } = await mount(
      tags(["project", 1], ["project/alpha", 1], ["project/beta", 1], ["idea", 1]),
    );
    expect(rowLabels(container)).toEqual(["project", "alpha", "beta", "idea"]);
    expect(container.textContent).not.toContain("project/");
    expect(countFor(container, "project")).toBe("1");
    expect(countFor(container, "alpha")).toBe("1");

    const project = rowNamed(container, "project");
    const group = project.nextElementSibling!;
    expect(group.getAttribute("role")).toBe("group");
    expect(rowLabels(group as HTMLElement)).toEqual(["alpha", "beta"]);
  });

  it("gives a parent nobody uses directly a row with no count", async () => {
    const { container } = await mount(tags(["project/alpha", 3], ["project/beta", 2]));
    expect(rowLabels(container)).toEqual(["project", "alpha", "beta"]);
    expect(rowNamed(container, "project").querySelector(".tags-row-count")).toBeNull();
    expect(countFor(container, "alpha")).toBe("3");
  });

  it("nests three levels, each a step deeper", async () => {
    const { container } = await mount(tags(["a/b/c", 1]));
    const [a, b, c] = rows(container);
    expect(a.getAttribute("aria-level")).toBe("1");
    expect(b.getAttribute("aria-level")).toBe("2");
    expect(c.getAttribute("aria-level")).toBe("3");
    expect(a.style.paddingLeft).toBe("10px");
    expect(b.style.paddingLeft).toBe("26px");
    expect(c.style.paddingLeft).toBe("42px");
  });

  it("selects the whole family from the parent and one tag from a child", async () => {
    const { container } = await mount(tags(["project", 1], ["project/alpha", 1]));
    const project = rowNamed(container, "project");
    const alpha = rowNamed(container, "alpha");

    fireEvent.click(project);
    expect(sidebar.selectedTag()).toBe("project");
    expect(project.getAttribute("aria-selected")).toBe("true");
    expect(alpha.getAttribute("aria-selected")).toBe("false");
    expect(project.classList.contains("is-selected")).toBe(true);

    fireEvent.click(alpha);
    expect(sidebar.selectedTag()).toBe("project/alpha");
    expect(project.getAttribute("aria-selected")).toBe("false");
    expect(alpha.getAttribute("aria-selected")).toBe("true");

    fireEvent.click(alpha);
    expect(sidebar.selectedTag()).toBeNull();
    expect(container.querySelector(".tags-row.is-selected")).toBeNull();
  });

  it("folds the children from the caret without selecting the parent", async () => {
    const { container } = await mount(tags(["project/alpha", 1], ["project/beta", 1]));
    const project = rowNamed(container, "project");
    expect(project.getAttribute("aria-expanded")).toBe("true");
    expect(rowNamed(container, "alpha").hasAttribute("aria-expanded")).toBe(false);

    fireEvent.click(project.querySelector(".tags-row-caret")!);
    expect(sidebar.selectedTag()).toBeNull();
    expect(project.getAttribute("aria-expanded")).toBe("false");
    expect(rowLabels(container)).toEqual(["project"]);

    fireEvent.click(project.querySelector(".tags-row-caret")!);
    expect(rowLabels(container)).toEqual(["project", "alpha", "beta"]);
  });

  it("folds and unfolds from the keyboard, walks rows with the arrows and selects on Enter", async () => {
    const { container } = await mount(tags(["project/alpha", 1], ["idea", 1]));
    const project = rowNamed(container, "project");
    project.focus();

    fireEvent.keyDown(project, { key: "ArrowLeft" });
    expect(rowLabels(container)).toEqual(["project", "idea"]);
    fireEvent.keyDown(project, { key: "ArrowRight" });
    expect(rowLabels(container)).toEqual(["project", "alpha", "idea"]);

    fireEvent.keyDown(project, { key: "ArrowDown" });
    const alpha = rowNamed(container, "alpha");
    expect(document.activeElement).toBe(alpha);
    fireEvent.keyDown(alpha, { key: "ArrowUp" });
    expect(document.activeElement).toBe(project);

    fireEvent.keyDown(alpha, { key: "Enter" });
    expect(sidebar.selectedTag()).toBe("project/alpha");
    fireEvent.keyDown(project, { key: " " });
    expect(sidebar.selectedTag()).toBe("project");
  });

  it("updates the counts on a notes:changed", async () => {
    const { container } = await mount(tags(["idea", 1]));
    expect(countFor(container, "idea")).toBe("1");

    mockedApi.noteAllTags.mockResolvedValue(tags(["idea", 4], ["draft", 2]));
    notesChanged()({ path: "/notes/One.md", removed: false });
    await settle();

    expect(countFor(container, "idea")).toBe("4");
    expect(rowLabels(container)).toEqual(["idea", "draft"]);
  });

  it("is one tree of rows the keyboard reaches", async () => {
    const { container } = await mount(tags(["project/alpha", 2], ["idea", 1]));
    const tree = container.querySelector(".tags-tree")!;
    expect(tree.getAttribute("role")).toBe("tree");
    expect(tree.getAttribute("aria-label")).toBe("Tags");
    const all = rows(container);
    expect(all).toHaveLength(3);
    for (const row of all) {
      expect(row.getAttribute("role")).toBe("treeitem");
      expect(row.tabIndex).toBe(0);
    }
  });
});

const TAGS_CSS = readFileSync(resolve(process.cwd(), "src/components/Sidebar/TagsSection.css"), "utf8");

describe("the tag row on the design baseline", () => {
  it("is a pill on the row fill, with the count muted and small", () => {
    expect(TAGS_CSS).toMatch(/\.tags-row\s*\{[^}]*height:\s*var\(--writ-sidebar-row-fill\)/);
    expect(TAGS_CSS).toMatch(/\.tags-row\s*\{[^}]*border-radius:\s*var\(--writ-r-pill\)/);
    expect(TAGS_CSS).toMatch(/\.tags-row\s*\{[^}]*margin:\s*1px 6px/);
    expect(TAGS_CSS).toMatch(/\.tags-row\s*\{[^}]*padding-right:\s*10px/);
    expect(TAGS_CSS).toMatch(
      /\.tags-row-count\s*\{[^}]*color:\s*var\(--writ-fg-muted\)[^}]*font-size:\s*var\(--writ-ui-sm\)/,
    );
  });

  it("spends the accent on the selected row's hash and the focus ring, nowhere else", () => {
    expect(TAGS_CSS).toMatch(
      /\.tags-row\.is-selected\s*\{[^}]*background:\s*var\(--writ-bg-selected\)/,
    );
    expect(TAGS_CSS).toMatch(
      /\.tags-row\.is-selected \.tags-row-hash\s*\{\s*color:\s*var\(--writ-accent\)/,
    );
    expect(TAGS_CSS).toMatch(
      /\.tags-row:focus-visible\s*\{[^}]*outline:\s*2px solid var\(--writ-accent\)[^}]*outline-offset:\s*-2px/,
    );
    const rowRule = /\.tags-row\s*\{[^}]*\}/.exec(TAGS_CSS)![0];
    expect(rowRule).not.toContain("--writ-accent");
  });

  it("holds the caret in a 12px slot like the folder tree", () => {
    expect(TAGS_CSS).toMatch(
      /\.tags-row-caret\s*\{[^}]*display:\s*inline-flex[^}]*flex:\s*none[^}]*width:\s*12px/,
    );
    expect(TAGS_CSS).not.toContain(".tags-group");
  });

  it("names no colour of its own", () => {
    expect(TAGS_CSS).not.toMatch(/#[0-9a-f]{3,8}\b/i);
    expect(TAGS_CSS).not.toMatch(/\b(rgba?|hsla?)\(/);
  });
});
