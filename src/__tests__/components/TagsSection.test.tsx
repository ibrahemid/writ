import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";

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

function rowLabels(container: HTMLElement): string[] {
  return Array.from(container.querySelectorAll(".tags-row")).map((row) =>
    (row.querySelector(".tags-row-name") as HTMLElement).textContent!.trim(),
  );
}

function countFor(container: HTMLElement, tag: string): string {
  const row = Array.from(container.querySelectorAll<HTMLElement>(".tags-row")).find(
    (candidate) => candidate.querySelector(".tags-row-name")!.textContent === tag,
  );
  expect(row, `no row for ${tag}`).toBeDefined();
  return row!.querySelector(".tags-row-count")!.textContent!;
}

let sidebar: SidebarStore;

beforeEach(async () => {
  await noteFactsStore.reset();
  vi.clearAllMocks();
  mockedEvents.onEvent.mockResolvedValue(() => {});
  sidebar = createSidebarStore();
  h.sidebar = sidebar;
});

afterEach(() => {
  cleanup();
});

describe("the tags section", () => {
  it("is absent from the tree when the folder has no tags", async () => {
    const { container } = await mount([]);
    expect(container.querySelector(".tags-section")).toBeNull();
    expect(container.textContent).toBe("");
  });

  it("names the section Tags and lists a tag with the notes carrying it", async () => {
    const { container } = await mount(tags(["idea", 3], ["draft", 1]));
    expect(container.querySelector(".sidebar-section-title")!.textContent).toBe("Tags");
    expect(rowLabels(container)).toEqual(["idea", "draft"]);
    expect(countFor(container, "idea")).toBe("3");
    expect(container.querySelector(".tags-row-hash")!.textContent).toBe("#");
  });

  it("counts a tag used twice in one note once", async () => {
    // What `NotesIndex::all_tags` answers: `COUNT(DISTINCT path)`, pinned in
    // `the_tag_list_counts_notes_not_mentions`. One note writing `#idea` twice
    // is one note carrying `#idea`.
    const { container } = await mount(tags(["idea", 1]));
    expect(countFor(container, "idea")).toBe("1");
  });

  it("groups project/alpha and project/beta under one project parent with the subtotal", async () => {
    const { container } = await mount(tags(["project/alpha", 3], ["project/beta", 2], ["idea", 1]));
    const groups = container.querySelectorAll(".tags-group");
    expect(groups).toHaveLength(1);
    const head = groups[0].querySelector(".tags-group-head")!;
    expect(head.querySelector(".tags-group-name")!.textContent).toBe("project");
    expect(head.querySelector(".tags-group-count")!.textContent).toBe("5");
    expect(rowLabels(groups[0] as HTMLElement)).toEqual(["project/alpha", "project/beta"]);
    // The nested tag is one tag, never a parent row and a child row.
    expect(rowLabels(container)).toEqual(["project/alpha", "project/beta", "idea"]);
  });

  it("lists the parent tag's own uses beside the group's subtotal", async () => {
    const { container } = await mount(tags(["project/alpha", 3], ["project", 1]));
    const group = container.querySelector(".tags-group")!;
    expect(group.querySelector(".tags-group-count")!.textContent).toBe("4");
    expect(rowLabels(group as HTMLElement)).toEqual(["project", "project/alpha"]);
    expect(countFor(container, "project")).toBe("1");
  });

  it("keeps a first segment carrying one tag as a row rather than a group", async () => {
    const { container } = await mount(tags(["project/alpha", 3]));
    expect(container.querySelector(".tags-group")).toBeNull();
    expect(rowLabels(container)).toEqual(["project/alpha"]);
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

  it("marks the tag it selects, and clears it when the same tag is picked again", async () => {
    const { container } = await mount(tags(["idea", 3], ["draft", 1]));
    const [idea, draft] = Array.from(container.querySelectorAll<HTMLButtonElement>(".tags-row"));

    idea.click();
    expect(sidebar.selectedTag()).toBe("idea");
    expect(idea.getAttribute("aria-pressed")).toBe("true");
    expect(draft.getAttribute("aria-pressed")).toBe("false");
    expect(idea.classList.contains("is-selected")).toBe(true);

    draft.click();
    expect(sidebar.selectedTag()).toBe("draft");
    expect(idea.getAttribute("aria-pressed")).toBe("false");

    draft.click();
    expect(sidebar.selectedTag()).toBeNull();
    expect(draft.getAttribute("aria-pressed")).toBe("false");
    expect(container.querySelector(".tags-row.is-selected")).toBeNull();
  });

  it("gives every tag a control the keyboard reaches", async () => {
    const { container } = await mount(tags(["project/alpha", 2], ["project/beta", 1]));
    const rows = Array.from(container.querySelectorAll(".tags-row"));
    expect(rows).toHaveLength(2);
    for (const row of rows) {
      expect(row.tagName).toBe("BUTTON");
      expect(row.getAttribute("type")).toBe("button");
    }
    // The group head is a label, so it is not in the tab order.
    expect(container.querySelector(".tags-group-head button")).toBeNull();
  });
});

const TAGS_CSS = readFileSync(resolve(process.cwd(), "src/components/Sidebar/TagsSection.css"), "utf8");

describe("the tag row on the design baseline", () => {
  it("is a pill on the row fill, with the count muted and small", () => {
    expect(TAGS_CSS).toMatch(/\.tags-row\s*\{[^}]*height:\s*var\(--writ-sidebar-row-fill\)/);
    expect(TAGS_CSS).toMatch(/\.tags-row\s*\{[^}]*border-radius:\s*var\(--writ-r-pill\)/);
    expect(TAGS_CSS).toMatch(/\.tags-row\s*\{[^}]*margin:\s*1px 6px/);
    expect(TAGS_CSS).toMatch(
      /\.tags-row-count\s*\{[^}]*color:\s*var\(--writ-fg-muted\)[^}]*font-size:\s*var\(--writ-ui-sm\)/,
    );
  });

  it("spends the accent on the selected row's hash and nowhere else", () => {
    expect(TAGS_CSS).toMatch(
      /\.tags-row\.is-selected\s*\{[^}]*background:\s*var\(--writ-bg-selected\)/,
    );
    expect(TAGS_CSS).toMatch(
      /\.tags-row\.is-selected \.tags-row-hash\s*\{\s*color:\s*var\(--writ-accent\)/,
    );
    const rowRule = /\.tags-row\s*\{[^}]*\}/.exec(TAGS_CSS)![0];
    expect(rowRule).not.toContain("--writ-accent");
  });

  it("stretches a nested row to the sidebar edge so its count lands there", () => {
    expect(TAGS_CSS).toMatch(
      /\.tags-group\s*\{[^}]*display:\s*flex[^}]*flex-direction:\s*column/,
    );
    expect(TAGS_CSS).toMatch(/\.tags-list\s*\{[^}]*flex-direction:\s*column/);
  });

  it("indents a nested tag 16px past the parent label and draws the ring at -2px", () => {
    expect(TAGS_CSS).toMatch(/\.tags-group \.tags-row\s*\{\s*padding-left:\s*26px/);
    expect(TAGS_CSS).toMatch(/\.tags-row:focus-visible\s*\{[^}]*outline-offset:\s*-2px/);
  });

  it("names no colour of its own", () => {
    expect(TAGS_CSS).not.toMatch(/#[0-9a-f]{3,8}\b/i);
    expect(TAGS_CSS).not.toMatch(/\b(rgba?|hsla?)\(/);
  });
});
