import { type Extension } from "@codemirror/state";
import {
  EditorView,
  ViewPlugin,
  ViewUpdate,
  Decoration,
  WidgetType,
  type DecorationSet,
  type PluginValue,
} from "@codemirror/view";
import { syntaxTree } from "@codemirror/language";
import {
  imageReferenceAt,
  inlineImageField,
  InlineImageWidget,
  type InlineImageDeps,
  type InlineImageState,
  inlineImages,
} from "./markdown-images";
import { fenceCollapse, fenceCollapsible } from "./markdown-fences";

// Minimal structural types matching @lezer/common — avoids a direct import
// of @lezer/common which is not in the direct dependency list.
interface SyntaxNodeRef {
  readonly name: string;
  readonly from: number;
  readonly to: number;
  readonly node: SyntaxNodeFull;
}

interface SyntaxNodeFull extends SyntaxNodeRef {
  readonly firstChild: SyntaxNodeFull | null;
  readonly nextSibling: SyntaxNodeFull | null;
  readonly parent: SyntaxNodeFull | null;
}

// ─── Decoration factories ──────────────────────────────────────────────────

const lineDec: Record<string, Decoration> = {
  ATXHeading1: Decoration.line({ class: "cm-line-md-h1 cm-line-md-hang" }),
  ATXHeading2: Decoration.line({ class: "cm-line-md-h2 cm-line-md-hang" }),
  ATXHeading3: Decoration.line({ class: "cm-line-md-h3 cm-line-md-hang" }),
  ATXHeading4: Decoration.line({ class: "cm-line-md-h4 cm-line-md-hang" }),
  ATXHeading5: Decoration.line({ class: "cm-line-md-h5 cm-line-md-hang" }),
  ATXHeading6: Decoration.line({ class: "cm-line-md-h6 cm-line-md-hang" }),
};

// Fenced code keeps mono: prose sans is the writing face, and a fence is code
// (ADR-030 decision 7). Applied per line so the fences read as part of it; the
// edge lines carry the corner radius and the block's vertical padding, so the
// run of lines reads as one slab rather than a stack of filled rows.
const codeBlockLine = Decoration.line({ class: "cm-md-codeblock" });
const codeBlockFirstLine = Decoration.line({ class: "cm-md-codeblock cm-md-codeblock-first" });
const codeBlockLastLine = Decoration.line({ class: "cm-md-codeblock cm-md-codeblock-last" });
const codeBlockSoleLine = Decoration.line({
  class: "cm-md-codeblock cm-md-codeblock-first cm-md-codeblock-last",
});

// Pulls a line's leading marker into the left margin without taking it out of
// flow, so a wrapped line keeps one box and CodeMirror's height cache stays
// right (an absolutely-positioned marker inside .cm-line does not).
const hangLine = Decoration.line({ class: "cm-line-md-hang" });

// Blocks the editor does not render: a table and an html block are styled
// where they are written.
const tableLine = Decoration.line({ class: "cm-md-table" });
const htmlLine = Decoration.line({ class: "cm-md-html" });

const markDecByNode: Record<string, Decoration> = {
  StrongEmphasis: Decoration.mark({ class: "cm-md-strong" }),
  Emphasis:       Decoration.mark({ class: "cm-md-em" }),
  Strikethrough:  Decoration.mark({ class: "cm-md-strike" }),
  InlineCode:     Decoration.mark({ class: "cm-md-code" }),
};

const markerReplace  = Decoration.replace({});
// Markers that stay in the text hang in the left margin at the formatting ink,
// so the sentence itself never carries raw punctuation (ADR-030 decision 3).
const hungMarkerMark = Decoration.mark({ class: "cm-md-marker-hung" });
const urlDimMark     = Decoration.mark({ class: "cm-md-url-dim" });
const linkTextMark   = Decoration.mark({ class: "cm-md-link-text" });
const blockquoteMark = Decoration.mark({ class: "cm-md-blockquote" });
const calloutMarkerMark = Decoration.mark({ class: "cm-md-callout-marker" });
const listNumMark    = Decoration.mark({ class: "cm-md-list-num" });
const markerDimMark  = Decoration.mark({ class: "cm-md-marker-dim" });
const codeInfoMark   = Decoration.mark({ class: "cm-md-code-info" });
const taskDoneMark   = Decoration.mark({ class: "cm-md-task-done" });
const tableDelimiterMark = Decoration.mark({ class: "cm-md-table-delimiter" });

