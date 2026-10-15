import { afterEach, beforeEach, describe, it, expect, vi } from "vitest";
import { EditorSelection, EditorState, type Extension } from "@codemirror/state";
import { Decoration, EditorView, WidgetType, type RectangleMarker } from "@codemirror/view";
import {
  SELECTION_LAYER_CLASS,
  SELECTION_PIECE_CLASS,
  selectionMarkers,
  writSelection,
} from "../../editor/selection-layer";
import { writThemeLight } from "../../components/Editor/cm-theme";

// jsdom lays nothing out, so the stubs below give every line a 20px row, every
// character 8px, and the line CodeMirror's own 6px left padding.
const ROW = 20;
const CHAR = 8;
const PAD = 6;
const WIDTH = 400;

function rect(left: number, top: number, right: number, bottom: number): DOMRect {
  return { left, top, right, bottom, x: left, y: top, width: right - left, height: bottom - top, toJSON: () => ({}) };
}

function lineOf(node: Node): HTMLElement | null {
  const element = node.nodeType === 1 ? (node as HTMLElement) : node.parentElement;
  return element?.closest(".cm-line") ?? null;
}

function rowOf(line: Element): number {
  return Array.prototype.indexOf.call(line.parentElement?.children ?? [], line);
}

function textOffset(line: HTMLElement, node: Node, offset: number): number {
  const before = document.createRange();
  before.setStart(line, 0);
  before.setEnd(node, offset);
  return before.toString().length;
}

let views: EditorView[] = [];

beforeEach(() => {
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
    if (this.classList.contains("cm-line")) {
      const row = rowOf(this);
      return rect(0, row * ROW, WIDTH, (row + 1) * ROW);
    }
    if (this.classList.contains("cm-content") || this.classList.contains("cm-scroller")) {
      return rect(0, 0, WIDTH, 10 * ROW);
    }
    return rect(0, 0, 0, 0);
  });
  vi.spyOn(Range.prototype, "getClientRects").mockImplementation(function (this: Range) {
    const line = lineOf(this.startContainer);
    const rects: DOMRect[] = [];
    if (line && !this.collapsed && line === lineOf(this.endContainer)) {
      const from = textOffset(line, this.startContainer, this.startOffset);
      const to = textOffset(line, this.endContainer, this.endOffset);
      const row = rowOf(line);
      if (to > from) rects.push(rect(PAD + from * CHAR, row * ROW + 3, PAD + to * CHAR, row * ROW + 17));
    }
    return Object.assign(rects, { item: (index: number) => rects[index] ?? null }) as unknown as DOMRectList;
  });
});

afterEach(() => {
  for (const view of views) view.destroy();
  views = [];
  vi.restoreAllMocks();
});

function mount(doc: string, anchor: number, head: number, extra: Extension = []): EditorView {
  const parent = document.createElement("div");
  document.body.appendChild(parent);
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      selection: EditorSelection.single(anchor, head),
      extensions: [EditorState.allowMultipleSelections.of(true), writSelection(), writThemeLight, extra],
    }),
  });
  views.push(view);
  return view;
}

function boxes(markers: readonly unknown[]) {
  return (markers as RectangleMarker[]).map(({ left, top, width, height }) => ({
    left,
    right: left + (width ?? 0),
    top,
    bottom: top + height,
  }));
}

