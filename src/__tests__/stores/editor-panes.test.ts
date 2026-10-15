import { describe, it, expect, vi, beforeEach } from "vitest";
import { createRoot, createSignal } from "solid-js";
import { EditorState, Text } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { history, undoDepth } from "@codemirror/commands";

// The editor store with two panes (ADR-056): every top-level accessor answers
// for the focused pane, a pane's slot is its own, a note's dirty answer holds
// for each note on screen, and a change in one pane's editor of a note reaches
// the other pane's.

const backend = vi.hoisted(() => ({
  answers: new Map<string, { state: string; disk?: { hash: string } }>(),
}));

vi.mock("../../services/tauri", () => ({
  saveBufferContent: vi.fn().mockResolvedValue(null),
  recordUnsavedNotes: vi.fn().mockResolvedValue(undefined),
  discardUnsavedText: vi.fn().mockResolvedValue(undefined),
  setBufferViewPosition: vi.fn().mockResolvedValue(undefined),
  noteDiskState: vi.fn(async (id: string) => backend.answers.get(id) ?? { state: "no_file" }),
}));

import { createEditorStore, DIRTY_COMPARE_IDLE_MS } from "../../stores/window/editor-store";
import { createCountsStore } from "../../stores/window/counts-store";
import { mirroredSpec } from "../../editor/pane-mirror";
import { agentEditMarks, agentEditRanges } from "../../editor/agent-edit-marks";
import { resetAutosave } from "../../services/autosave";
import type { PaneId } from "../../lib/pane-layout";

function store() {
  return createRoot(() => {
    const [focused, setFocused] = createSignal<PaneId>("left");
    return { editor: createEditorStore({ focusedPane: focused }), setFocused };
  });
}

function viewOf(text: string): EditorView {
  return new EditorView({ state: EditorState.create({ doc: text, extensions: history() }) });
}

function compared(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, DIRTY_COMPARE_IDLE_MS + 30));
}

async function opened(editor: ReturnType<typeof store>["editor"], id: string, view: EditorView) {
  backend.answers.set(id, { state: "described", disk: { hash: `${id}`.padEnd(64, "0") } });
  editor.noteOpened(id, view.state.doc, true);
  await vi.waitFor(() => expect(editor.isTracked(id)).toBe(true));
}

beforeEach(() => {
  resetAutosave();
  backend.answers.clear();
});

describe("the store as each pane sees it", () => {
  it("answers for the focused pane at the top level", () => {
    const { editor, setFocused } = store();
    const left = viewOf("left text");
    const right = viewOf("right text");
    editor.forPane("left").registerView(left);
    editor.forPane("left").setCurrentBufferId("A");
    editor.forPane("right").registerView(right);
    editor.forPane("right").setCurrentBufferId("B");
    editor.forPane("right").setCursorLine(7);

    expect(editor.getView()).toBe(left);
    expect(editor.currentBufferId()).toBe("A");
    expect(editor.cursorLine()).toBe(1);

    setFocused("right");
    expect(editor.getView()).toBe(right);
    expect(editor.currentBufferId()).toBe("B");
    expect(editor.cursorLine()).toBe(7);
    expect(editor.liveDoc()?.toString()).toBe("right text");
  });

  it("counts the focused pane's document, again once focus moves", async () => {
    const { editor, setFocused } = store();
    editor.forPane("left").registerView(viewOf("one two"));
    editor.forPane("left").setCurrentBufferId("A");
    editor.forPane("right").registerView(viewOf("three four five"));
    editor.forPane("right").setCurrentBufferId("B");
    const counts = createRoot(() => createCountsStore(editor));
    counts.watch();
    await vi.waitFor(() => expect(counts.counts()?.words).toBe(2));

    setFocused("right");
    await vi.waitFor(() => expect(counts.counts()?.words).toBe(3));
  });

  it("names the left pane's editor to register the commands, else the right's", () => {
    const { editor } = store();
    expect(editor.commandPane()).toBeNull();
    editor.forPane("right").setMounted(true);
    expect(editor.commandPane()).toBe("right");
    editor.forPane("left").setMounted(true);
    expect(editor.commandPane()).toBe("left");
  });
});

describe("a note on screen in each pane", () => {
  it("reads clean again for both once each edit is undone", async () => {
    const { editor } = store();
    const left = viewOf("alpha");
    const right = viewOf("beta");
    editor.forPane("left").registerView(left);
    editor.forPane("left").setCurrentBufferId("A");
    await opened(editor, "A", left);
    editor.forPane("right").registerView(right);
    editor.forPane("right").setCurrentBufferId("B");
    await opened(editor, "B", right);

    editor.noteEdited("A", Text.of(["alpha!"]));
    editor.noteEdited("B", Text.of(["beta!"]));
    expect(editor.isDirty("A")).toBe(true);
    expect(editor.isDirty("B")).toBe(true);

    editor.noteEdited("A", Text.of(["alpha"]));
    editor.noteEdited("B", Text.of(["beta"]));
    await compared();
    expect(editor.isDirty("A")).toBe(false);
    expect(editor.isDirty("B")).toBe(false);
  });
});

