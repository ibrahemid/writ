// Math in Markdown: `$…$` inline and `$$…$$` display, read by the rules the
// preview's parser applies (pulldown-cmark with ENABLE_MATH), so the editor
// and the rendered note agree on what is math:
//
// - an opening `$` is followed by something other than whitespace, and a
//   closing `$` follows something other than whitespace; an inline span is
//   never empty, so a lone currency `$` stays text;
// - math holds no `$` of its own: the first one after the opener closes the
//   span or, when it cannot, leaves the opener as text;
// - `$$` opens display math, which the next `$$` closes;
// - a `$` inside braces belongs to the expression, and a backslash escapes
//   the character after it;
// - code spans and fences are code, so nothing in them is math.
//
// A paragraph that is nothing but one display span is a `BlockMath` block;
// display math with text around it stays inline. The parser itself is the
// editor grammar's (@lezer/markdown), which is not a direct dependency, so the
// shapes it calls with are named structurally here.

const DOLLAR = 36;
const BACKSLASH = 92;
const OPEN_BRACE = 123;
const CLOSE_BRACE = 125;

function isSpace(code: number): boolean {
  return code === 32 || code === 9 || code === 10 || code === 13;
}

/**
 * Where the math opened at `start` ends (just past its closing delimiter), or
 * -1 when the `$` there opens nothing.
 *
 * `charAt` answers the character code at a position, or -1 past `end`.
 */
export function mathSpanEnd(charAt: (pos: number) => number, start: number, end: number): number {
  if (charAt(start) !== DOLLAR) return -1;
  const display = charAt(start + 1) === DOLLAR;
  const open = display ? 2 : 1;
  if (!display) {
    const next = charAt(start + 1);
    if (next < 0 || isSpace(next)) return -1;
  }
  let depth = 0;
  for (let pos = start + open; pos < end; pos++) {
    const code = charAt(pos);
    if (code === BACKSLASH) {
      pos++;
      continue;
    }
    if (code === OPEN_BRACE) {
      depth++;
      continue;
    }
    if (code === CLOSE_BRACE) {
      if (depth > 0) depth--;
      continue;
    }
    if (code !== DOLLAR || depth > 0) continue;
    if (display) {
      // Only a `$$` closes display math, and nothing else may come first.
      if (charAt(pos + 1) !== DOLLAR || pos === start + open) return -1;
      return pos + 2;
    }
    if (isSpace(charAt(pos - 1))) return -1;
    return pos + 1;
  }
  return -1;
}

/**
 * The length of the display span a leaf's text opens with, when that span
 * is all the leaf holds apart from trailing whitespace; otherwise -1.
 */
export function blockMathLength(text: string): number {
  if (text.charCodeAt(0) !== DOLLAR || text.charCodeAt(1) !== DOLLAR) return -1;
  const end = mathSpanEnd((pos) => (pos < text.length ? text.charCodeAt(pos) : -1), 0, text.length);
  if (end < 0) return -1;
  if (text.slice(end).trim() !== "") return -1;
  if (text.slice(2, end - 2).trim() === "") return -1;
  return end;
}

/** The expression inside a math node's delimiters, and whether it is display math. */
export function mathSource(text: string): { tex: string; display: boolean } {
  const display = text.startsWith("$$") && text.endsWith("$$") && text.length >= 4;
  const cut = display ? 2 : 1;
  return { tex: text.slice(cut, text.length - cut).trim(), display };
}

// ─── Grammar extension ─────────────────────────────────────────────────────

interface InlineContextLike {
  readonly end: number;
  char(pos: number): number;
  elt(type: string, from: number, to: number, children?: readonly unknown[]): unknown;
  addElement(element: unknown): number;
}

interface LeafBlockLike {
  readonly start: number;
  readonly content: string;
}

interface BlockContextLike {
  elt(type: string, from: number, to: number, children?: readonly unknown[]): unknown;
  addLeafElement(leaf: LeafBlockLike, element: unknown): void;
}

function parseInlineMath(cx: InlineContextLike, next: number, start: number): number {
  if (next !== DOLLAR) return -1;
  const end = mathSpanEnd((pos) => cx.char(pos), start, cx.end);
  if (end < 0) return -1;
  const mark = cx.char(start + 1) === DOLLAR ? 2 : 1;
  return cx.addElement(
    cx.elt("InlineMath", start, end, [
      cx.elt("InlineMathMark", start, start + mark),
      cx.elt("InlineMathMark", end - mark, end),
    ]),
  );
}

function addBlockMath(cx: BlockContextLike, leaf: LeafBlockLike, length: number): void {
  const from = leaf.start;
  const to = from + length;
  cx.addLeafElement(
    leaf,
    cx.elt("BlockMath", from, to, [
      cx.elt("BlockMathMark", from, from + 2),
      cx.elt("BlockMathMark", to - 2, to),
    ]),
  );
}

/**
 * Reads a paragraph that opens with `$$`. The block ends on the line its
 * closing `$$` ends, so a line of text right under it starts a paragraph of
 * its own instead of turning the block back into prose.
 */
class BlockMathLeaf {
  nextLine(cx: BlockContextLike, _line: unknown, leaf: LeafBlockLike): boolean {
    const length = blockMathLength(leaf.content);
    if (length < 0) return false;
    addBlockMath(cx, leaf, length);
    return true;
  }

  finish(cx: BlockContextLike, leaf: LeafBlockLike): boolean {
    const length = blockMathLength(leaf.content);
    if (length < 0) return false;
    addBlockMath(cx, leaf, length);
    return true;
  }
}

/** The math grammar extension, for `markdown({ extensions })`. */
export const markdownMath = {
  defineNodes: [
    { name: "InlineMath" },
    { name: "InlineMathMark" },
    { name: "BlockMath", block: true },
    { name: "BlockMathMark" },
  ],
  parseInline: [{ name: "InlineMath", parse: parseInlineMath }],
  parseBlock: [
    {
      name: "BlockMath",
      leaf: (_cx: unknown, leaf: LeafBlockLike) =>
        leaf.content.startsWith("$$") ? new BlockMathLeaf() : null,
      before: "SetextHeading",
    },
  ],
};

/** The grammar extensions every Markdown parse in the editor carries. */
export const MARKDOWN_EXTENSIONS = [markdownMath];