describe("the selection layer", () => {
  it("paints each selected line from its first selected character to its last, never to the pane edge", () => {
    const view = mount("alpha\nbeta gamma\n\ndelta", 2, 20);
    expect(boxes(selectionMarkers(view))).toEqual([
      { left: PAD + 2 * CHAR, right: PAD + 5 * CHAR, top: 0, bottom: ROW },
      { left: PAD, right: PAD + 10 * CHAR, top: ROW, bottom: 2 * ROW },
      { left: PAD, right: PAD + 2 * CHAR, top: 3 * ROW, bottom: 4 * ROW },
    ]);
  });

  it("paints every range of a multiple selection", () => {
    const view = mount("one two one", 0, 3);
    view.dispatch({
      selection: EditorSelection.create([EditorSelection.range(0, 3), EditorSelection.range(8, 11)]),
    });
    expect(boxes(selectionMarkers(view))).toEqual([
      { left: PAD, right: PAD + 3 * CHAR, top: 0, bottom: ROW },
      { left: PAD + 8 * CHAR, right: PAD + 11 * CHAR, top: 0, bottom: ROW },
    ]);
  });

  it("paints a line wider than the column to its selected text when wrapping is off", () => {
    const long = "x".repeat(60);
    const view = mount(long, 5, 60);
    expect(PAD + 60 * CHAR).toBeGreaterThan(WIDTH);
    expect(boxes(selectionMarkers(view))).toEqual([
      { left: PAD + 5 * CHAR, right: PAD + 60 * CHAR, top: 0, bottom: ROW },
    ]);
  });

  it("spans a line's vertical padding, so a padded line meets the lines around it", () => {
    const padded = Decoration.line({ attributes: { style: "padding-top: 6px; padding-bottom: 6px" } });
    const view = mount("alpha\nbeta\ngamma", 0, 16, EditorView.decorations.of(Decoration.set(padded.range(6))));
    expect(boxes(selectionMarkers(view))).toEqual([
      { left: PAD, right: PAD + 5 * CHAR, top: 0, bottom: ROW },
      { left: PAD, right: PAD + 4 * CHAR, top: ROW, bottom: 2 * ROW },
      { left: PAD, right: PAD + 5 * CHAR, top: 2 * ROW, bottom: 3 * ROW },
    ]);
  });

  it("paints nothing for a cursor", () => {
    expect(selectionMarkers(mount("alpha", 2, 2))).toEqual([]);
  });

  it("is the one layer that paints the selection", async () => {
    const view = mount("alpha\nbeta", 1, 8);
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));

    const ours = view.scrollDOM.querySelector(`.${SELECTION_LAYER_CLASS}`) as HTMLElement;
    expect(ours).not.toBeNull();
    expect(getComputedStyle(ours).display).not.toBe("none");
    expect(ours.querySelectorAll(`.${SELECTION_PIECE_CLASS}`)).toHaveLength(2);

    const painting = Array.from(view.scrollDOM.querySelectorAll<HTMLElement>(".cm-layer")).filter(
      (layer) => !layer.classList.contains("cm-cursorLayer") && getComputedStyle(layer).display !== "none",
    );
    expect(painting).toEqual([ours]);
    const theirs = view.scrollDOM.querySelector(".cm-selectionLayer") as HTMLElement;
    expect(getComputedStyle(theirs).display).toBe("none");
  });

  it("lets the pointer through to the text under it", () => {
    const view = mount("alpha", 0, 3);
    const ours = view.scrollDOM.querySelector(`.${SELECTION_LAYER_CLASS}`) as HTMLElement;
    expect(getComputedStyle(ours).pointerEvents).toBe("none");
  });
});

class InputWidget extends WidgetType {
  toDOM(): HTMLElement {
    return document.createElement("input");
  }
}

function selectStart(target: EventTarget): Event {
  const event = new Event("selectstart", { bubbles: true, cancelable: true });
  target.dispatchEvent(event);
  return event;
}

