import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { Show, batch, createSignal } from "solid-js";
import { EditorView } from "@codemirror/view";
import { EditorSelection } from "@codemirror/state";
import { undo, undoDepth } from "@codemirror/commands";
import type { BufferDocument } from "../../../types/buffer";
import WindowProvider, { useWindow } from "../../WindowProvider/WindowProvider";
import type { WindowState } from "../../../stores/window/createWindowState";
import type { PaneId } from "../../../lib/pane-layout";
import { agentEditRanges } from "../../../editor/agent-edit-marks";

// Two editors side by side (ADR-056): a note open in both shares its document
// and its save, each pane keeps its own caret and undo, a tab moved to the
// other pane keeps its history, and the editor's commands act on the focused
// pane however many editors are mounted.

const bufferContent = new Map<string, string>();
const saves: Array<[string, string]> = [];
const openTabs: BufferDocument[] = [];

vi.mock("../../../services/tauri", () => ({
  readBufferContent: vi.fn(async (id: string) => bufferContent.get(id) ?? ""),
  saveBufferContent: vi.fn(async (id: string, content: string) => {
    saves.push([id, content]);
    bufferContent.set(id, content);
    return null;
  }),
  noteDiskState: vi.fn(async () => ({ state: "no_file" })),
  setBufferViewPosition: vi.fn(async () => undefined),
  updateConfig: vi.fn(async () => undefined),
  checkSpelling: vi.fn(async () => []),
}));

vi.mock("../../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    readContent: vi.fn(async (id: string) => bufferContent.get(id) ?? ""),
    activeTabs: () => openTabs,
  },
}));

import { getCommand, executeCommand } from "../../../commands/registry";
import { bufferRegistry } from "../../../stores/global/buffer-registry";
import { findStore } from "../../../stores/global/find-store";
import { spellingStore } from "../../../stores/global/spelling";
import { configStore } from "../../../stores/global/config";

function mockBuffer(id: string): BufferDocument {
  return {
    id,
    title: id,
    filename: `${id}.txt`,
    status: "active",
    language: null,
    source_path: null,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    closed_at: null,
    read_only: false,
    size_bytes: 0,
    line_ending: "lf",
    encoding: "UTF-8",
  };
}

async function settle(n = 40) {
  for (let i = 0; i < n; i++) await Promise.resolve();
}

async function mountPanes(left: BufferDocument | null) {
  const EditorInstance = (await import("../EditorInstance")).default;
  const [leftBuf, setLeft] = createSignal<BufferDocument | null>(left);
  const [rightBuf, setRight] = createSignal<BufferDocument | null>(null);
  let win!: WindowState;
  function Capture() {
    win = useWindow();
    return null;
  }
  const result = render(() => (
    <WindowProvider windowId={9705}>
      <Capture />
      <Show when={leftBuf()}>{(b) => <EditorInstance buffer={b()} pane="left" />}</Show>
      <Show when={rightBuf()}>{(b) => <EditorInstance buffer={b()} pane="right" />}</Show>
    </WindowProvider>
  ));
  await settle();
  const view = (pane: PaneId): EditorView => {
    const found = win.editor.forPane(pane).getView();
    if (!found) throw new Error(`no editor in the ${pane} pane`);
    return found;
  };
  return { ...result, setLeft, setRight, win: () => win, view };
}

function type(view: EditorView, at: number, text: string) {
  view.dispatch({
    changes: { from: at, insert: text },
    selection: EditorSelection.cursor(at + text.length),
    userEvent: "input.type",
  });
}

beforeEach(async () => {
  (await import("../../../services/autosave")).resetAutosave();
  bufferContent.clear();
  saves.length = 0;
  openTabs.length = 0;
  localStorage.clear();
  vi.clearAllMocks();
});

afterEach(() => {
  cleanup();
});