// ─── Widgets ───────────────────────────────────────────────────────────────

const SVG_NS = "http://www.w3.org/2000/svg";

class TaskCheckboxWidget extends WidgetType {
  constructor(readonly checked: boolean) {
    super();
  }

  override eq(other: TaskCheckboxWidget): boolean {
    return other.checked === this.checked;
  }

  toDOM(): HTMLElement {
    // The box is drawn by the wrapper and the tick by an inline SVG, because a
    // native checkbox paints in the platform's own colours and ignores the
    // preset. The real input stays on top at zero opacity so the control keeps
    // checkbox semantics and the existing mousedown handler keeps working.
    const box = document.createElement("span");
    box.className = "cm-md-task-box";
    if (this.checked) box.dataset.checked = "true";

    const input = document.createElement("input");
    input.type = "checkbox";
    input.className = "cm-md-task-checkbox";
    input.checked = this.checked;
    input.setAttribute("aria-label", this.checked ? "Completed task" : "Open task");
    box.appendChild(input);

    if (this.checked) {
      const svg = document.createElementNS(SVG_NS, "svg");
      svg.setAttribute("viewBox", "0 0 16 16");
      svg.setAttribute("aria-hidden", "true");
      svg.setAttribute("class", "cm-md-task-check");
      const path = document.createElementNS(SVG_NS, "path");
      path.setAttribute("d", "M3.75 8.5 6.6 11.35 12.25 5");
      path.setAttribute("fill", "none");
      path.setAttribute("stroke", "currentColor");
      path.setAttribute("stroke-width", "2");
      path.setAttribute("stroke-linecap", "round");
      path.setAttribute("stroke-linejoin", "round");
      svg.appendChild(path);
      box.appendChild(svg);
    }

    return box;
  }

  override ignoreEvent(): boolean {
    return false;
  }
}

class BulletWidget extends WidgetType {
  toDOM(): HTMLElement {
    const dot = document.createElement("span");
    dot.className = "cm-md-bullet";
    dot.textContent = "•";
    return dot;
  }
}

class HorizontalRuleWidget extends WidgetType {
  toDOM(): HTMLElement {
    const rule = document.createElement("span");
    rule.className = "cm-md-hr";
    return rule;
  }
}

const bulletReplace = Decoration.replace({ widget: new BulletWidget() });
const hrReplace = Decoration.replace({ widget: new HorizontalRuleWidget() });
const taskCheckedReplace = Decoration.replace({ widget: new TaskCheckboxWidget(true) });
const taskUncheckedReplace = Decoration.replace({ widget: new TaskCheckboxWidget(false) });

// The marker prefix of a task-list line: indentation, a bullet or ordered
// marker, whitespace, then the checkbox brackets.
const TASK_LINE_PREFIX = /^(\s*(?:[-+*]|\d+[.)])\s+\[)([ xX])\]/;
// The remainder of a list line that makes its marker a task item's marker.
const TASK_AFTER_MARK = /^\s+\[[ xX]\]/;
// The leading '>' of a quoted line, up to and including the marker itself.
const QUOTE_LINE_MARK = /^\s*>/;
// Whitespace between a marker and the text it introduces.
const LEADING_SPACE = /^[^\S\n]*/;

// Inline noise: characters like '**', '_', '`', '[', ']' that carry no meaning
// once the styling they describe is rendered. Replaced on inactive lines.
const MARKER_NAMES = new Set([
  "EmphasisMark",
  "CodeMark",
  "StrikethroughMark",
  "LinkMark",
]);

// Structure: the '#' of a heading and the '>' of a quote say what the block is,
// so they stay readable and hang in the margin instead of being replaced. A
// deliberate narrowing of ADR-014's replace-on-inactive rule.
const HUNG_MARKER_NAMES = new Set(["HeaderMark", "QuoteMark"]);

