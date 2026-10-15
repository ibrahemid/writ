import { describe, it, expect, vi, beforeEach } from "vitest";
import type { BufferDocument } from "../../types/buffer";

// The tab store with two panes (ADR-056): `activeTabId` is the focused pane's
// tab in front, a close in one pane of a tab the other still shows keeps it
// open, and a pane that loses its last tab collapses.

let tabIdCounter = 0;

function mockBuffer(overrides: Partial<BufferDocument> = {}): BufferDocument {
  tabIdCounter++;
  return {
    id: `buf-${tabIdCounter}`,
    title: `Buffer ${tabIdCounter}`,
    filename: `buf-${tabIdCounter}.md`,
    status: "active",
    language: null,
    source_path: null,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: tabIdCounter,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    closed_at: null,
    read_only: false,
    size_bytes: 0,
    line_ending: "lf",
    encoding: "UTF-8",
    ...overrides,
  };
}

vi.mock("../../stores/global/dialogs", () => ({
  requestConfirm: vi.fn(() => Promise.resolve(false)),
}));

vi.mock("../../services/tauri", () => ({
  createBuffer: vi.fn().mockImplementation(() => Promise.resolve(mockBuffer())),
  listActiveBuffers: vi.fn().mockResolvedValue([]),
  listHistory: vi.fn().mockResolvedValue([]),
  closeBuffer: vi.fn().mockResolvedValue(undefined),
  closeBuffers: vi.fn().mockResolvedValue(undefined),
  discardUnsavedText: vi.fn().mockResolvedValue(undefined),
  saveBufferContent: vi.fn().mockResolvedValue(undefined),
  previewClose: vi.fn().mockResolvedValue(undefined),
  recordUnsavedNotes: vi.fn().mockResolvedValue(undefined),
  deleteNote: vi.fn().mockResolvedValue(null),
}));

import { createTabStore } from "../../stores/window/tab-store";
import { createPaneStore } from "../../stores/window/pane-store";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { createDownloadStore } from "../../stores/window/download-store";
import { cancelAutosave } from "../../services/autosave";
import * as api from "../../services/tauri";

const mockedApi = vi.mocked(api);

function window() {
  const panes = createPaneStore();
  const tabs = createTabStore({ downloads: createDownloadStore(), registry: bufferRegistry, panes });
  return { panes, tabs };
}

function open(): string[] {
  return bufferRegistry.activeTabs().map((b) => b.id);
}

async function threeTabs() {
  const win = window();
  const a = await win.tabs.createTab();
  const b = await win.tabs.createTab();
  const c = await win.tabs.createTab();
  return { ...win, a: a.id, b: b.id, c: c.id };
}

beforeEach(async () => {
  vi.clearAllMocks();
  localStorage.clear();
  tabIdCounter = 0;
  mockedApi.listActiveBuffers.mockResolvedValue([]);
  mockedApi.listHistory.mockResolvedValue([]);
  for (const b of bufferRegistry.activeTabs()) cancelAutosave(b.id);
  await bufferRegistry.load();
});

describe("the tab in front", () => {
  it("is the focused pane's", async () => {
    const { tabs, panes, b, c } = await threeTabs();
    tabs.moveToOtherPane(c);
    expect(panes.focused()).toBe("right");
    expect(tabs.activeTabId()).toBe(c);
    tabs.focusOtherPane();
    expect(tabs.activeTabId()).toBe(b);
  });

  it("moves focus to the pane showing an open tab that is selected", async () => {
    const { tabs, panes, a, c } = await threeTabs();
    tabs.moveToOtherPane(c);
    tabs.setActiveTabId(a);
    expect(panes.focused()).toBe("left");
    expect(tabs.activeTabId()).toBe(a);
  });

  it("is a new tab in the focused pane when one is opened there", async () => {
    const { tabs, panes, c } = await threeTabs();
    tabs.moveToOtherPane(c);
    const d = await tabs.createTab();
    expect(panes.holds("right", d.id)).toBe(true);
    expect(panes.holds("left", d.id)).toBe(false);
    expect(tabs.activeTabId()).toBe(d.id);
  });

  it("is a tab opened outside the store, in the focused pane, when it is selected", async () => {
    const { tabs, panes, c } = await threeTabs();
    tabs.moveToOtherPane(c);
    const licenses = mockBuffer();
    bufferRegistry.registerOpenResult({ doc: licenses, mode: { kind: "Normal" }, size_bytes: 0 });

    tabs.setActiveTabId(licenses.id);

    expect(panes.focused()).toBe("right");
    expect(panes.holds("right", licenses.id)).toBe(true);
    expect(panes.holds("left", licenses.id)).toBe(false);
    expect(tabs.activeTabId()).toBe(licenses.id);
  });

  it("is a tab opened outside the store again after it closed, in the focused pane", async () => {
    const { tabs, panes, c } = await threeTabs();
    const licenses = mockBuffer();
    bufferRegistry.registerOpenResult({ doc: licenses, mode: { kind: "Normal" }, size_bytes: 0 });
    tabs.setActiveTabId(licenses.id);
    await tabs.closeTab(licenses.id);
    expect(open()).not.toContain(licenses.id);
    tabs.moveToOtherPane(c);

    bufferRegistry.registerOpenResult({ doc: licenses, mode: { kind: "Normal" }, size_bytes: 0 });
    tabs.setActiveTabId(licenses.id);

    expect(panes.focused()).toBe("right");
    expect(panes.holds("right", licenses.id)).toBe(true);
    expect(panes.holds("left", licenses.id)).toBe(false);
  });

  it("lists each pane's own tabs for the strip and the tab chords", async () => {
    const { tabs, a, b, c } = await threeTabs();
    tabs.moveToOtherPane(c);
    expect(tabs.tabsIn("left")).toEqual([a, b]);
    expect(tabs.tabsIn("right")).toEqual([c]);
  });
});

