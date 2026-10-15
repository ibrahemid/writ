import type { Extension, SelectionRange } from "@codemirror/state";
import {
  BlockType,
  Direction,
  EditorView,
  RectangleMarker,
  drawSelection,
  layer,
  type BlockInfo,
  type LayerMarker,
} from "@codemirror/view";
import {
  clipToColumn,
  isVisibleBox,
  piecesForLine,
  replaceBoxes,
  type Box,
  type Span,
} from "./selection-geometry";

/** The class of the layer that paints the selection. */
export const SELECTION_LAYER_CLASS = "cm-writ-selection";
/** The class of each piece, shared with CodeMirror's own selection styling. */
export const SELECTION_PIECE_CLASS = "cm-selectionBackground";

/** A space in fonts the canvas cannot measure, in ems. */
const SPACE_EM_FALLBACK = 0.3;
/** A box this wide against the column may be a widget's block container. */
const BLOCK_CONTAINER_SHARE = 0.9;
/** How far a widget's box may sit from its height-map extent, in pixels. */
const WIDGET_TOLERANCE_PX = 2;

interface DomPoint {
  node: Node;
  offset: number;
}

function pixels(value: string): number {
  const parsed = parseFloat(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

function boxOf(rect: DOMRectReadOnly): Box {
  return { left: rect.left, right: rect.right, top: rect.top, bottom: rect.bottom };
}

function isElement(node: Node | null | undefined): node is HTMLElement {
  return !!node && node.nodeType === 1;
}

// CodeMirror's own getBase, which it does not export: layer markers are
// positioned from the scroller's scroll origin.
function layerOrigin(view: EditorView): { left: number; top: number } {
  const rect = view.scrollDOM.getBoundingClientRect();
  const left =
    view.textDirection === Direction.LTR ? rect.left : rect.right - view.scrollDOM.clientWidth * view.scaleX;
  return {
    left: left - view.scrollDOM.scrollLeft * view.scaleX,
    top: rect.top - view.scrollDOM.scrollTop * view.scaleY,
  };
}

function textColumn(view: EditorView): Span {
  const rect = view.contentDOM.getBoundingClientRect();
  const style = getComputedStyle(view.contentDOM);
  return {
    left: rect.left + pixels(style.paddingLeft) * view.scaleX,
    right: rect.right - pixels(style.paddingRight) * view.scaleX,
  };
}

function lineElementFrom(view: EditorView, point: DomPoint): HTMLElement | null {
  const { node, offset } = point;
  let at: Node | null =
    isElement(node) && !node.classList.contains("cm-line") ? (node.childNodes[offset] ?? node) : node;
  for (; at && at !== view.contentDOM; at = at.parentNode) {
    if (isElement(at) && at.classList.contains("cm-line")) return at;
  }
  return null;
}

function lineElementAt(view: EditorView, from: number, to: number): HTMLElement | null {
  return lineElementFrom(view, view.domAtPos(from, 1)) ?? lineElementFrom(view, view.domAtPos(to, -1));
}

function pointIn(view: EditorView, line: HTMLElement, pos: number, side: -1 | 1): DomPoint | null {
  const point = view.domAtPos(pos, side);
  return line.contains(point.node) ? point : null;
}

function contentBoxes(element: Element): Box[] {
  const range = element.ownerDocument.createRange();
  range.selectNodeContents(element);
  return Array.from(range.getClientRects(), boxOf).filter(isVisibleBox);
}

// A widget drawn as a block inside its line (an image under its markup)
// reports a box as wide as the line; its content is what is on screen.
function widgetContentBoxes(range: Range, line: HTMLElement, boxes: Box[], column: Span): Box[] {
  const wide = (column.right - column.left) * BLOCK_CONTAINER_SHARE;
  if (!boxes.some((box) => box.right - box.left >= wide)) return boxes;
  let result = boxes;
  for (const child of Array.from(line.children)) {
    if ((child as HTMLElement).contentEditable !== "false" || !range.intersectsNode(child)) continue;
    result = replaceBoxes(result, Array.from(child.getClientRects(), boxOf), contentBoxes(child));
  }
  return result;
}

function boxesBetween(line: HTMLElement, from: DomPoint, to: DomPoint, column: Span): Box[] {
  const range = line.ownerDocument.createRange();
  range.setStart(from.node, from.offset);
  range.setEnd(to.node, to.offset);
  if (range.collapsed) return [];
  return widgetContentBoxes(range, line, Array.from(range.getClientRects(), boxOf), column);
}

const spaceWidths = new Map<string, number>();
let measureContext: CanvasRenderingContext2D | null | undefined;

function spaceWidth(style: CSSStyleDeclaration, doc: Document): number {
  const font = `${style.fontStyle} ${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
  const known = spaceWidths.get(font);
  if (known !== undefined) return known;
  if (measureContext === undefined) {
    measureContext =
      typeof CanvasRenderingContext2D === "function" ? doc.createElement("canvas").getContext("2d") : null;
  }
  let width = pixels(style.fontSize) * SPACE_EM_FALLBACK;
  if (measureContext) {
    measureContext.font = font;
    const measured = measureContext.measureText(" ").width;
    if (measured > 0) width = measured;
  }
  spaceWidths.set(font, width);
  return width;
}

function linePieces(
  view: EditorView,
  part: BlockInfo,
  range: SelectionRange,
  newline: boolean,
  column: Span,
): Box[] {
  const from = Math.max(range.from, part.from);
  const to = Math.min(range.to, part.to);
  if (from > to || (from === to && !newline)) return [];
  const line = lineElementAt(view, part.from, part.to);
  if (!line) return [];

  const lineStart: DomPoint = { node: line, offset: 0 };
  const lineEnd: DomPoint = { node: line, offset: line.childNodes.length };
  const start = from > part.from ? (pointIn(view, line, from, 1) ?? lineStart) : lineStart;
  const end = to < part.to ? (pointIn(view, line, to, -1) ?? lineEnd) : lineEnd;
  const selected = from < to ? boxesBetween(line, start, end, column) : [];
  const unselected = [
    ...(from > part.from ? boxesBetween(line, lineStart, start, column) : []),
    ...(to < part.to ? boxesBetween(line, end, lineEnd, column) : []),
  ];

  const style = getComputedStyle(line);
  const rect = line.getBoundingClientRect();
  const rtl = style.direction === "rtl";
  const empty = !selected.some(isVisibleBox) && !unselected.some(isVisibleBox);
  const caret = newline && empty ? view.coordsAtPos(part.from, 1) : null;
  return piecesForLine(
    {
      selected,
      unselected,
      content: {
        left: rect.left + pixels(style.paddingLeft) * view.scaleX,
        right: rect.right - pixels(style.paddingRight) * view.scaleX,
        top: rect.top,
        bottom: rect.bottom,
      },
      rtl,
      wraps: view.lineWrapping,
      newline,
      newlineWidth: newline ? spaceWidth(style, line.ownerDocument) * view.scaleX : 0,
      emptyStart: caret ? (rtl ? caret.right : caret.left) : null,
    },
    column,
  );
}

function widgetElementFor(view: EditorView, part: BlockInfo, documentTop: number): HTMLElement | null {
  const { node, offset } = view.domAtPos(part.from, 1);
  const child = node.childNodes[offset];
  if (!isElement(child) || child.classList.contains("cm-line")) return null;
  const rect = child.getBoundingClientRect();
  const fits =
    Math.abs(rect.top - (documentTop + part.top)) <= WIDGET_TOLERANCE_PX &&
    Math.abs(rect.bottom - (documentTop + part.bottom)) <= WIDGET_TOLERANCE_PX;
  return fits ? child : null;
}

function widgetPieces(view: EditorView, part: BlockInfo, column: Span, documentTop: number): Box[] {
  const element = widgetElementFor(view, part, documentTop);
  const box: Box = element
    ? boxOf(element.getBoundingClientRect())
    : { ...column, top: documentTop + part.top, bottom: documentTop + part.bottom };
  return clipToColumn([box], column);
}

function coversWidget(range: SelectionRange, part: BlockInfo): boolean {
  return part.from < part.to
    ? range.from < part.to && range.to > part.from
    : range.from <= part.from && range.to >= part.to;
}

function blockPieces(
  view: EditorView,
  block: BlockInfo,
  range: SelectionRange,
  column: Span,
  documentTop: number,
): Box[] {
  const parts = Array.isArray(block.type) ? (block.type as readonly BlockInfo[]) : [block];
  const lineBreak = block.to < view.state.doc.length && range.from <= block.to && range.to > block.to;
  const pieces: Box[] = [];
  parts.forEach((part, index) => {
    if (part.type === BlockType.Text) {
      pieces.push(...linePieces(view, part, range, lineBreak && index === parts.length - 1, column));
    } else if (coversWidget(range, part)) {
      pieces.push(...widgetPieces(view, part, column, documentTop));
    }
  });
  return pieces;
}

/**
 * The selection's pieces over the viewport, one per visual row of each
 * selected line and one per selected block widget, in layer coordinates.
 */
export function selectionMarkers(view: EditorView): LayerMarker[] {
  const ranges = view.state.selection.ranges.filter((range) => !range.empty);
  const blocks = view.viewportLineBlocks;
  if (ranges.length === 0 || blocks.length === 0) return [];
  const column = textColumn(view);
  const origin = layerOrigin(view);
  const documentTop = view.documentTop;
  const pieces: Box[] = [];
  let first = 0;
  for (const range of ranges) {
    while (first < blocks.length && blocks[first].to < range.from) first++;
    for (let index = first; index < blocks.length && blocks[index].from <= range.to; index++) {
      pieces.push(...blockPieces(view, blocks[index], range, column, documentTop));
    }
  }
  return pieces.map(
    (piece) =>
      new RectangleMarker(
        SELECTION_PIECE_CLASS,
        piece.left - origin.left,
        piece.top - origin.top,
        piece.right - piece.left,
        piece.bottom - piece.top,
      ),
  );
}

const writSelectionLayer = layer({
  above: true,
  class: SELECTION_LAYER_CLASS,
  markers: selectionMarkers,
  update: (update) => update.docChanged || update.selectionSet || update.viewportChanged,
});

// drawSelection stays for its cursor layer and for hiding the native
// selection; only its selection layer is replaced.
const selectionStructure = EditorView.baseTheme({
  ".cm-selectionLayer": { display: "none" },
  [`.${SELECTION_LAYER_CLASS}`]: { pointerEvents: "none" },
});

function focusIsInContent(view: EditorView): boolean {
  return view.contentDOM.contains(view.root.activeElement);
}

// A click on chrome that cannot select (user-select: none) moves focus but
// leaves the DOM selection in the content, where WebKit still types, pastes
// and runs Select All. The selection is dropped when focus leaves for another
// element; a window losing focus keeps the active element and the caret.
function dropSelectionLeftInContent(view: EditorView): void {
  if (focusIsInContent(view)) return;
  const selection = view.contentDOM.ownerDocument.getSelection();
  if (!selection || selection.rangeCount === 0) return;
  if (view.contentDOM.contains(selection.anchorNode) || view.contentDOM.contains(selection.focusNode)) {
    selection.removeAllRanges();
  }
}

// Focus that comes back by Tab or element.focus() finds no DOM selection in
// the content, so WebKit would put the caret at the start and scroll to it.
// Writing the state's selection during the focus event keeps both.
function restoreSelectionOnFocus(view: EditorView): void {
  const selection = view.contentDOM.ownerDocument.getSelection();
  if (selection && selection.rangeCount > 0 && view.contentDOM.contains(selection.anchorNode)) return;
  view.focus();
}

// WebKit's Select All selects and focuses the editable root holding the DOM
// selection; it fires a cancelable selectstart on that root first, as does a
// drag that starts outside the editor and enters the content.
const selectionFocusGuard: Extension = [
  EditorView.domEventObservers({
    blur: (_event, view) => dropSelectionLeftInContent(view),
    focus: (_event, view) => restoreSelectionOnFocus(view),
  }),
  EditorView.domEventHandlers({
    selectstart: (_event, view) => view.state.facet(EditorView.editable) && !focusIsInContent(view),
  }),
];

/**
 * The editor's selection: CodeMirror's cursors and native-selection hiding,
 * with one layer painting the selected characters' boxes per visual row.
 * Selecting and typing reach the note only while focus is in it.
 */
export function writSelection(): Extension {
  return [drawSelection(), writSelectionLayer, selectionStructure, selectionFocusGuard];
}