/**
 * The end of the address run of a link or image: its url, plus the optional
 * quoted title that follows it inside the same parentheses. A title left on
 * its own reads as stray prose, so it goes with the url. The run stops before
 * the closing ')', which the marker rule replaces on its own.
 *
 * CommonMark lets the title sit on a later line than the url, and a plugin may
 * not replace a line break, so a run that leaves `lineEnd` falls back to the
 * url alone.
 */
function addressEndAfter(url: SyntaxNodeFull, lineEnd: number): number {
  const title = url.nextSibling;
  if (title === null || title.name !== "LinkTitle") return url.to;
  const closing = title.nextSibling;
  const end = closing !== null && closing.name === "LinkMark" ? closing.from : title.to;
  return end <= lineEnd ? end : url.to;
}

/**
 * Whether `link` is the `[!type]` that opens an Obsidian callout, which the
 * grammar reads as a shortcut reference link.
 *
 * The rule is the preview's (writ-render's `callout.rs` and `header_at`): the
 * first thing in the first paragraph of a quote, a non-empty type after the
 * `!`, and nothing after the `]` that makes it an inline or full reference
 * link. A fold marker or a title after it is the text that follows the node.
 */
function isCalloutMarker(
  link: SyntaxNodeFull,
  docSlice: (from: number, to: number) => string,
): boolean {
  const paragraph = link.parent;
  if (paragraph === null || paragraph.name !== "Paragraph" || paragraph.from !== link.from) {
    return false;
  }
  const quote = paragraph.parent;
  if (quote === null || quote.name !== "Blockquote") return false;
  let firstBlock = quote.firstChild;
  while (firstBlock !== null && firstBlock.name === "QuoteMark") {
    firstBlock = firstBlock.nextSibling;
  }
  if (firstBlock === null || firstBlock.from !== paragraph.from) return false;
  const open = link.firstChild;
  const close = open?.nextSibling ?? null;
  if (
    open?.name !== "LinkMark" ||
    close === null ||
    close.name !== "LinkMark" ||
    close.nextSibling !== null
  ) {
    return false;
  }
  const label = docSlice(open.to, close.from);
  return label.startsWith("!") && label.slice(1).trim() !== "";
}

// ─── Pure decoration builder ───────────────────────────────────────────────

export interface DecorationSpec {
  from: number;
  to: number;
  decoration: Decoration;
}

const NO_IMAGES: ReadonlyMap<string, InlineImageState> = new Map();
const NO_LINES: ReadonlySet<number> = new Set();

/**
 * The start of every line a selection touches, clipped to the visible range.
 *
 * A selection range covers whole lines, not two end points: a block selected
 * from the middle of one line to the middle of another shows its markup on
 * every line between them, not only on the two ends.
 *
 * The clip is not an optimisation. Without it, selecting a 2 MB document
 * marks every line in it active and the builder walks the whole document to
 * paint one screen.
 */
export function activeLineStarts(
  ranges: readonly { from: number; to: number }[],
  docLineAt: (pos: number) => { from: number; to: number; number: number },
  visibleFrom: number,
  visibleTo: number,
): Set<number> {
  const starts = new Set<number>();
  for (const range of ranges) {
    const from = Math.max(range.from, visibleFrom);
    const to = Math.min(range.to, visibleTo);
    if (from > to) continue;
    let pos = from;
    for (;;) {
      let line: { from: number; to: number };
      try {
        line = docLineAt(pos);
      } catch {
        break;
      }
      starts.add(line.from);
      if (line.to >= to) break;
      pos = line.to + 1;
    }
  }
  return starts;
}

/**
 * Builds decoration specs for the given visible range of a markdown document.
 *
 * Pure function: takes syntax tree iteration and document queries, returns an
 * ordered array of DecorationSpec objects. No side effects; safe to call in
 * unit tests without a DOM.
 *
 * @param iterateTree  Calls the callback for each node in [from, to).
 * @param docLineAt    Returns the line at a document position.
 * @param docSlice     Returns the document text in [from, to).
 * @param activeLineFroms  Start positions of the lines a selection touches;
 *                         markers on those lines are revealed, not replaced.
 *                         Built by {@link activeLineStarts}.
 * @param visibleFrom  Start of the visible range.
 * @param visibleTo    End of the visible range.
 * @param images       What is known about each authored image reference.
 */
