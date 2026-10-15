import { StateEffect, StateField, type EditorState, type Extension } from "@codemirror/state";
import {
  Decoration,
  EditorView,
  ViewPlugin,
  WidgetType,
  type DecorationSet,
  type PluginValue,
  type ViewUpdate,
} from "@codemirror/view";
import { foldedRanges, syntaxTree } from "@codemirror/language";
import { mathSource } from "./markdown-math";
import { TYPING_PAUSE_MS } from "./markdown-images";
import type { BlockRenderer, BlockTheme } from "./block-renderer";
import { sanitizedFragment } from "./sanitize-markup";

// Minimal structural types matching @lezer/common, mirroring the shape used
// by editor/markdown-typography.ts so this module keeps out of the direct
// dependency list.
interface SyntaxNodeLike {
  readonly name: string;
  readonly from: number;
  readonly to: number;
  readonly firstChild: SyntaxNodeLike | null;
  readonly lastChild: SyntaxNodeLike | null;
  readonly nextSibling: SyntaxNodeLike | null;
}

// ─── Keys ──────────────────────────────────────────────────────────────────

/**
 * A 53-bit hash of `text` and its length (cyrb53). A block is known by what
 * it holds, so the same table, expression or diagram anywhere, in any tab,
 * shares one drawing and one measured height.
 */
export function blockHash(text: string): string {
  let h1 = 0xdeadbeef;
  let h2 = 0x41c6ce57;
  for (let i = 0; i < text.length; i++) {
    const code = text.charCodeAt(i);
    h1 = Math.imul(h1 ^ code, 2654435761);
    h2 = Math.imul(h2 ^ code, 1597334677);
  }
  h1 = Math.imul(h1 ^ (h1 >>> 16), 2246822507);
  h1 ^= Math.imul(h2 ^ (h2 >>> 13), 3266489909);
  h2 = Math.imul(h2 ^ (h2 >>> 16), 2246822507);
  h2 ^= Math.imul(h1 ^ (h1 >>> 13), 3266489909);
  const hash = 4294967296 * (2097151 & h2) + (h1 >>> 0);
  return `${hash.toString(36)}.${text.length.toString(36)}`;
}

export function mathKey(tex: string, display: boolean): string {
  return `math-${display ? "d" : "i"}:${blockHash(tex)}`;
}

export function diagramKey(source: string, theme: BlockTheme): string {
  return `diagram-${theme}:${blockHash(source)}`;
}

function tableKey(text: string): string {
  return `table:${blockHash(text)}`;
}

/** The diagram theme for the editor's polarity. */
export function blockThemeOf(state: EditorState): BlockTheme {
  return state.facet(EditorView.darkTheme) ? "dark" : "light";
}

// ─── What has been drawn ───────────────────────────────────────────────────

/** Where one math expression or diagram's drawing stands. */
export type BlockRender =
  | { status: "pending" }
  | { status: "ready"; markup: string }
  | { status: "failed"; line: number | null };

const PENDING: BlockRender = { status: "pending" };
const NO_RENDERS: ReadonlyMap<string, BlockRender> = new Map();

/** Records how a drawing came out. */
export const setBlockRenders = StateEffect.define<ReadonlyArray<{ key: string; render: BlockRender }>>();

/** Forgets every drawing but the ones named. */
export const keepBlockRenders = StateEffect.define<ReadonlySet<string>>();

/**
 * The drawings this editor has, by key. A field rather than plugin state: a
 * drawing outlives the update that asked for it, and both the block widgets
 * and the inline math read it.
 */
export const blockRenderField = StateField.define<ReadonlyMap<string, BlockRender>>({
  create: () => NO_RENDERS,
  update(value, tr) {
    let next: Map<string, BlockRender> | null = null;
    for (const effect of tr.effects) {
      if (effect.is(setBlockRenders)) {
        next ??= new Map(value);
        for (const { key, render } of effect.value) next.set(key, render);
      } else if (effect.is(keepBlockRenders)) {
        next ??= new Map(value);
        for (const key of [...next.keys()]) if (!effect.value.has(key)) next.delete(key);
      }
    }
    return next ?? value;
  },
});

/** The drawing for `key`, pending until it arrives. */
export function blockRenderOf(state: EditorState, key: string): BlockRender {
  return state.field(blockRenderField, false)?.get(key) ?? PENDING;
}

// ─── Finding the blocks ────────────────────────────────────────────────────

