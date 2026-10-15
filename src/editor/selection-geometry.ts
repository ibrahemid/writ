/** A box in client pixels. */
export interface Box {
  left: number;
  right: number;
  top: number;
  bottom: number;
}

/** A horizontal extent in client pixels. */
export interface Span {
  left: number;
  right: number;
}

/** A vertical extent in client pixels. */
export interface Band {
  top: number;
  bottom: number;
}

/** What one rendered line contributes to a selection, measured from its DOM. */
export interface LineMeasure {
  /** Boxes of the line's selected characters. */
  selected: readonly Box[];
  /** Boxes of the line's other characters. */
  unselected: readonly Box[];
  /** The line's box: inside its padding across, its full height down. */
  content: Box;
  rtl: boolean;
  /** Text wraps at the column; a line that does not wrap may run past it. */
  wraps: boolean;
  /** The line break after the line is inside the selection. */
  newline: boolean;
  /** One space in the line's font. */
  newlineWidth: number;
  /** Where an empty line starts (its caret position), when known. */
  emptyStart: number | null;
}

/** A character box and whether the selection holds it. */
export interface Item {
  box: Box;
  selected: boolean;
}

export interface Row extends Band {
  items: Item[];
}

/** Narrower than this, a box paints nothing (widget buffers, collapsed text). */
export const MIN_BOX_WIDTH = 0.5;
/** Boxes this close count as touching: subpixel seams between text runs. */
const TOUCH_PX = 0.5;

function center(top: number, bottom: number): number {
  return (top + bottom) / 2;
}

/** Whether a box paints anything. */
export function isVisibleBox(box: Box): boolean {
  return box.right - box.left >= MIN_BOX_WIDTH && box.bottom > box.top;
}

function sameBox(a: Box, b: Box): boolean {
  return (
    Math.abs(a.left - b.left) < 0.01 &&
    Math.abs(a.right - b.right) < 0.01 &&
    Math.abs(a.top - b.top) < 0.01 &&
    Math.abs(a.bottom - b.bottom) < 0.01
  );
}

function leftmost(boxes: readonly Box[]): number {
  return boxes.reduce((least, box) => Math.min(least, box.left), Infinity);
}

function rightmost(boxes: readonly Box[]): number {
  return boxes.reduce((most, box) => Math.max(most, box.right), -Infinity);
}

function widthOf(boxes: readonly Box[]): number {
  return boxes.length === 0 ? 0 : rightmost(boxes) - leftmost(boxes);
}

/**
 * Swaps a widget's own boxes for the boxes of its content when the content is
 * narrower: a block container inside a line spans the line, its image does not.
 */
export function replaceBoxes(boxes: readonly Box[], own: readonly Box[], content: readonly Box[]): Box[] {
  if (content.length === 0 || widthOf(content) >= widthOf(own) - 1) return [...boxes];
  const kept = boxes.filter((box) => !own.some((other) => sameBox(box, other)));
  return kept.length === boxes.length ? [...boxes] : [...kept, ...content];
}

/**
 * Splits a line's boxes into visual rows, top to bottom. A box joins a row
 * when either one's vertical center falls inside the other, so text of mixed
 * sizes on one row stays together and wrapped rows stay apart even when a
 * tight line height lets their glyph boxes overlap a little.
 */
export function groupRows(items: readonly Item[]): Row[] {
  const sorted = [...items].sort(
    (a, b) => center(a.box.top, a.box.bottom) - center(b.box.top, b.box.bottom),
  );
  const rows: Row[] = [];
  for (const item of sorted) {
    const row = rows[rows.length - 1];
    const itemCenter = center(item.box.top, item.box.bottom);
    if (
      row &&
      ((itemCenter >= row.top && itemCenter <= row.bottom) ||
        (center(row.top, row.bottom) >= item.box.top && center(row.top, row.bottom) <= item.box.bottom))
    ) {
      row.top = Math.min(row.top, item.box.top);
      row.bottom = Math.max(row.bottom, item.box.bottom);
      row.items.push(item);
    } else {
      rows.push({ top: item.box.top, bottom: item.box.bottom, items: [item] });
    }
  }
  return rows;
}

/**
 * The vertical extent each row paints: the first row from the line's top, the
 * last to its bottom, and a wrap boundary halfway between two rows' glyphs,
 * so the rows of one line and the lines after it meet without a gap.
 */