describe("a note open in both panes", () => {
  it("mirrors an edit, keeps each caret and undo, and saves once", async () => {
    const a = mockBuffer("A");
    openTabs.push(a);
    bufferContent.set("A", "first line\nsecond line\n");
    const panes = await mountPanes(a);
    panes.win().tabs.setActiveTabId("A");
    panes.win().tabs.splitRight();
    panes.setRight(a);
    await settle();

    const left = panes.view("left");
    const right = panes.view("right");
    expect(right).not.toBe(left);
    expect(right.state.doc.toString()).toBe("first line\nsecond line\n");
    expect(vi.mocked(bufferRegistry.readContent)).toHaveBeenCalledTimes(1);

    right.dispatch({ selection: EditorSelection.cursor(17) });
    type(left, 0, "new ");
    expect(right.state.doc.toString()).toBe("new first line\nsecond line\n");
    expect(left.state.selection.main.head).toBe(4);
    expect(right.state.selection.main.head).toBe(21);
    expect(undoDepth(left.state)).toBe(1);
    expect(undoDepth(right.state)).toBe(0);

    type(right, right.state.doc.length, "third");
    expect(left.state.doc.toString()).toBe("new first line\nsecond line\nthird");

    await panes.win().editor.flushAutosave("A");
    expect(saves).toEqual([["A", "new first line\nsecond line\nthird"]]);
  });

  it("keeps the other pane's editor when one pane lets go of the note", async () => {
    const a = mockBuffer("A");
    openTabs.push(a);
    bufferContent.set("A", "kept\n");
    const panes = await mountPanes(a);
    panes.win().tabs.setActiveTabId("A");
    panes.win().tabs.splitRight();
    panes.setRight(a);
    await settle();
    type(panes.view("left"), 4, " here");

    await panes.win().tabs.closeTab("A", "right");
    panes.setRight(null);
    await settle();

    expect(panes.win().panes.isSplit()).toBe(false);
    expect(openTabs).toContain(a);
    const left = panes.view("left");
    expect(left.state.doc.toString()).toBe("kept here\n");
    expect(undoDepth(left.state)).toBe(1);
    expect(panes.win().editor.isTracked("A")).toBe(true);
  });
});

describe("a program's write to a note open in both panes", () => {
  const CLAUDE = { kind: "program", name: "claude-code" } as const;

  function markedText(view: EditorView): { text: string; title: string }[] {
    const doc = view.state.doc.toString();
    return agentEditRanges(view.state).map((range) => ({
      text: doc.slice(range.from, range.to),
      title: range.title,
    }));
  }

  it("is marked in both panes' editors", async () => {
    const a = mockBuffer("A");
    openTabs.push(a);
    bufferContent.set("A", "one\n");
    const panes = await mountPanes(a);
    panes.win().tabs.setActiveTabId("A");
    panes.win().tabs.splitRight();
    panes.setRight(a);
    await settle();

    bufferContent.set("A", "one\nfrom the program\n");
    panes.win().editor.requestExternalReload("A", CLAUDE);
    await settle();

    expect(vi.mocked(bufferRegistry.readContent)).toHaveBeenCalledTimes(2);
    for (const pane of ["left", "right"] as const) {
      expect(panes.view(pane).state.doc.toString()).toBe("one\nfrom the program\n");
      expect(markedText(panes.view(pane))).toHaveLength(1);
      expect(markedText(panes.view(pane))[0].title).toBe("Changed by Claude Code");
    }
    expect(markedText(panes.view("right"))).toEqual(markedText(panes.view("left")));
    expect(panes.win().editor.updatedFromDiskBy("A")).toBe("Claude Code");

    type(panes.view("left"), 0, "x");
    expect(markedText(panes.view("left"))).toEqual([]);
    expect(markedText(panes.view("right"))).toEqual([]);
  });

  it("is marked in the state the other pane keeps for it, and shows when that pane comes back to it", async () => {
    const a = mockBuffer("A");
    const b = mockBuffer("B");
    openTabs.push(a, b);
    bufferContent.set("A", "one\n");
    bufferContent.set("B", "other\n");
    const panes = await mountPanes(a);
    panes.win().tabs.setActiveTabId("A");
    panes.win().tabs.splitRight();
    panes.setRight(a);
    await settle();
    panes.win().tabs.moveToOtherPane("B", "left");
    panes.setRight(b);
    await settle();
    expect(panes.view("right").state.doc.toString()).toBe("other\n");

    bufferContent.set("A", "one\nfrom the program\n");
    panes.win().editor.requestExternalReload("A", CLAUDE);
    await settle();
    expect(markedText(panes.view("left"))).toHaveLength(1);

    panes.win().tabs.selectIn("right", "A");
    panes.setRight(a);
    await settle();

    expect(panes.view("right").state.doc.toString()).toBe("one\nfrom the program\n");
    expect(markedText(panes.view("right"))).toEqual(markedText(panes.view("left")));
  });

  it("drops the marks from the state a pane keeps when it leaves the note", async () => {
    const a = mockBuffer("A");
    const b = mockBuffer("B");
    openTabs.push(a, b);
    bufferContent.set("A", "one\n");
    bufferContent.set("B", "other\n");
    const panes = await mountPanes(a);
    panes.win().tabs.setActiveTabId("A");
    panes.win().tabs.splitRight();
    panes.setRight(a);
    await settle();

    bufferContent.set("A", "one\nfrom the program\n");
    panes.win().editor.requestExternalReload("A", CLAUDE);
    await settle();
    expect(markedText(panes.view("right"))).toHaveLength(1);

    panes.win().tabs.moveToOtherPane("B", "left");
    panes.setRight(b);
    await settle();
    panes.win().tabs.selectIn("right", "A");
    panes.setRight(a);
    await settle();

    expect(panes.view("right").state.doc.toString()).toBe("one\nfrom the program\n");
    expect(markedText(panes.view("right"))).toEqual([]);
    expect(markedText(panes.view("left"))).toHaveLength(1);
  });
});

