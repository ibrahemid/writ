import { type Extension } from "@codemirror/state";
import {
  EditorView,
  ViewPlugin,
  ViewUpdate,
  Decoration,
  type DecorationSet,
  type PluginValue,
} from "@codemirror/view";
import { syntaxTree } from "@codemirror/language";

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
}

// ─── Decoration factories ──────────────────────────────────────────────────

const lineDec: Record<string, Decoration> = {
  ATXHeading1: Decoration.line({ class: "cm-line-md-h1" }),
  ATXHeading2: Decoration.line({ class: "cm-line-md-h2" }),
  ATXHeading3: Decoration.line({ class: "cm-line-md-h3" }),
  ATXHeading4: Decoration.line({ class: "cm-line-md-h4" }),
  ATXHeading5: Decoration.line({ class: "cm-line-md-h5" }),
  ATXHeading6: Decoration.line({ class: "cm-line-md-h6" }),
};

const markDecByNode: Record<string, Decoration> = {
  StrongEmphasis: Decoration.mark({ class: "cm-md-strong" }),
  Emphasis:       Decoration.mark({ class: "cm-md-em" }),
  Strikethrough:  Decoration.mark({ class: "cm-md-strike" }),
  InlineCode:     Decoration.mark({ class: "cm-md-code" }),
};

const markerReplace  = Decoration.replace({});
const urlDimMark     = Decoration.mark({ class: "cm-md-url-dim" });
const linkTextMark   = Decoration.mark({ class: "cm-md-link-text" });
const blockquoteMark = Decoration.mark({ class: "cm-md-blockquote" });

// Syntax marker node names — characters like '#', '**', '_', '`', '>', '[', ']',
// '(', ')' that we dim/replace on inactive lines.
const MARKER_NAMES = new Set([
  "HeaderMark",
  "EmphasisMark",
  "CodeMark",
  "StrikethroughMark",
  "QuoteMark",
  "LinkMark",
]);

// ─── Pure decoration builder ───────────────────────────────────────────────

export interface DecorationSpec {
  from: number;
  to: number;
  decoration: Decoration;
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
 * @param cursorPositions  Set of cursor head positions; markers on lines
 *                         containing any cursor are revealed (not replaced).
 * @param visibleFrom  Start of the visible range.
 * @param visibleTo    End of the visible range.
 */
export function buildMarkdownDecorations(
  iterateTree: (from: number, to: number, cb: (node: SyntaxNodeRef) => boolean | void) => void,
  docLineAt: (pos: number) => { from: number; to: number; number: number },
  cursorPositions: ReadonlySet<number>,
  visibleFrom: number,
  visibleTo: number,
): DecorationSpec[] {
  const activeLineFroms = new Set<number>();
  for (const pos of cursorPositions) {
    try {
      activeLineFroms.add(docLineAt(pos).from);
    } catch {
      // pos out of range — skip
    }
  }

  const specs: DecorationSpec[] = [];

  // Tracks replaced [from,to) intervals to prevent overlaps.
  const replacedRanges: Array<[number, number]> = [];

  function wouldOverlap(from: number, to: number): boolean {
    for (const [a, b] of replacedRanges) {
      if (from < b && to > a) return true;
    }
    return false;
  }

  function addReplace(from: number, to: number) {
    if (from >= to) return;
    if (wouldOverlap(from, to)) return;
    replacedRanges.push([from, to]);
    specs.push({ from, to, decoration: markerReplace });
  }

  function addMark(from: number, to: number, dec: Decoration) {
    if (from >= to) return;
    specs.push({ from, to, decoration: dec });
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

    // ── Blockquote content ────────────────────────────────────────────────
    if (name === "Blockquote") {
      addMark(from, to, blockquoteMark);
      return;
    }

    // ── Link: styled label text + dimmed URL on inactive lines ───────────
    if (name === "Link") {
      // Access the full SyntaxNode to walk children.
      const fullNode = nodeRef.node;
      let child = fullNode.firstChild;
      let labelFrom = -1;
      let labelTo = -1;
      let urlFrom = -1;
      let urlTo = -1;
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
          urlTo = child.to;
        }
        child = child.nextSibling;
      }

      if (labelFrom >= 0 && labelTo > labelFrom) {
        addMark(labelFrom, labelTo, linkTextMark);
      }
      if (urlFrom >= 0 && urlTo > urlFrom) {
        try {
          const lineFr = docLineAt(urlFrom).from;
          if (!activeLineFroms.has(lineFr)) {
            addMark(urlFrom, urlTo, urlDimMark);
          }
        } catch {
          // skip
        }
      }
      return;
    }

    // ── Syntax markers: replace on inactive lines, reveal on active ───────
    if (MARKER_NAMES.has(name)) {
      let lineFr: number;
      try {
        lineFr = docLineAt(from).from;
      } catch {
        return;
      }
      if (activeLineFroms.has(lineFr)) return;
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
  const cursorPositions = new Set(
    state.selection.ranges.flatMap((r) => [r.head, r.anchor]),
  );
  const allSpecs: DecorationSpec[] = [];

  for (const { from, to } of view.visibleRanges) {
    const rangeSpecs = buildMarkdownDecorations(
      (vf, vt, cb) => tree.iterate({ from: vf, to: vt, enter: cb }),
      (pos) => state.doc.lineAt(pos),
      cursorPositions,
      from,
      to,
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
      update.transactions.some((tr) => tr.reconfigured)
    ) {
      this.decorations = buildDecorationSet(update.view);
    }
  }
}

export const markdownTypographyPlugin: Extension = ViewPlugin.fromClass(
  MarkdownTypographyPlugin,
  { decorations: (v) => v.decorations },
);