export function bandRows(rows: readonly Band[], content: Band): Band[] {
  return rows.map((row, index) => {
    const top = index === 0 ? content.top : center(rows[index - 1].bottom, row.top);
    const bottom = index === rows.length - 1 ? content.bottom : center(row.bottom, rows[index + 1].top);
    return { top, bottom: Math.max(top, bottom) };
  });
}

/**
 * Joins the selected boxes of one row into spans. Two boxes join when they
 * touch, or when none of the row's unselected characters sits between them
 * (the margin around a fold placeholder, hidden markup); a gap holding an
 * unselected character is a bidi split and stays open.
 */
export function mergeSpans(selected: readonly Box[], unselected: readonly Box[]): Span[] {
  const sorted = [...selected].sort((a, b) => a.left - b.left);
  const spans: Span[] = [];
  for (const box of sorted) {
    const last = spans[spans.length - 1];
    if (!last) {
      spans.push({ left: box.left, right: box.right });
      continue;
    }
    const gapFrom = last.right;
    const gapTo = box.left;
    const blocked =
      gapTo - gapFrom > TOUCH_PX &&
      unselected.some((other) => other.right > gapFrom + TOUCH_PX && other.left < gapTo - TOUCH_PX);
    if (blocked) spans.push({ left: box.left, right: box.right });
    else last.right = Math.max(last.right, box.right);
  }
  return spans;
}

/**
 * The newline box at a line's end edge, kept inside the line's box unless the
 * text already runs past it, where the box follows the text.
 */
function newlineSpan(edge: number, rtl: boolean, width: number, content: Box, follow: boolean): Span | null {
  const past = follow && (rtl ? edge < content.left - TOUCH_PX : edge > content.right + TOUCH_PX);
  const left = rtl ? edge - width : edge;
  const right = rtl ? edge : edge + width;
  const span = past ? { left, right } : { left: Math.max(content.left, left), right: Math.min(content.right, right) };
  return span.right - span.left >= MIN_BOX_WIDTH ? span : null;
}

function addSpan(spans: Span[], extra: Span): Span[] {
  const all = [...spans, extra].sort((a, b) => a.left - b.left);
  const joined: Span[] = [];
  for (const span of all) {
    const last = joined[joined.length - 1];
    if (last && span.left <= last.right + TOUCH_PX) last.right = Math.max(last.right, span.right);
    else joined.push({ ...span });
  }
  return joined;
}

/** Clips boxes to the text column, dropping any that end up empty. */
export function clipToColumn(boxes: readonly Box[], column: Span): Box[] {
  const clipped: Box[] = [];
  for (const box of boxes) {
    const left = Math.max(box.left, column.left);
    const right = Math.min(box.right, column.right);
    if (right - left >= MIN_BOX_WIDTH && box.bottom > box.top) clipped.push({ ...box, left, right });
  }
  return clipped;
}

/**
 * The selection pieces of one rendered line: per visual row, the selected
 * characters' boxes at the row's full height, plus a newline box one space
 * wide at the end of the line's last row when its line break is selected (at
 * the start of an empty line). A wrapping line paints inside the text column;
 * a line that does not wrap paints where its text is, past the column too.
 */
export function piecesForLine(line: LineMeasure, column: Span): Box[] {
  const items: Item[] = [
    ...line.selected.filter(isVisibleBox).map((box) => ({ box, selected: true })),
    ...line.unselected.filter(isVisibleBox).map((box) => ({ box, selected: false })),
  ];
  const { content } = line;

  if (items.length === 0) {
    if (!line.newline) return [];
    const start = line.emptyStart ?? (line.rtl ? content.right : content.left);
    const span = newlineSpan(start, line.rtl, line.newlineWidth, content, false);
    return span ? clipToColumn([{ ...span, top: content.top, bottom: content.bottom }], column) : [];
  }

  const rows = groupRows(items);
  const bands = bandRows(rows, content);
  const pieces: Box[] = [];
  rows.forEach((row, index) => {
    let spans = mergeSpans(
      row.items.filter((item) => item.selected).map((item) => item.box),
      row.items.filter((item) => !item.selected).map((item) => item.box),
    );
    if (line.newline && index === rows.length - 1) {
      const boxes = row.items.map((item) => item.box);
      const edge = line.rtl ? leftmost(boxes) : rightmost(boxes);
      const span = newlineSpan(edge, line.rtl, line.newlineWidth, content, !line.wraps);
      if (span) spans = addSpan(spans, span);
    }
    for (const span of spans) {
      pieces.push({ left: span.left, right: span.right, top: bands[index].top, bottom: bands[index].bottom });
    }
  });
  return line.wraps ? clipToColumn(pieces, column) : pieces;
}