describe("Select All with focus outside the editor", () => {
  let outside: HTMLButtonElement;

  beforeEach(() => {
    outside = document.createElement("button");
    document.body.appendChild(outside);
  });

  afterEach(() => {
    outside.remove();
  });

  it("does not select the note when focus is on the chrome", () => {
    const view = mount("alpha\nbeta", 2, 2);
    outside.focus();
    expect(document.activeElement).toBe(outside);
    expect(selectStart(view.contentDOM).defaultPrevented).toBe(true);
  });

  it("does not select the note when nothing is focused", () => {
    const view = mount("alpha\nbeta", 2, 2);
    expect(document.activeElement).toBe(document.body);
    expect(selectStart(view.contentDOM).defaultPrevented).toBe(true);
  });

  it("selects the note when the editor is focused", () => {
    const view = mount("alpha\nbeta", 2, 2);
    view.focus();
    expect(document.activeElement).toBe(view.contentDOM);
    expect(selectStart(view.contentDOM).defaultPrevented).toBe(false);
  });

  it("leaves selection to a focused field inside the note", () => {
    const field = Decoration.widget({ widget: new InputWidget(), side: 1 });
    const view = mount("alpha\nbeta", 2, 2, EditorView.decorations.of(Decoration.set(field.range(3))));
    const input = view.contentDOM.querySelector("input") as HTMLInputElement;
    input.focus();
    expect(document.activeElement).toBe(input);
    expect(view.hasFocus).toBe(false);
    expect(selectStart(view.contentDOM).defaultPrevented).toBe(false);
  });

  it("leaves a view that cannot be edited to the browser", () => {
    const view = mount("alpha\nbeta", 2, 2, EditorView.editable.of(false));
    expect(document.activeElement).toBe(document.body);
    expect(selectStart(view.contentDOM).defaultPrevented).toBe(false);
  });
});

describe("the DOM selection when focus leaves the editor", () => {
  let paragraph: HTMLParagraphElement;

  beforeEach(() => {
    paragraph = document.createElement("p");
    paragraph.textContent = "chrome text";
    document.body.append(paragraph);
  });

  afterEach(() => {
    paragraph.remove();
    document.getSelection()?.removeAllRanges();
  });

  function caretInContent(view: EditorView): boolean {
    const selection = document.getSelection();
    return !!selection && selection.rangeCount > 0 && view.contentDOM.contains(selection.anchorNode);
  }

  it("is dropped when focus falls to the page", () => {
    const view = mount("alpha\nbeta", 2, 2);
    view.focus();
    expect(caretInContent(view)).toBe(true);
    view.contentDOM.blur();
    expect(document.activeElement).toBe(document.body);
    expect(document.getSelection()?.rangeCount).toBe(0);
  });

  it("is kept when the window loses focus and the editor stays the active element", () => {
    const view = mount("alpha\nbeta", 2, 2);
    view.focus();
    view.contentDOM.dispatchEvent(new FocusEvent("blur"));
    expect(document.activeElement).toBe(view.contentDOM);
    expect(caretInContent(view)).toBe(true);
  });

  it("leaves a selection outside the editor alone", () => {
    const view = mount("alpha\nbeta", 2, 2);
    view.focus();
    const text = paragraph.firstChild as Text;
    document.getSelection()?.setBaseAndExtent(text, 0, text, 6);
    view.contentDOM.blur();
    expect(document.getSelection()?.toString()).toBe("chrome");
  });

  it("is written back from the editor's state when focus returns to the content element", () => {
    const view = mount("alpha\nbeta", 8, 8);
    view.focus();
    view.contentDOM.blur();
    // WebKit fires focus while the DOM selection is still outside the content;
    // jsdom collapses it into the element first, so that order is set up here.
    view.contentDOM.focus();
    document.getSelection()?.removeAllRanges();
    view.contentDOM.dispatchEvent(new FocusEvent("focus"));
    const selection = document.getSelection();
    expect(document.activeElement).toBe(view.contentDOM);
    expect(selection?.anchorNode?.textContent).toBe("beta");
    expect(selection?.anchorOffset).toBe(2);
    expect(view.state.selection.main.head).toBe(8);
  });

  it("is written back from the editor's state when the editor is focused again", () => {
    const view = mount("alpha\nbeta", 2, 2);
    view.focus();
    view.contentDOM.blur();
    view.focus();
    expect(caretInContent(view)).toBe(true);
    expect(view.state.selection.main.head).toBe(2);
  });
});
