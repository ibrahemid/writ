import { describe, it, expect, afterEach } from "vitest";
import { EditorSelection, EditorState, type Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { history, undo, undoDepth } from "@codemirror/commands";
import {
  endDictationAnchor,
  insertDictationSegment,
  showDictationPartial,
  startDictationAnchor,
  withoutDictationAnchor,
} from "../../editor/dictation";

const views: EditorView[] = [];

function makeView(
  doc: string,
  cursor: number | [number, number],
  readOnly = false,
  extra: Extension[] = [],
): EditorView {
  const parent = document.createElement("div");
  document.body.appendChild(parent);
  const [anchor, head] = typeof cursor === "number" ? [cursor, cursor] : cursor;
  const view = new EditorView({
    state: EditorState.create({
      doc,
      selection: EditorSelection.single(anchor, head),
      extensions: [history(), EditorState.readOnly.of(readOnly), extra],
    }),
    parent,
  });
  views.push(view);
  return view;
}

afterEach(() => {
  for (const view of views.splice(0)) {
    view.dom.parentElement?.remove();
    view.destroy();
  }
});

const text = (view: EditorView) => view.state.doc.toString();

describe("dictation at the cursor", () => {
  it("inserts each segment at the cursor, spaced, with the caret after it", () => {
    const view = makeView("Hello", 5);
    expect(startDictationAnchor(view)).toBe(true);
    expect(insertDictationSegment(view, "there")).toBe("inserted");
    expect(insertDictationSegment(view, "friend.")).toBe("inserted");
    expect(text(view)).toBe("Hello there friend.");
    expect(view.state.selection.main.head).toBe(text(view).length);
  });

  it("keeps the anchor in place while the document is typed into elsewhere", () => {
    const view = makeView("one two", 3);
    startDictationAnchor(view);
    view.dispatch({ changes: { from: 0, insert: ">> " } });
    view.dispatch({ changes: { from: view.state.doc.length, insert: " three" } });
    insertDictationSegment(view, "and a half");
    expect(text(view)).toBe(">> one and a half two three");
  });

  it("follows the caret only when it sat at the anchor", () => {
    const view = makeView("alpha beta", 5);
    startDictationAnchor(view);
    view.dispatch({ selection: EditorSelection.cursor(10) });
    insertDictationSegment(view, "gamma");
    expect(text(view)).toBe("alpha gamma beta");
    expect(view.state.selection.main.head).toBe(text(view).length);
  });

  it("makes each segment its own undo step", () => {
    const view = makeView("Start", 5);
    startDictationAnchor(view);
    insertDictationSegment(view, "one");
    insertDictationSegment(view, "two");
    expect(undoDepth(view.state)).toBe(2);
    undo(view);
    expect(text(view)).toBe("Start one");
    undo(view);
    expect(text(view)).toBe("Start");
  });

  it("tags the insertion as dictation input", () => {
    const events: string[] = [];
    const listener = EditorView.updateListener.of((update) => {
      for (const tr of update.transactions) {
        if (tr.docChanged) events.push(tr.isUserEvent("input.dictation") ? "dictation" : "other");
      }
    });
    const view = makeView("", 0, false, [listener]);
    startDictationAnchor(view);
    insertDictationSegment(view, "spoken");
    expect(events).toEqual(["dictation"]);
  });

  it("replaces the selection the session started on with the first segment only", () => {
    const view = makeView("keep REPLACE keep", [5, 12]);
    startDictationAnchor(view);
    insertDictationSegment(view, "new words");
    insertDictationSegment(view, "more");
    expect(text(view)).toBe("keep new words more keep");
  });

  it("draws the partial text after the anchor without putting it in the document", () => {
    const view = makeView("Note", 4);
    startDictationAnchor(view);
    showDictationPartial(view, "maybe this");
    expect(text(view)).toBe("Note");
    const partial = view.dom.querySelector(".cm-dictation-partial");
    expect(partial?.textContent).toBe(" maybe this");
    expect(partial?.getAttribute("dir")).toBe("auto");
    insertDictationSegment(view, "maybe this.");
    expect(view.dom.querySelector(".cm-dictation-partial")).toBeNull();
    expect(text(view)).toBe("Note maybe this.");
  });

  it("clears the partial text when the session ends", () => {
    const view = makeView("Note", 4);
    startDictationAnchor(view);
    showDictationPartial(view, "unsure");
    endDictationAnchor(view);
    expect(view.dom.querySelector(".cm-dictation-partial")).toBeNull();
    expect(insertDictationSegment(view, "late")).toBe("anchor-lost");
    expect(text(view)).toBe("Note");
  });

  it("refuses a read-only document", () => {
    const view = makeView("locked", 6, true);
    expect(startDictationAnchor(view)).toBe(false);
    expect(insertDictationSegment(view, "words")).toBe("anchor-lost");
    expect(text(view)).toBe("locked");
  });

  it("answers anchor-lost once the editor state is replaced", () => {
    const view = makeView("before", 6);
    startDictationAnchor(view);
    view.setState(EditorState.create({ doc: "another file" }));
    expect(insertDictationSegment(view, "words")).toBe("anchor-lost");
    expect(text(view)).toBe("another file");
  });

  it("keeps no partial text or anchor in a tab's state kept for later", () => {
    const view = makeView("Tab A text", 10);
    startDictationAnchor(view);
    showDictationPartial(view, "half heard");
    const kept = withoutDictationAnchor(view.state);
    view.setState(EditorState.create({ doc: "Tab B" }));
    endDictationAnchor(view);
    view.setState(kept);
    expect(view.dom.querySelector(".cm-dictation-partial")).toBeNull();
    expect(insertDictationSegment(view, "late")).toBe("anchor-lost");
    expect(text(view)).toBe("Tab A text");
  });

  it("hands back a state with no dictation in it unchanged", () => {
    const view = makeView("plain", 5);
    expect(withoutDictationAnchor(view.state)).toBe(view.state);
  });

  it("inserts nothing for a segment of whitespace", () => {
    const view = makeView("same", 4);
    startDictationAnchor(view);
    expect(insertDictationSegment(view, "  ")).toBe("inserted");
    expect(text(view)).toBe("same");
    expect(undoDepth(view.state)).toBe(0);
  });
});