describe("a note restored in both panes at launch", () => {
  it("is read once, and both panes share its document and every later change", async () => {
    const a = mockBuffer("A");
    openTabs.push(a);
    bufferContent.set("A", "one\n");
    localStorage.setItem(
      "writ.editor.panes.v1",
      JSON.stringify({ v: 1, ratio: 0.5, focused: "right", right: [], both: ["A"], active: { left: "A", right: "A" } }),
    );
    // Another program writes the file just after the first read.
    vi.mocked(bufferRegistry.readContent).mockImplementationOnce(async (id: string) => {
      const text = bufferContent.get(id) ?? "";
      bufferContent.set(id, "one\nchanged elsewhere\n");
      return text;
    });
    const panes = await mountPanes(null);
    panes.win().panes.restore(["A"]);
    batch(() => {
      panes.setLeft(a);
      panes.setRight(a);
    });
    await settle();

    expect(vi.mocked(bufferRegistry.readContent)).toHaveBeenCalledTimes(1);
    expect(panes.view("left").state.doc).toBe(panes.view("right").state.doc);

    panes.win().editor.requestExternalReload("A");
    await settle();
    expect(panes.view("left").state.doc.toString()).toBe("one\nchanged elsewhere\n");
    expect(panes.view("right").state.doc.toString()).toBe("one\nchanged elsewhere\n");

    type(panes.view("left"), 0, "typed ");
    await panes.win().editor.flushAutosave("A");
    expect(saves).toEqual([["A", "typed one\nchanged elsewhere\n"]]);
    expect(panes.view("right").state.doc.toString()).toBe("typed one\nchanged elsewhere\n");
  });

  it("reads the note in the second pane when the first pane's read fails", async () => {
    const a = mockBuffer("A");
    openTabs.push(a);
    bufferContent.set("A", "one\n");
    localStorage.setItem(
      "writ.editor.panes.v1",
      JSON.stringify({ v: 1, ratio: 0.5, focused: "right", right: [], both: ["A"], active: { left: "A", right: "A" } }),
    );
    vi.mocked(bufferRegistry.readContent).mockImplementationOnce(async () => {
      throw new Error("the disk did not answer");
    });
    const panes = await mountPanes(null);
    panes.win().panes.restore(["A"]);
    batch(() => {
      panes.setLeft(a);
      panes.setRight(a);
    });
    await settle();

    expect(vi.mocked(bufferRegistry.readContent)).toHaveBeenCalledTimes(2);
    expect(panes.win().editor.forPane("left").getView()).toBeNull();
    expect(panes.win().editor.forPane("left").readFailure()?.bufferId).toBe("A");
    expect(panes.view("right").state.doc.toString()).toBe("one\n");
  });
});