export function buildMarkdownDecorations(
  iterateTree: (from: number, to: number, cb: (node: SyntaxNodeRef) => boolean | void) => void,
  docLineAt: (pos: number) => { from: number; to: number; number: number },
  docSlice: (from: number, to: number) => string,
  activeLineFroms: ReadonlySet<number>,
  visibleFrom: number,
  visibleTo: number,
  images: ReadonlyMap<string, InlineImageState> = NO_IMAGES,
): DecorationSpec[] {
  const specs: DecorationSpec[] = [];

  // Tracks replaced [from,to) intervals to prevent overlaps.
  const replacedRanges: Array<[number, number]> = [];

  function wouldOverlap(from: number, to: number): boolean {
    for (const [a, b] of replacedRanges) {
      if (from < b && to > a) return true;
    }
    return false;
  }

  function addReplace(from: number, to: number, decoration: Decoration = markerReplace) {
    if (from >= to) return;
    if (wouldOverlap(from, to)) return;
    replacedRanges.push([from, to]);
    specs.push({ from, to, decoration });
  }

  function isActiveLine(pos: number): boolean | null {
    try {
      return activeLineFroms.has(docLineAt(pos).from);
    } catch {
      return null;
    }
  }

  function addMark(from: number, to: number, dec: Decoration) {
    if (from >= to) return;
    specs.push({ from, to, decoration: dec });
  }

  function addressRunEnd(url: SyntaxNodeFull): number {
    try {
      return addressEndAfter(url, docLineAt(url.from).to);
    } catch {
      return url.to;
    }
  }

  // Which of a fenced block's own lines the fence field takes off screen.
  // Empty for a block that keeps its fences: one that never closed, one that
  // opens the document, and one whose fences are being edited.
  function collapsedFenceLines(node: SyntaxNodeFull): ReadonlySet<number> {
    if (node.name !== "FencedCode") return NO_LINES;
    const fences = fenceCollapsible(node, docLineAt);
    if (!fences) return NO_LINES;
    if (activeLineFroms.has(fences.open.from) || activeLineFroms.has(fences.close.from)) {
      return NO_LINES;
    }
    return new Set([fences.open.from, fences.close.from]);
  }

  function isCollapsedFenceLine(fence: SyntaxNodeFull, pos: number): boolean {
    try {
      return collapsedFenceLines(fence).has(docLineAt(pos).from);
    } catch {
      return false;
    }
  }

  iterateTree(visibleFrom, visibleTo, (nodeRef) => {
    const { from, to, name } = nodeRef;

    // ── Heading line decorations ──────────────────────────────────────────
    if (name in lineDec) {
      try {
        const line = docLineAt(from);
        specs.push({ from: line.from, to: line.from, decoration: lineDec[name] });
      } catch {
        // skip un-parseable positions
      }
      return; // children (HeaderMark) handled by the MARKER_NAMES branch below
    }

    // ── Inline mark decorations ───────────────────────────────────────────
    if (name in markDecByNode) {
      addMark(from, to, markDecByNode[name]);
      return;
    }

    // ── Fenced code: mono, one line decoration per line in the block ──────
    if (name === "FencedCode" || name === "CodeBlock") {
      try {
        // A collapsed fence line is merged into the line above it, so a slab
        // decoration for it would paint that line instead. The edges move
        // onto the content lines, which are the whole block once the fences
        // are gone.
        const collapsed = collapsedFenceLines(nodeRef.node);
        const lines: Array<{ from: number }> = [];
        let pos = from;
        for (;;) {
          const line = docLineAt(pos);
          if (!collapsed.has(line.from)) lines.push({ from: line.from });
          if (line.to >= to) break;
          pos = line.to + 1;
        }
        lines.forEach((line, index) => {
          const isFirst = index === 0;
          const isLast = index === lines.length - 1;
          const decoration = isFirst && isLast
            ? codeBlockSoleLine
            : isFirst
              ? codeBlockFirstLine
              : isLast
                ? codeBlockLastLine
                : codeBlockLine;
          specs.push({ from: line.from, to: line.from, decoration });
        });
      } catch {
        // skip un-parseable positions
      }
      return;
    }

    // ── Fence markers and info string: demoted, never removed ─────────────
    // Replacing them (the inline-code rule) left the opening line showing a
    // bare info string and the closing line showing nothing but fill.
    if (name === "CodeMark" && nodeRef.node.parent?.name === "FencedCode") {
      const active = isActiveLine(from);
      if (active !== false) return;
      if (isCollapsedFenceLine(nodeRef.node.parent, from)) return;
      addMark(from, to, markerDimMark);
      return;
    }

    if (name === "CodeInfo") {
      const active = isActiveLine(from);
      if (active !== false) return;
      const fence = nodeRef.node.parent;
      if (fence && isCollapsedFenceLine(fence, from)) return;
      addMark(from, to, codeInfoMark);
      return;
    }

    // ── Blocks that stay as source ────────────────────────────────────────
    // A table needs a column-width pass over the whole block and a replace
    // spanning its line breaks per row, which leaves no way to edit the text
    // inside it. Arbitrary html inside the editor webview would need a hole
    // in the window's own policy, which is what the preview pane is for.
    // Both are styled where they are written, pipes and tags included.
    if (name === "Table" || name === "HTMLBlock") {
      const lineClass = name === "Table" ? tableLine : htmlLine;
      try {
        let pos = from;
        for (;;) {
          const line = docLineAt(pos);
          specs.push({ from: line.from, to: line.from, decoration: lineClass });
          if (line.to >= to) break;
          pos = line.to + 1;
        }
      } catch {
        // skip un-parseable positions
      }
      return;
    }

    if (name === "TableDelimiter") {
      addMark(from, to, tableDelimiterMark);
      return;
    }

    // ── Blockquote content ────────────────────────────────────────────────
    if (name === "Blockquote") {
      try {
        let pos = from;
        for (;;) {
          const line = docLineAt(pos);
          specs.push({ from: line.from, to: line.from, decoration: hangLine });
          // The rail is the mark's left border, so the mark starts after the
          // '>' the hang pulls into the margin: spanning the marker would drag
          // the rail out there with it.
          const marker = QUOTE_LINE_MARK.exec(docSlice(line.from, line.to));
          addMark(line.from + (marker ? marker[0].length : 0), line.to, blockquoteMark);
          if (line.to >= to) break;
          pos = line.to + 1;
        }
      } catch {
        // skip un-parseable positions
      }
      return;
    }

    // ── Link: styled label text + dimmed URL on inactive lines ───────────
    if (name === "Link") {
      // Access the full SyntaxNode to walk children.
      const fullNode = nodeRef.node;
      if (isCalloutMarker(fullNode, docSlice)) {
        // Skipping the children keeps the brackets the marker rule would hide.
        addMark(from, to, calloutMarkerMark);
        return false;
      }
      let child = fullNode.firstChild;
      let labelFrom = -1;
      let labelTo = -1;
      let urlFrom = -1;
      let addressTo = -1;
      let inLabel = false;

      while (child) {
        if (child.name === "LinkMark") {
          if (!inLabel && labelFrom === -1) {
            labelFrom = child.to; // position after opening '['
            inLabel = true;
          } else if (inLabel) {
            labelTo = child.from; // position before closing ']'
            inLabel = false;
          }
        } else if (child.name === "URL") {
          urlFrom = child.from;
          addressTo = addressRunEnd(child);
        }
        child = child.nextSibling;
      }

      if (labelFrom >= 0 && labelTo > labelFrom) {
        addMark(labelFrom, labelTo, linkTextMark);
      }
      if (urlFrom >= 0 && addressTo > urlFrom) {
        // The label alone reads as the link, which is what makes the line
        // prose rather than markup. The address comes back on the active line,
        // dimmed, so it stays legible while it is being edited.
        const active = isActiveLine(urlFrom);
        if (active === false) addReplace(urlFrom, addressTo);
        else if (active === true) addMark(urlFrom, addressTo, urlDimMark);
      }
      return;
    }

    // ── Image: the picture goes under the line, the source is hidden ──────
    if (name === "Image") {
      const reference = imageReferenceAt(nodeRef.node, docSlice);
      if (reference === null) return;
      try {
        const line = docLineAt(from);
        specs.push({
          from: line.to,
          to: line.to,
          decoration: Decoration.widget({
            widget: new InlineImageWidget(reference, images.get(reference) ?? { status: "pending" }),
            side: 1,
          }),
        });
      } catch {
        // skip un-parseable positions
      }
      return;
    }

    // ── Task checkboxes: widget on inactive lines, raw on active ──────────
    if (name === "TaskMarker") {
      const checked = /[xX]/.test(docSlice(from, to));
      if (checked) {
        // Done state, not markup: the strike stays on the active line too.
        try {
          const line = docLineAt(from);
          const gap = LEADING_SPACE.exec(docSlice(to, line.to));
          addMark(to + (gap ? gap[0].length : 0), line.to, taskDoneMark);
        } catch {
          // skip un-parseable positions
        }
      }
      const active = isActiveLine(from);
      if (active !== false) return;
      addReplace(from, to, checked ? taskCheckedReplace : taskUncheckedReplace);
      return;
    }

    // ── List marks: bullet dot / hidden task dash / muted numbers ─────────
    if (name === "ListMark") {
      const text = docSlice(from, to);
      if (/^[-+*]$/.test(text)) {
        // ADR-014's rule stands for a replace: hiding the character under the
        // cursor would hide what the user is typing. Only the marks below are
        // narrowed.
        const active = isActiveLine(from);
        if (active !== false) return;
        let lineTo: number;
        try {
          lineTo = docLineAt(from).to;
        } catch {
          return;
        }
        const rest = docSlice(to, lineTo);
        const taskRest = TASK_AFTER_MARK.exec(rest);
        if (taskRest) {
          // A task item shows only its checkbox; swallow the gap up to the box
          // so no stray indent remains.
          addReplace(from, to + taskRest[0].indexOf("["));
        } else {
          addReplace(from, to, bulletReplace);
        }
      } else if (/\d/.test(text)) {
        addMark(from, to, listNumMark);
      }
      return;
    }

    // ── Horizontal rules: drawn as a rule on inactive lines ───────────────
    if (name === "HorizontalRule") {
      const active = isActiveLine(from);
      if (active !== false) return;
      addReplace(from, to, hrReplace);
      return;
    }

    // ── Autolinks: bare urls read as links. URLs inside a Link or Image are
    // handled by the Link branch above (label styling + dimming). ──────────
    if (name === "URL") {
      const parent = nodeRef.node.parent;
      if (parent && parent.name === "Link") return;
      if (parent && parent.name === "Image") {
        // The picture under the line says what the source is; the path above
        // it says it twice.
        if (isActiveLine(from) === false) addReplace(from, addressRunEnd(nodeRef.node));
        return;
      }
      addMark(from, to, linkTextMark);
      return;
    }

    // ── Structural markers: hung in the margin, never replaced ────────────
    if (HUNG_MARKER_NAMES.has(name)) {
      addMark(from, to, hungMarkerMark);
      return;
    }

    // ── Inline markers: replace on inactive lines, reveal on active ───────
    if (MARKER_NAMES.has(name)) {
      const active = isActiveLine(from);
      if (active !== false) return;
      addReplace(from, to);
    }
  });

  // CM6 requires decorations sorted by range start; line decs (to === from)
  // must precede mark decs at the same position.
  specs.sort((a, b) => {
    if (a.from !== b.from) return a.from - b.from;
    const aIsLine = a.to === a.from;
    const bIsLine = b.to === b.from;
    if (aIsLine && !bIsLine) return -1;
    if (!aIsLine && bIsLine) return 1;
    return a.to - b.to;
  });

  return specs;
}

