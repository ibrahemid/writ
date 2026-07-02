import {
  EditorSelection,
  EditorState,
  type SelectionRange,
  type StateCommand,
  type TransactionSpec,
} from "@codemirror/state";
import { syntaxTree } from "@codemirror/language";

interface InlineFormat {
  marker: string;
  nodeName: string;
  markName: string;
}

const BOLD: InlineFormat = { marker: "**", nodeName: "StrongEmphasis", markName: "EmphasisMark" };
const ITALIC: InlineFormat = { marker: "*", nodeName: "Emphasis", markName: "EmphasisMark" };
const STRIKE: InlineFormat = { marker: "~~", nodeName: "Strikethrough", markName: "StrikethroughMark" };
const CODE: InlineFormat = { marker: "`", nodeName: "InlineCode", markName: "CodeMark" };

const WRAP_ON_TYPE_MARKERS = new Set(["*", "_", "~", "`"]);

interface RangeResult {
  range: SelectionRange;
  changes?: { from: number; to?: number; insert?: string }[];
}

type EnclosingNode = ReturnType<ReturnType<typeof syntaxTree>["resolveInner"]>;

function findEnclosing(
  state: EditorState,
  from: number,
  to: number,
  nodeName: string,
): EnclosingNode | null {
  let node: EnclosingNode | null = syntaxTree(state).resolveInner(from, 1);
  while (node) {
    if (node.name === nodeName && node.from <= from && node.to >= to) return node;
    node = node.parent;
  }
  return null;
}

// True when the text is wrapped by exactly this marker: `**x**` matches for
// `**` but its inner text must not lead/trail with the marker character, so
// `**x**` never reads as a single-`*` wrap.
function exactWrapped(text: string, marker: string): boolean {
  if (text.length < marker.length * 2) return false;
  if (!text.startsWith(marker) || !text.endsWith(marker)) return false;
  const inner = text.slice(marker.length, text.length - marker.length);
  return !inner.startsWith(marker[0]) && !inner.endsWith(marker[0]);
}

function unwrapNode(node: EnclosingNode, format: InlineFormat, range: SelectionRange): RangeResult {
  const marks = node.getChildren(format.markName);
  const first = marks[0];
  const last = marks.length > 1 ? marks[marks.length - 1] : undefined;
  if (!first || !last) {
    // Malformed node — strip marker-length spans off both ends instead.
    const m = format.marker.length;
    return {
      changes: [
        { from: node.from, to: node.from + m },
        { from: node.to - m, to: node.to },
      ],
      range: range.empty
        ? EditorSelection.cursor(Math.max(node.from, range.head - m))
        : EditorSelection.range(node.from, node.to - 2 * m),
    };
  }
  const firstLen = first.to - first.from;
  const lastLen = last.to - last.from;
  const changes = [
    { from: first.from, to: first.to },
    { from: last.from, to: last.to },
  ];
  if (range.empty) {
    const head = Math.min(Math.max(node.from, range.head - firstLen), node.to - firstLen - lastLen);
    return { changes, range: EditorSelection.cursor(head) };
  }
  return {
    changes,
    range: EditorSelection.range(node.from, node.to - firstLen - lastLen),
  };
}

function wrapRange(from: number, to: number, marker: string): RangeResult {
  const m = marker.length;
  return {
    changes: [
      { from, insert: marker },
      { from: to, insert: marker },
    ],
    range: from === to
      ? EditorSelection.cursor(from + m)
      : EditorSelection.range(from + m, to + m),
  };
}

function toggleRange(state: EditorState, range: SelectionRange, format: InlineFormat): RangeResult {
  if (!range.empty) {
    const text = state.sliceDoc(range.from, range.to);
    if (exactWrapped(text, format.marker)) {
      const m = format.marker.length;
      return {
        changes: [
          { from: range.from, to: range.from + m },
          { from: range.to - m, to: range.to },
        ],
        range: EditorSelection.range(range.from, range.to - 2 * m),
      };
    }
    const node = findEnclosing(state, range.from, range.to, format.nodeName);
    if (node) return unwrapNode(node, format, range);
    return wrapRange(range.from, range.to, format.marker);
  }

  const node = findEnclosing(state, range.head, range.head, format.nodeName);
  if (node) return unwrapNode(node, format, range);

  const word = state.wordAt(range.head);
  if (word && !word.empty) {
    return {
      ...wrapRange(word.from, word.to, format.marker),
      range: EditorSelection.cursor(range.head + format.marker.length),
    };
  }

  return wrapRange(range.head, range.head, format.marker);
}

function toggleInline(format: InlineFormat): StateCommand {
  return ({ state, dispatch }) => {
    const spec = state.changeByRange((range) => toggleRange(state, range, format));
    dispatch(state.update(spec, { scrollIntoView: true, userEvent: "input" }));
    return true;
  };
}

export const toggleBold: StateCommand = toggleInline(BOLD);
export const toggleItalic: StateCommand = toggleInline(ITALIC);
export const toggleStrikethrough: StateCommand = toggleInline(STRIKE);
export const toggleInlineCode: StateCommand = toggleInline(CODE);

const URL_PATTERN = /^https?:\/\/\S+$/;

function linkRange(state: EditorState, range: SelectionRange): RangeResult {
  const link = findEnclosing(state, range.from, range.to, "Link");
  if (link) {
    const url = link.getChild("URL");
    if (url) return { range: EditorSelection.range(url.from, url.to) };
    return { range };
  }

  if (!range.empty) {
    const text = state.sliceDoc(range.from, range.to);
    if (URL_PATTERN.test(text)) {
      return {
        changes: [
          { from: range.from, insert: "[](" },
          { from: range.to, insert: ")" },
        ],
        range: EditorSelection.cursor(range.from + 1),
      };
    }
    return {
      changes: [
        { from: range.from, insert: "[" },
        { from: range.to, insert: "]()" },
      ],
      range: EditorSelection.cursor(range.to + 3),
    };
  }

  return {
    changes: [{ from: range.head, insert: "[]()" }],
    range: EditorSelection.cursor(range.head + 1),
  };
}

export const insertLink: StateCommand = ({ state, dispatch }) => {
  const spec = state.changeByRange((range) => linkRange(state, range));
  dispatch(state.update(spec, { scrollIntoView: true, userEvent: "input" }));
  return true;
};

/**
 * Wraps every selection range in the typed marker character. Returns null
 * when the input is not a markdown marker or any range is empty, so the
 * caller falls through to normal insertion.
 */
export function wrapOnType(state: EditorState, text: string): TransactionSpec | null {
  if (text.length !== 1 || !WRAP_ON_TYPE_MARKERS.has(text)) return null;
  if (state.selection.ranges.some((r) => r.empty)) return null;
  return {
    ...state.changeByRange((range) => wrapRange(range.from, range.to, text)),
    scrollIntoView: true,
    userEvent: "input.type",
  };
}