describe("a change to a note open in both panes", () => {
  it("reaches the other pane's editor outside its undo history", () => {
    const { editor } = store();
    const left = viewOf("shared");
    const right = new EditorView({
      state: EditorState.create({ doc: left.state.doc, extensions: history() }),
    });
    editor.forPane("left").registerView(left);
    editor.forPane("left").setCurrentBufferId("A");
    editor.forPane("right").registerView(right);
    editor.forPane("right").setCurrentBufferId("A");

    right.dispatch({ selection: { anchor: 6 } });

    const tr = left.state.update({ changes: { from: 0, insert: "the " }, userEvent: "input.type" });
    left.dispatch(tr);
    editor.forPane("left").mirrorChanges("A", tr.startState.doc.length, mirroredSpec(tr.changes, false));

    expect(right.state.doc.toString()).toBe("the shared");
    expect(right.state.selection.main.head).toBe(10);
    expect(left.state.selection.main.head).toBe(0);
    expect(undoDepth(left.state)).toBe(1);
    expect(undoDepth(right.state)).toBe(0);
  });

  it("is carried into the state the other pane keeps for it", () => {
    const { editor } = store();
    const left = viewOf("kept");
    editor.forPane("left").registerView(left);
    editor.forPane("left").setCurrentBufferId("A");
    const keptState = EditorState.create({ doc: left.state.doc, extensions: history() });
    editor.forPane("right").keepTabState("A", { state: keptState, scroll: null, mode: { kind: "Normal" }, longest: 4 });

    const tr = left.state.update({ changes: { from: 4, insert: " text" } });
    left.dispatch(tr);
    editor.forPane("left").mirrorChanges("A", tr.startState.doc.length, mirroredSpec(tr.changes, false));

    expect(editor.forPane("right").takeTabState("A")?.state.doc.toString()).toBe("kept text");
  });

  it("leaves a kept state alone that no longer lines up, and drops it", () => {
    const { editor } = store();
    const left = viewOf("one");
    editor.forPane("left").registerView(left);
    editor.forPane("left").setCurrentBufferId("A");
    const other = EditorState.create({ doc: "something longer" });
    editor.forPane("right").keepTabState("A", { state: other, scroll: null, mode: { kind: "Normal" }, longest: 16 });

    const tr = left.state.update({ changes: { from: 3, insert: "!" } });
    left.dispatch(tr);
    editor.forPane("left").mirrorChanges("A", tr.startState.doc.length, mirroredSpec(tr.changes, false));

    expect(editor.forPane("right").takeTabState("A")).toBeUndefined();
  });
});

describe("a tab moving to the other pane", () => {
  it("takes the state of the pane's editor with it, marked as built there", () => {
    const { editor } = store();
    const left = viewOf("moved");
    left.dispatch({ changes: { from: 5, insert: " along" } });
    editor.forPane("left").registerView(left);
    editor.forPane("left").setCurrentBufferId("A");

    editor.moveTabState("left", "right", "A");

    const taken = editor.forPane("right").takeTabState("A");
    expect(taken?.foreign).toBe(true);
    expect(taken?.state.doc.toString()).toBe("moved along");
    expect(undoDepth(taken!.state)).toBe(1);
  });

  it("drops what a pane kept for a tab that only left it", () => {
    const { editor } = store();
    editor.forPane("right").keepTabState("A", {
      state: EditorState.create({ doc: "x" }),
      scroll: null,
      mode: { kind: "Normal" },
      longest: 1,
    });
    editor.dropTabState("right", "A");
    expect(editor.forPane("right").takeTabState("A")).toBeUndefined();
  });
});

describe("a file that changed under a note no pane shows", () => {
  it("is taken into every state the panes keep for it", () => {
    const { editor } = store();
    for (const pane of ["left", "right"] as const) {
      editor.forPane(pane).keepTabState("A", {
        state: EditorState.create({ doc: "old", extensions: history() }),
        scroll: null,
        mode: { kind: "Normal" },
        longest: 3,
      });
    }
    editor.applyExternalContent("A", "new\r\n");
    for (const pane of ["left", "right"] as const) {
      expect(editor.forPane(pane).takeTabState("A")?.state.doc.toString()).toBe("new\n");
    }
  });

  it("marks a program's text in every state the panes keep for it", () => {
    const { editor } = store();
    for (const pane of ["left", "right"] as const) {
      editor.forPane(pane).keepTabState("A", {
        state: EditorState.create({ doc: "old\n", extensions: [history(), agentEditMarks] }),
        scroll: null,
        mode: { kind: "Normal" },
        longest: 3,
      });
    }
    editor.applyExternalContent("A", "old\nnew\n", { kind: "program", name: "claude-code" });
    for (const pane of ["left", "right"] as const) {
      const kept = editor.forPane(pane).takeTabState("A")!.state;
      expect(kept.doc.toString()).toBe("old\nnew\n");
      expect(agentEditRanges(kept).map((range) => range.title)).toEqual(["Changed by Claude Code"]);
      expect(undoDepth(kept)).toBe(1);
    }
    expect(editor.updatedFromDiskBy("A")).toBe("Claude Code");
  });
});