describe("a note whose file was deleted, opened in the second pane", () => {
  it("shows the typing the first pane holds and reads nothing from disk", async () => {
    const a = mockBuffer("A");
    openTabs.push(a);
    bufferContent.set("A", "on disk\n");
    const panes = await mountPanes(a);
    panes.win().editor.markRemovedOnDisk("A", "on disk\n");
    type(panes.view("left"), 7, " and typed");
    vi.mocked(bufferRegistry.readContent).mockClear();

    panes.win().tabs.setActiveTabId("A");
    panes.win().tabs.splitRight();
    panes.setRight(a);
    await settle();

    expect(panes.view("right").state.doc.toString()).toBe("on disk and typed\n");
    expect(vi.mocked(bufferRegistry.readContent)).not.toHaveBeenCalled();
    expect(panes.win().editor.isRemovedOnDisk("A")).toBe(true);
    expect(panes.win().editor.isDirty("A")).toBe(true);
    expect(saves).toEqual([]);
  });
});

describe("a tab moved to the other pane", () => {
  it("keeps its undo history and its edits", async () => {
    const a = mockBuffer("A");
    const b = mockBuffer("B");
    openTabs.push(a, b);
    bufferContent.set("A", "moving\n");
    bufferContent.set("B", "staying\n");
    const panes = await mountPanes(a);
    panes.win().tabs.setActiveTabId("A");
    type(panes.view("left"), 6, " along");

    panes.win().tabs.moveToOtherPane("A", "left");
    expect(panes.win().panes.activeIn("right")).toBe("A");
    expect(panes.win().panes.activeIn("left")).toBe("B");
    panes.setLeft(b);
    panes.setRight(a);
    await settle();

    const right = panes.view("right");
    expect(right.state.doc.toString()).toBe("moving along\n");
    expect(undoDepth(right.state)).toBe(1);
    undo(right);
    expect(right.state.doc.toString()).toBe("moving\n");
    expect(panes.view("left").state.doc.toString()).toBe("staying\n");

    await panes.win().editor.flushAutosave("A");
    expect(saves[saves.length - 1]).toEqual(["A", "moving\n"]);
  });
});

describe("the editor's commands with two editors mounted", () => {
  it("act on the focused pane and outlive the right pane's editor", async () => {
    const a = mockBuffer("A");
    const b = mockBuffer("B");
    openTabs.push(a, b);
    bufferContent.set("A", "one\ntwo\n");
    bufferContent.set("B", "three\nfour\n");
    const panes = await mountPanes(a);
    panes.win().tabs.setActiveTabId("A");
    panes.win().tabs.setActiveTabId("B");
    panes.win().tabs.moveToOtherPane("B", "left");
    panes.setRight(b);
    await settle();
    expect(panes.win().panes.focused()).toBe("right");

    executeCommand("editor.selectLine");
    expect(panes.view("right").state.selection.main.to).toBe(6);
    expect(panes.view("left").state.selection.main.empty).toBe(true);

    panes.win().tabs.closeSplit();
    panes.setRight(null);
    panes.setLeft(b);
    await settle();
    expect(getCommand("editor.selectLine")).toBeDefined();
    expect(getCommand("editor.addCursorUp")).toBeDefined();
  });
});

describe("focus moving to the other pane", () => {
  afterEach(async () => {
    findStore.close();
    vi.restoreAllMocks();
    const current = configStore.config();
    await configStore.save({ ...current, spelling: { ...current.spelling, enabled: false } });
  });

  it("takes an open Find bar and the spell check to the newly focused pane's note", async () => {
    const current = configStore.config();
    await configStore.save({ ...current, spelling: { ...current.spelling, enabled: true } });
    const a = mockBuffer("A");
    const b = mockBuffer("B");
    openTabs.push(a, b);
    bufferContent.set("A", "needle and needle\n");
    bufferContent.set("B", "one needle\n");
    const panes = await mountPanes(a);
    panes.win().tabs.setActiveTabId("A");
    panes.win().tabs.setActiveTabId("B");
    panes.win().tabs.moveToOtherPane("B", "left");
    panes.setRight(b);
    await settle();
    panes.win().tabs.focusOtherPane();
    expect(panes.win().panes.focused()).toBe("left");

    findStore.open();
    findStore.setQueryText("needle");
    await vi.waitFor(() => expect(findStore.matches().total).toBe(2));
    const attach = vi.spyOn(spellingStore, "attach");

    panes.win().tabs.focusOtherPane();

    expect(panes.win().panes.focused()).toBe("right");
    await vi.waitFor(() => expect(findStore.matches().total).toBe(1));
    expect(panes.view("right").dom.querySelectorAll(".cm-searchMatch")).toHaveLength(1);
    expect(attach).toHaveBeenLastCalledWith(panes.view("right"));
  });
});