describe("closing a tab with two panes", () => {
  it("keeps a tab open that the other pane still shows", async () => {
    const { tabs, panes, c } = await threeTabs();
    tabs.splitRight();
    expect(panes.holds("left", c) && panes.holds("right", c)).toBe(true);

    await tabs.closeTab(c, "right");

    expect(open()).toContain(c);
    expect(mockedApi.closeBuffer).not.toHaveBeenCalled();
    expect(panes.isSplit()).toBe(false);
    expect(tabs.activeTabId()).toBe(c);
  });

  it("collapses the right pane on its last tab and closes the file", async () => {
    const { tabs, panes, b, c } = await threeTabs();
    tabs.moveToOtherPane(c);

    await tabs.closeTab(c);

    expect(open()).not.toContain(c);
    expect(mockedApi.closeBuffer).toHaveBeenCalledWith(c);
    expect(panes.isSplit()).toBe(false);
    expect(panes.focused()).toBe("left");
    expect(tabs.activeTabId()).toBe(b);
  });

  it("moves the right pane's tabs left when the left pane's last tab closes", async () => {
    const win = window();
    const a = (await win.tabs.createTab()).id;
    const c = (await win.tabs.createTab()).id;
    win.tabs.moveToOtherPane(c);
    win.tabs.setActiveTabId(a);

    await win.tabs.closeTab(a);

    expect(win.panes.isSplit()).toBe(false);
    expect(win.tabs.tabsIn("left")).toEqual([c]);
    expect(win.tabs.activeTabId()).toBe(c);
  });
});

describe("deleting a note both panes show", () => {
  it("moves every pane off it before it goes, never back onto it", async () => {
    const win = window();
    const x = (await win.tabs.createTab()).id;
    const y = (await win.tabs.createTab()).id;
    win.tabs.moveToOtherPane(y);
    win.tabs.setActiveTabId(x);
    win.tabs.splitRight();
    expect(win.tabs.tabsIn("left")).toEqual([x]);
    expect(win.tabs.tabsIn("right")).toEqual([x, y]);
    expect(win.tabs.activeTabId()).toBe(x);
    let inFrontAtDelete: string | null | undefined;
    mockedApi.deleteNote.mockImplementationOnce(async () => {
      inFrontAtDelete = win.tabs.activeTabId();
      return null;
    });

    await win.tabs.deleteNote(x);

    expect(inFrontAtDelete).toBe(y);
    expect(win.tabs.activeTabId()).toBe(y);
    expect(win.panes.isSplit()).toBe(false);
  });
});

describe("moving a tab", () => {
  it("leaves a pane's only tab where it is while the other pane is empty", async () => {
    const win = window();
    const a = (await win.tabs.createTab()).id;
    win.tabs.moveToOtherPane(a);
    expect(win.panes.isSplit()).toBe(false);
    expect(win.tabs.activeTabId()).toBe(a);
  });

  it("closes the split with every tab in one pane", async () => {
    const { tabs, panes, a, b, c } = await threeTabs();
    tabs.moveToOtherPane(c);
    tabs.closeSplit();
    expect(panes.isSplit()).toBe(false);
    expect(tabs.tabsIn("left")).toEqual([a, b, c]);
    expect(tabs.activeTabId()).toBe(c);
  });
});

describe("the panes at launch", () => {
  it("come back before a tab is chosen, keeping one a launch already opened in front", async () => {
    const before = await threeTabs();
    before.tabs.restorePanes();
    before.tabs.moveToOtherPane(before.c);

    const after = window();
    after.tabs.setActiveTabId(before.a);
    after.tabs.restorePanes();

    expect(after.panes.isSplit()).toBe(true);
    expect(after.tabs.tabsIn("right")).toEqual([before.c]);
    expect(after.tabs.activeTabId()).toBe(before.a);
    expect(after.panes.focused()).toBe("left");
  });
});