// ─── ViewPlugin ───────────────────────────────────────────────────────────

function buildDecorationSet(view: EditorView): DecorationSet {
  const { state } = view;
  const tree = syntaxTree(state);
  const images = state.field(inlineImageField, false) ?? NO_IMAGES;
  const allSpecs: DecorationSpec[] = [];

  for (const { from, to } of view.visibleRanges) {
    const rangeSpecs = buildMarkdownDecorations(
      (vf, vt, cb) => tree.iterate({ from: vf, to: vt, enter: cb }),
      (pos) => state.doc.lineAt(pos),
      (sliceFrom, sliceTo) => state.doc.sliceString(sliceFrom, sliceTo),
      activeLineStarts(state.selection.ranges, (pos) => state.doc.lineAt(pos), from, to),
      from,
      to,
      images,
    );
    allSpecs.push(...rangeSpecs);
  }

  allSpecs.sort((a, b) => {
    if (a.from !== b.from) return a.from - b.from;
    const aIsLine = a.to === a.from;
    const bIsLine = b.to === b.from;
    if (aIsLine && !bIsLine) return -1;
    if (!aIsLine && bIsLine) return 1;
    return a.to - b.to;
  });

  // true = let CM sort; avoids "Ranges must be added sorted" for same-from pairs.
  return Decoration.set(allSpecs.map((s) => s.decoration.range(s.from, s.to)), true);
}