/** One inline run of a table cell. */
export type CellRun =
  | { kind: "text"; text: string }
  | { kind: "code"; text: string }
  | { kind: "math"; tex: string; display: boolean; key: string }
  | { kind: "strong" | "em" | "strike" | "link"; runs: CellRun[] };

export type CellAlign = "left" | "center" | "right" | null;

/** A table as its widget draws it. */
export interface TableModel {
  align: CellAlign[];
  head: CellRun[][];
  rows: CellRun[][][];
  /** Where each cell's text starts, counted from the start of the table's first line. */
  offsets: { head: number[]; rows: number[][] };
  /** The math the cells hold, by key. */
  math: string[];
  /** Equal for two tables that draw the same. */
  signature: string;
}

export type MarkdownBlockKind = "table" | "math" | "diagram";

/** One block Inline draws in place of its source. */
export interface MarkdownBlock {
  kind: MarkdownBlockKind;
  /** Start of the block's first line: a block widget takes whole lines. */
  from: number;
  /** End of its last line. */
  to: number;
  /** Its key: the hash of what it is drawn from. */
  key: string;
  /** The expression or the diagram source; the table's text for a table. */
  source: string;
  table?: TableModel;
}

// Containers a block can sit inside and still be drawn. A quote is left out:
// a block widget takes whole lines, and the quote's markers are on them.
const CONTAINERS = new Set(["Document", "BulletList", "OrderedList", "ListItem"]);

const MARK_NAMES = new Set([
  "EmphasisMark",
  "CodeMark",
  "StrikethroughMark",
  "LinkMark",
  "InlineMathMark",
]);