class MarkdownTypographyPlugin implements PluginValue {
  decorations: DecorationSet;

  constructor(view: EditorView) {
    this.decorations = buildDecorationSet(view);
  }

  update(update: ViewUpdate) {
    if (
      update.docChanged ||
      update.viewportChanged ||
      update.selectionSet ||
      update.transactions.some((tr) => tr.reconfigured) ||
      // Bytes for an image arrive in their own transaction, which changes
      // neither the document nor the selection.
      update.startState.field(inlineImageField, false) !==
        update.state.field(inlineImageField, false)
    ) {
      this.decorations = buildDecorationSet(update.view);
    }
  }
}

/**
 * Flips the task checkbox on the line at `pos` between `[ ]` and `[x]`.
 * The document is the only state; the widget re-renders from the new text.
 */
export function toggleTaskAt(view: EditorView, pos: number): boolean {
  let line: { from: number; text: string };
  try {
    line = view.state.doc.lineAt(pos);
  } catch {
    return false;
  }
  const match = TASK_LINE_PREFIX.exec(line.text);
  if (!match) return false;
  const boxPos = line.from + match[1].length;
  view.dispatch({
    changes: { from: boxPos, to: boxPos + 1, insert: match[2] === " " ? "x" : " " },
    userEvent: "input",
  });
  return true;
}

/**
 * Handles a mousedown on a rendered task checkbox. Exported for tests; wired
 * through EditorView.domEventHandlers below.
 */
export function handleTaskMousedown(event: MouseEvent, view: EditorView): boolean {
  if (event.button !== 0) return false;
  const target = event.target;
  if (
    !(target instanceof HTMLInputElement) ||
    !target.classList.contains("cm-md-task-checkbox")
  ) {
    return false;
  }
  // Keep the selection where it is: moving the cursor onto the line would
  // make it active and dissolve the widget under the pointer.
  event.preventDefault();
  return toggleTaskAt(view, view.posAtDOM(target));
}

const taskClickHandler = EditorView.domEventHandlers({
  mousedown: handleTaskMousedown,
});

export const markdownTypographyPlugin: Extension = [
  ViewPlugin.fromClass(MarkdownTypographyPlugin, { decorations: (v) => v.decorations }),
  taskClickHandler,
];

/**
 * The markdown decorations plus the images they draw.
 *
 * One builder and one plugin: a second decoration source over the same
 * ranges cannot see the first one's replaced ranges, and CodeMirror refuses
 * overlapping replacements.
 */
export function markdownInlineExtension(deps: InlineImageDeps): Extension {
  return [markdownTypographyPlugin, inlineImages(deps), fenceCollapse];
}