// `[[Target]]`, `[[Target#Heading]]` and `[[Target|Alias]]` read as the name
// they show.
const WIKILINK = /\[\[([^\]|#]*)(?:#[^\]|]*)?(?:\|([^\]]*))?\]\]/g;

interface ChildLike {
  readonly name: string;
  readonly from: number;
  readonly to: number;
  readonly nextSibling: ChildLike | null;
}

/** True for a fenced block whose info string names Mermaid. */
export function isMermaidFence(
  fence: { readonly firstChild: ChildLike | null },
  docSlice: (from: number, to: number) => string,
): boolean {
  for (let child = fence.firstChild; child; child = child.nextSibling) {
    if (child.name !== "CodeInfo") continue;
    const first = docSlice(child.from, child.to).trim().split(/\s+/)[0] ?? "";
    return first.toLowerCase() === "mermaid";
  }
  return false;
}

function cellText(text: string): string {
  return text.replace(WIKILINK, (_match, target: string, alias?: string) =>
    (alias ?? target).trim(),
  );
}

/** The inside of an inline node: what lies between its first and last mark. */
function innerOf(node: SyntaxNodeLike): { from: number; to: number } {
  const first = node.firstChild;
  const last = node.lastChild;
  const from = first && MARK_NAMES.has(first.name) ? first.to : node.from;
  const to = last && last !== first && MARK_NAMES.has(last.name) ? last.from : node.to;
  return { from, to: Math.max(from, to) };
}

function runsIn(
  slice: (from: number, to: number) => string,
  parent: SyntaxNodeLike,
  from: number,
  to: number,
): CellRun[] {
  const runs: CellRun[] = [];
  // Plain text gathers across escapes before it is read, so a `[[a\|b]]`
  // the escape split in two still reads as one link.
  let pending = "";
  let pos = from;
  const gather = (end: number) => {
    if (end > pos) pending += slice(pos, end);
  };
  const flush = () => {
    if (pending !== "") runs.push({ kind: "text", text: cellText(pending) });
    pending = "";
  };
  for (let child = parent.firstChild; child; child = child.nextSibling) {
    if (child.to <= from || child.from >= to) continue;
    gather(child.from);
    pos = child.to;
    if (MARK_NAMES.has(child.name)) continue;
    if (child.name === "Escape") {
      pending += slice(child.from + 1, child.to);
      continue;
    }
    // The grammar reads the inside of `[[…]]` as a bracketed link; the
    // wikilink is text here, read whole below.
    if (
      child.name === "Link" &&
      slice(child.from - 1, child.from) === "[" &&
      slice(child.to, child.to + 1) === "]"
    ) {
      pending += slice(child.from, child.to).replace(/\\(.)/g, "$1");
      continue;
    }
    flush();
    runs.push(...runFor(slice, child));
  }
  gather(to);
  flush();
  return runs;
}

function runFor(slice: (from: number, to: number) => string, node: SyntaxNodeLike): CellRun[] {
  const inner = innerOf(node);
  switch (node.name) {
    case "StrongEmphasis":
      return [{ kind: "strong", runs: runsIn(slice, node, inner.from, inner.to) }];
    case "Emphasis":
      return [{ kind: "em", runs: runsIn(slice, node, inner.from, inner.to) }];
    case "Strikethrough":
      return [{ kind: "strike", runs: runsIn(slice, node, inner.from, inner.to) }];
    case "InlineCode":
      return [{ kind: "code", text: slice(inner.from, inner.to) }];
    case "InlineMath": {
      const { tex, display } = mathSource(slice(node.from, node.to));
      return [{ kind: "math", tex, display, key: mathKey(tex, display) }];
    }
    case "Link": {
      // The label: between the opening `[` and the `]` that closes it.
      const open = node.firstChild;
      let close = open?.nextSibling ?? null;
      while (close && close.name !== "LinkMark") close = close.nextSibling;
      if (!open || !close) return [{ kind: "text", text: slice(node.from, node.to) }];
      return [{ kind: "link", runs: runsIn(slice, node, open.to, close.from) }];
    }
    case "Image": {
      const open = node.firstChild;
      let close = open?.nextSibling ?? null;
      while (close && close.name !== "LinkMark") close = close.nextSibling;
      return open && close ? [{ kind: "text", text: slice(open.to, close.from) }] : [];
    }
    case "URL":
    case "Autolink":
      return [{ kind: "link", runs: [{ kind: "text", text: slice(inner.from, inner.to) }] }];
    default:
      return [{ kind: "text", text: cellText(slice(node.from, node.to)) }];
  }
}

/**
 * The cells of one table row, one per column the row's pipes mark out, each
 * with where its text starts: an empty cell starts just past its pipe.
 */
function rowCells(row: SyntaxNodeLike): Array<{ cell: SyntaxNodeLike | null; at: number }> {
  const pipes: SyntaxNodeLike[] = [];
  const cells: SyntaxNodeLike[] = [];
  for (let child = row.firstChild; child; child = child.nextSibling) {
    if (child.name === "TableDelimiter") pipes.push(child);
    else if (child.name === "TableCell") cells.push(child);
  }
  const segments: Array<[number, number]> = [];
  let start = row.from;
  for (const pipe of pipes) {
    segments.push([start, pipe.from]);
    start = pipe.to;
  }
  segments.push([start, row.to]);
  if (pipes.length > 0 && pipes[0].from === row.from) segments.shift();
  if (pipes.length > 0 && pipes[pipes.length - 1].to === row.to) segments.pop();
  return segments.map(([from, to]) => {
    const cell = cells.find((candidate) => candidate.from >= from && candidate.to <= to) ?? null;
    return { cell, at: cell ? cell.from : Math.min(from + 1, to) };
  });
}

function alignmentsOf(delimiterRow: string): CellAlign[] {
  let text = delimiterRow.trim();
  if (text.startsWith("|")) text = text.slice(1);
  if (text.endsWith("|")) text = text.slice(0, -1);
  return text.split("|").map((cell) => {
    const spec = cell.trim();
    const left = spec.startsWith(":");
    const right = spec.endsWith(":");
    if (left && right) return "center";
    if (right) return "right";
    if (left) return "left";
    return null;
  });
}

// Tables are rebuilt from their text only when it changes, which keeps a
// keystroke elsewhere from walking every cell on screen. Singleton state:
// the models are pure functions of the text.
const KEPT_TABLES = 512;
const tableModels = new Map<string, TableModel>();

function tableModel(
  slice: (from: number, to: number) => string,
  table: SyntaxNodeLike,
  key: string,
  base: number,
): TableModel {
  const held = tableModels.get(key);
  if (held) return held;
  let header: SyntaxNodeLike | null = null;
  let delimiter = "";
  const bodyRows: SyntaxNodeLike[] = [];
  for (let child = table.firstChild; child; child = child.nextSibling) {
    if (child.name === "TableHeader") header = child;
    else if (child.name === "TableDelimiter") delimiter = slice(child.from, child.to);
    else if (child.name === "TableRow") bodyRows.push(child);
  }
  const cellRuns = ({ cell }: { cell: SyntaxNodeLike | null }): CellRun[] =>
    cell ? runsIn(slice, cell, cell.from, cell.to) : [];
  const headCells = header ? rowCells(header) : [];
  const head = headCells.map(cellRuns);
  const columns = head.length;
  const rowOffsets: number[][] = [];
  const rows = bodyRows.map((row) => {
    const found = rowCells(row).slice(0, columns);
    const cells = found.map(cellRuns);
    const offsets = found.map(({ at }) => at - base);
    // A short row's missing cells take the caret to the end of the row.
    while (cells.length < columns) {
      cells.push([]);
      offsets.push(row.to - base);
    }
    rowOffsets.push(offsets);
    return cells;
  });
  const align = alignmentsOf(delimiter).slice(0, columns);
  while (align.length < columns) align.push(null);
  const math: string[] = [];
  const collect = (runs: CellRun[]) => {
    for (const run of runs) {
      if (run.kind === "math") math.push(run.key);
      else if ("runs" in run) collect(run.runs);
    }
  };
  for (const cell of head) collect(cell);
  for (const row of rows) for (const cell of row) collect(cell);
  const model: TableModel = {
    align,
    head,
    rows,
    offsets: { head: headCells.map(({ at }) => at - base), rows: rowOffsets },
    math,
    signature: JSON.stringify([align, head, rows]),
  };
  tableModels.set(key, model);
  if (tableModels.size > KEPT_TABLES) {
    const oldest = tableModels.keys().next().value;
    if (oldest !== undefined) tableModels.delete(oldest);
  }
  return model;
}

/** The text of a fence's body, without the fence's own indentation. */
function fenceBody(state: EditorState, fence: SyntaxNodeLike): string | null {
  let marks = 0;
  let last: SyntaxNodeLike | null = null;
  for (let child = fence.firstChild; child; child = child.nextSibling) {
    if (child.name !== "CodeMark") continue;
    marks++;
    last = child;
  }
  if (marks < 2 || !last) return null;
  const open = state.doc.lineAt(fence.from);
  const close = state.doc.lineAt(last.from);
  if (close.number <= open.number) return null;
  const indent = fence.from - open.from;
  const lines: string[] = [];
  for (let n = open.number + 1; n < close.number; n++) {
    const text = state.doc.line(n).text;
    let cut = 0;
    while (cut < indent && text.charCodeAt(cut) === 32) cut++;
    lines.push(text.slice(cut));
  }
  return lines.join("\n");
}

function blockAt(state: EditorState, node: SyntaxNodeLike): MarkdownBlock | null {
  const slice = (from: number, to: number) => state.doc.sliceString(from, to);
  const first = state.doc.lineAt(node.from);
  const last = state.doc.lineAt(node.to);
  // A block that shares its first line with a list marker keeps its source:
  // the widget would take the marker with it.
  if (slice(first.from, node.from).trim() !== "") return null;
  switch (node.name) {
    case "Table": {
      const text = slice(first.from, last.to);
      const key = tableKey(text);
      return {
        kind: "table",
        from: first.from,
        to: last.to,
        key,
        source: text,
        table: tableModel(slice, node, key, first.from),
      };
    }
    case "BlockMath": {
      const { tex } = mathSource(slice(node.from, node.to));
      if (tex === "") return null;
      return { kind: "math", from: first.from, to: last.to, key: mathKey(tex, true), source: tex };
    }
    case "FencedCode": {
      if (!isMermaidFence(node, slice)) return null;
      const body = fenceBody(state, node);
      // An empty diagram draws nothing in the preview either.
      if (body === null || body.trim() === "") return null;
      return { kind: "diagram", from: first.from, to: last.to, key: blockHash(body), source: body };
    }
    default:
      return null;
  }
}

/** Every table, display math block and Mermaid diagram in `[from, to)`. */
export function markdownBlocksIn(state: EditorState, from: number, to: number): MarkdownBlock[] {
  const blocks: MarkdownBlock[] = [];
  syntaxTree(state).iterate({
    from,
    to,
    enter: (ref: { name: string; node: SyntaxNodeLike }) => {
      if (ref.name === "Table" || ref.name === "BlockMath" || ref.name === "FencedCode") {
        const block = blockAt(state, ref.node);
        if (block) blocks.push(block);
        return false;
      }
      return CONTAINERS.has(ref.name);
    },
  });
  return blocks;
}

// ─── Widgets ───────────────────────────────────────────────────────────────

// Each block's last measured height, by its key, for CodeMirror's height map
// while the block is off screen. Singleton state: a height belongs to what a
// block holds, whichever tab shows it.
const KEPT_HEIGHTS = 2048;
const heights = new Map<string, number>();

function rememberHeight(key: string, height: number): void {
  heights.delete(key);
  heights.set(key, height);
  if (heights.size > KEPT_HEIGHTS) {
    const oldest = heights.keys().next().value;
    if (oldest !== undefined) heights.delete(oldest);
  }
}

/** The height last measured for a block with this key, when there is one. */
export function measuredHeight(key: string): number | undefined {
  return heights.get(key);
}

/**
 * A block drawn in place of its source. CodeMirror measures it once it is on
 * screen; the height is kept by key and handed back as the estimate wherever
 * the block is laid out unseen, so scrolling past it does not move the page.
 *
 * A click inside it goes to the editor, which puts the caret in the block and
 * brings its source back.
 */
abstract class BlockWidget extends WidgetType {
  constructor(readonly key: string) {
    super();
  }

  override get estimatedHeight(): number {
    return heights.get(this.key) ?? -1;
  }

  override ignoreEvent(_event: Event): boolean {
    return false;
  }

  protected frame(view: EditorView, kind: string, status: string): HTMLElement {
    const frame = document.createElement("div");
    frame.className = `cm-md-block cm-md-${kind}`;
    frame.dataset.status = status;
    const key = this.key;
    view.requestMeasure({
      read: () => (frame.isConnected ? frame.getBoundingClientRect().height : 0),
      write: (height) => {
        if (height > 0 && status !== "pending") rememberHeight(key, height);
      },
    });
    return frame;
  }
}

function sourceFallback(frame: HTMLElement, source: string, caption: string): void {
  const pre = document.createElement("pre");
  pre.className = "cm-md-block-source";
  pre.textContent = source;
  frame.appendChild(pre);
  const note = document.createElement("span");
  note.className = "cm-md-block-caption";
  note.textContent = caption;
  frame.appendChild(note);
}

/** True when two drawings would put the same thing on screen. */
export function sameRender(a: BlockRender, b: BlockRender): boolean {
  if (a.status === "ready") return b.status === "ready" && a.markup === b.markup;
  if (a.status === "failed") return b.status === "failed" && a.line === b.line;
  return b.status === "pending";
}

/** What a failed diagram says under its source. */
export function diagramFailureCaption(line: number | null): string {
  return line === null
    ? "This diagram has an error."
    : `This diagram has an error on line ${line}.`;
}

export const MATH_FAILURE_CAPTION = "This math could not be drawn.";

class MathBlockWidget extends BlockWidget {
  constructor(
    key: string,
    readonly tex: string,
    readonly render: BlockRender,
  ) {
    super(key);
  }

  override eq(other: MathBlockWidget): boolean {
    return other.key === this.key && sameRender(other.render, this.render);
  }

  toDOM(view: EditorView): HTMLElement {
    const frame = this.frame(view, "math-block", this.render.status);
    if (this.render.status === "ready") {
      frame.append(sanitizedFragment(this.render.markup));
    } else if (this.render.status === "failed") {
      sourceFallback(frame, this.tex, MATH_FAILURE_CAPTION);
    } else {
      const pending = document.createElement("code");
      pending.className = "cm-md-math-pending";
      pending.textContent = this.tex;
      frame.appendChild(pending);
    }
    return frame;
  }
}

class DiagramWidget extends BlockWidget {
  constructor(
    key: string,
    readonly source: string,
    readonly render: BlockRender,
  ) {
    super(key);
  }

  override eq(other: DiagramWidget): boolean {
    return other.key === this.key && sameRender(other.render, this.render);
  }

  toDOM(view: EditorView): HTMLElement {
    const frame = this.frame(view, "diagram", this.render.status);
    if (this.render.status === "ready") {
      frame.append(sanitizedFragment(this.render.markup));
    } else if (this.render.status === "failed") {
      sourceFallback(frame, this.source, diagramFailureCaption(this.render.line));
    } else {
      // The space the diagram took last time, so its arrival moves nothing.
      const known = heights.get(this.key);
      if (known !== undefined) frame.style.minHeight = `${known}px`;
      frame.setAttribute("aria-busy", "true");
    }
    return frame;
  }
}

function appendRuns(parent: HTMLElement, runs: CellRun[], math: ReadonlyMap<string, BlockRender>): void {
  for (const run of runs) {
    switch (run.kind) {
      case "text":
        parent.appendChild(document.createTextNode(run.text));
        break;
      case "code": {
        const code = document.createElement("code");
        code.className = "cm-md-code";
        code.textContent = run.text;
        parent.appendChild(code);
        break;
      }
      case "math": {
        const render = math.get(run.key) ?? PENDING;
        const span = document.createElement("span");
        span.className = "cm-md-math-inline";
        if (render.status === "ready") span.append(sanitizedFragment(render.markup));
        else span.textContent = run.tex;
        parent.appendChild(span);
        break;
      }
      default: {
        const tag = { strong: "strong", em: "em", strike: "s", link: "span" }[run.kind];
        const element = document.createElement(tag);
        if (run.kind === "link") element.className = "cm-md-link-text";
        appendRuns(element, run.runs, math);
        parent.appendChild(element);
      }
    }
  }
}

class TableWidget extends BlockWidget {
  private readonly mathSignature: string;

  constructor(
    key: string,
    readonly model: TableModel,
    readonly math: ReadonlyMap<string, BlockRender>,
  ) {
    super(key);
    this.mathSignature = model.math
      .map((mathKey) => {
        const render = math.get(mathKey);
        return render?.status === "ready" ? render.markup.length : (render?.status ?? "-");
      })
      .join(",");
  }

  override eq(other: TableWidget): boolean {
    return (
      other.key === this.key &&
      other.model.signature === this.model.signature &&
      other.mathSignature === this.mathSignature
    );
  }

  toDOM(view: EditorView): HTMLElement {
    const frame = this.frame(view, "table-block", "ready");
    const table = document.createElement("table");
    table.className = "cm-md-table-widget";
    const cell = (tag: "th" | "td", runs: CellRun[], column: number, offset: number) => {
      const element = document.createElement(tag);
      const align = this.model.align[column];
      if (align) element.style.textAlign = align;
      element.dataset.offset = String(offset);
      appendRuns(element, runs, this.math);
      return element;
    };
    if (this.model.head.length > 0) {
      const head = document.createElement("thead");
      const row = document.createElement("tr");
      this.model.head.forEach((runs, column) =>
        row.appendChild(cell("th", runs, column, this.model.offsets.head[column])),
      );
      head.appendChild(row);
      table.appendChild(head);
    }
    if (this.model.rows.length > 0) {
      const body = document.createElement("tbody");
      this.model.rows.forEach((cells, index) => {
        const row = document.createElement("tr");
        cells.forEach((runs, column) =>
          row.appendChild(cell("td", runs, column, this.model.offsets.rows[index][column])),
        );
        body.appendChild(row);
      });
      table.appendChild(body);
    }
    frame.appendChild(table);
    // A click in a cell puts the caret in that cell's text, which brings the
    // table's source back with the caret where the click was.
    frame.addEventListener("mousedown", (event) => {
      if (event.button !== 0) return;
      const target = event.target instanceof Element ? event.target.closest("[data-offset]") : null;
      if (!(target instanceof HTMLElement)) return;
      event.preventDefault();
      const anchor = view.posAtDOM(frame) + Number(target.dataset.offset);
      view.dispatch({ selection: { anchor }, scrollIntoView: true, userEvent: "select.pointer" });
      view.focus();
    });
    return frame;
  }

  // A cell's own mousedown places the caret; anywhere else the editor does.
  override ignoreEvent(event: Event): boolean {
    return (
      event.type === "mousedown" &&
      event.target instanceof Element &&
      event.target.closest("[data-offset]") !== null
    );
  }
}

function widgetFor(block: MarkdownBlock, state: EditorState): WidgetType {
  const renders = state.field(blockRenderField, false) ?? NO_RENDERS;
  switch (block.kind) {
    case "table":
      return new TableWidget(block.key, block.table!, renders);
    case "math":
      return new MathBlockWidget(block.key, block.source, renders.get(block.key) ?? PENDING);
    case "diagram": {
      const key = diagramKey(block.source, blockThemeOf(state));
      return new DiagramWidget(key, block.source, renders.get(key) ?? PENDING);
    }
  }
}

// ─── The field ─────────────────────────────────────────────────────────────

/** True when a selection range touches `[from, to]`, either end included. */
export function selectionTouches(
  ranges: readonly { from: number; to: number }[],
  from: number,
  to: number,
): boolean {
  return ranges.some((range) => range.from <= to && range.to >= from);
}

/** True when a fold starts inside the block or ends inside it. */
function foldCutsInto(state: EditorState, block: { from: number; to: number }): boolean {
  let cuts = false;
  foldedRanges(state).between(block.from, block.to, (from, to) => {
    if ((from >= block.from && from <= block.to) || (to >= block.from && to < block.to)) {
      cuts = true;
      return false;
    }
    return undefined;
  });
  return cuts;
}

/** The stretch of the document the blocks are known for. */
export interface BlockWindow {
  from: number;
  to: number;
}

/** Moves the window the blocks are scanned for; dispatched by the view plugin. */
export const setBlockWindow = StateEffect.define<BlockWindow>();

interface BlockState {
  window: BlockWindow;
  blocks: MarkdownBlock[];
  /** The ranges drawn as widgets, in document order. */
  drawn: ReadonlyArray<{ from: number; to: number }>;
  decorations: DecorationSet;
}

function decorate(state: EditorState, window: BlockWindow, blocks: MarkdownBlock[]): BlockState {
  const ranges = [];
  const drawn: Array<{ from: number; to: number }> = [];
  for (const block of blocks) {
    // Source on the block the caret is in (ADR-014's rule, per block).
    if (selectionTouches(state.selection.ranges, block.from, block.to)) continue;
    // A fold that starts inside the block shows its first line and the fold.
    if (foldCutsInto(state, block)) continue;
    ranges.push(
      Decoration.replace({ widget: widgetFor(block, state), block: true }).range(block.from, block.to),
    );
    drawn.push({ from: block.from, to: block.to });
  }
  return { window, blocks, drawn, decorations: Decoration.set(ranges) };
}

function scan(state: EditorState, window: BlockWindow): BlockState {
  return decorate(state, window, markdownBlocksIn(state, window.from, window.to));
}

/**
 * Tables, display math and Mermaid diagrams, drawn in place while the caret
 * is outside them.
 *
 * A state field rather than a view plugin, because a block widget may not
 * come from a plugin. The viewport reaches it as an effect, as the fence
 * field's does: only the blocks near the screen are drawn, so a document of
 * a thousand tables costs what the screen holds.
 */
export const markdownBlockField: StateField<BlockState> = StateField.define<BlockState>({
  create(state) {
    return scan(state, { from: 0, to: Math.min(syntaxTree(state).length, state.doc.length) });
  },
  update(value, tr) {
    let window = value.window;
    for (const effect of tr.effects) {
      if (effect.is(setBlockWindow)) window = effect.value;
    }
    const moved = window !== value.window;
    if (moved || tr.docChanged || syntaxTree(tr.startState) !== syntaxTree(tr.state)) {
      if (!moved && tr.docChanged) {
        window = {
          from: tr.changes.mapPos(window.from, -1),
          to: tr.changes.mapPos(window.to, 1),
        };
      }
      return scan(tr.state, window);
    }
    if (
      !tr.startState.selection.eq(tr.state.selection) ||
      tr.startState.field(blockRenderField, false) !== tr.state.field(blockRenderField, false) ||
      foldedRanges(tr.startState) !== foldedRanges(tr.state) ||
      blockThemeOf(tr.startState) !== blockThemeOf(tr.state) ||
      tr.reconfigured
    ) {
      return decorate(tr.state, window, value.blocks);
    }
    return value;
  },
  provide: (field) => EditorView.decorations.from(field, (value) => value.decorations),
});

const NOTHING_DRAWN: ReadonlyArray<{ from: number; to: number }> = [];

/** The ranges drawn as block widgets: nothing else draws inside them. */
export function drawnBlocks(state: EditorState): ReadonlyArray<{ from: number; to: number }> {
  return state.field(markdownBlockField, false)?.drawn ?? NOTHING_DRAWN;
}

/** Tells the field which stretch of the document is near the screen. */
class BlockWindowPlugin implements PluginValue {
  private destroyed = false;

  constructor(private readonly view: EditorView) {
    this.publish();
  }

  update(update: ViewUpdate) {
    if (update.viewportChanged || update.docChanged) this.publish();
  }

  destroy() {
    this.destroyed = true;
  }

  private publish() {
    // The viewport, not the visible ranges: it reaches past the screen, so a
    // block is drawn before it scrolls into view rather than as it does.
    const { from, to } = this.view.viewport;
    const held = this.view.state.field(markdownBlockField, false)?.window;
    if (held && held.from === from && held.to === to) return;
    queueMicrotask(() => {
      if (this.destroyed) return;
      this.view.dispatch({ effects: setBlockWindow.of({ from, to }) });
    });
  }
}

// ─── Asking for drawings ───────────────────────────────────────────────────

/** How many drawings the field keeps before it forgets what is off screen. */
const KEPT_RENDERS = 256;

/** How long settled drawings wait to land together in one transaction. */
const SETTLE_BATCH_MS = 16;

interface Wanted {
  key: string;
  draw(): Promise<string>;
}

function failedLine(error: unknown): number | null {
  const line = (error as { line?: unknown } | null)?.line;
  return typeof line === "number" ? line : null;
}

/**
 * Asks the renderer for every expression and diagram near the screen, once
 * each, and records the answers. Emits no decorations: the block field and
 * the inline math read what arrives.
 *
 * While the document is being typed in, new sources wait for a pause of
 * [`TYPING_PAUSE_MS`]: a half-typed expression is not worth a drawing.
 */
class BlockRequester implements PluginValue {
  private readonly asked = new Set<string>();
  private typingPause: ReturnType<typeof setTimeout> | null = null;
  private settling: ReturnType<typeof setTimeout> | null = null;
  private settled: Array<{ key: string; render: BlockRender }> = [];
  private destroyed = false;

  constructor(
    private readonly view: EditorView,
    private readonly renderer: BlockRenderer,
  ) {
    this.scan();
  }

  update(update: ViewUpdate) {
    if (update.docChanged) {
      this.waitForTypingPause();
      return;
    }
    if (
      update.viewportChanged ||
      syntaxTree(update.startState) !== syntaxTree(update.state) ||
      blockThemeOf(update.startState) !== blockThemeOf(update.state)
    ) {
      this.scan();
    }
  }

  destroy() {
    this.destroyed = true;
    if (this.typingPause !== null) clearTimeout(this.typingPause);
    if (this.settling !== null) clearTimeout(this.settling);
  }

  private waitForTypingPause() {
    if (this.typingPause !== null) clearTimeout(this.typingPause);
    this.typingPause = setTimeout(() => {
      this.typingPause = null;
      if (!this.destroyed) this.scan();
    }, TYPING_PAUSE_MS);
  }

  private wanted(): Wanted[] {
    const { state } = this.view;
    const theme = blockThemeOf(state);
    const slice = (from: number, to: number) => state.doc.sliceString(from, to);
    const wanted: Wanted[] = [];
    const { from, to } = this.view.viewport;
    syntaxTree(state).iterate({
      from,
      to,
      enter: (ref: { name: string; from: number; to: number; node: SyntaxNodeLike }) => {
        if (ref.name === "InlineMath" || ref.name === "BlockMath") {
          const { tex, display } = mathSource(slice(ref.from, ref.to));
          const asDisplay = display || ref.name === "BlockMath";
          if (tex !== "") {
            wanted.push({
              key: mathKey(tex, asDisplay),
              draw: () => this.renderer.renderMath(tex, asDisplay),
            });
          }
          return false;
        }
        if (ref.name === "FencedCode") {
          if (isMermaidFence(ref.node, slice)) {
            const body = fenceBody(state, ref.node);
            if (body !== null && body.trim() !== "") {
              wanted.push({
                key: diagramKey(body, theme),
                draw: () => this.renderer.renderDiagram(body, theme),
              });
            }
          }
          return false;
        }
        return undefined;
      },
    });
    return wanted;
  }

  private scan() {
    if (this.typingPause !== null) return;
    const wanted = this.wanted();
    const held = this.view.state.field(blockRenderField, false) ?? NO_RENDERS;
    if (held.size > KEPT_RENDERS) {
      // Off-screen drawings go; the renderer still holds the recent ones.
      const keep = new Set(wanted.map((item) => item.key));
      this.asked.clear();
      for (const key of keep) if (held.has(key)) this.asked.add(key);
      queueMicrotask(() => {
        if (!this.destroyed) this.view.dispatch({ effects: keepBlockRenders.of(keep) });
      });
    }
    for (const item of wanted) {
      if (this.asked.has(item.key)) continue;
      this.asked.add(item.key);
      item.draw().then(
        (markup) => this.settle(item.key, { status: "ready", markup }),
        (error: unknown) => this.settle(item.key, { status: "failed", line: failedLine(error) }),
      );
    }
  }

  private settle(key: string, render: BlockRender) {
    if (this.destroyed) return;
    this.settled.push({ key, render });
    if (this.settling !== null) return;
    this.settling = setTimeout(() => {
      this.settling = null;
      if (this.destroyed || this.settled.length === 0) return;
      const batch = this.settled;
      this.settled = [];
      this.view.dispatch({ effects: setBlockRenders.of(batch) });
    }, SETTLE_BATCH_MS);
  }
}

/**
 * Tables, display math and diagrams drawn in place in Inline: the field that
 * holds the widgets, the window it reads, the drawings, and what asks for
 * them.
 */
export function markdownBlocks(renderer: BlockRenderer): Extension {
  return [
    blockRenderField,
    markdownBlockField,
    ViewPlugin.define((view) => new BlockWindowPlugin(view)),
    ViewPlugin.define((view) => new BlockRequester(view, renderer)),
  ];
}
