import {
  CharCategory,
  codePointAt,
  codePointSize,
  findClusterBreak,
  type EditorState,
  type Text,
  type TextIterator,
} from "@codemirror/state";
import type { SearchQuery } from "@codemirror/search";

/**
 * Where a chain of matches can be picked up as if it had been followed from
 * the start of the document.
 */
export interface Resume {
  pos: number;
  /** An empty match at `pos` is passed over, as a regexp cursor does right after a match. */
  skipEmpty: boolean;
}

export type MatchSink = (from: number, to: number) => boolean;

export interface ReadSink {
  /** From this position on the reader reports every point it stands free at; before it, only between blocks. */
  readonly watchFrom: number;
  match(from: number, to: number): boolean;
  free(at: Resume): boolean;
}

export interface ChainReader {
  readonly resume: Resume;
  readonly ended: boolean;
  /** The match a regexp cursor reading across lines turned down, which ends its chain. */
  readonly stopped: { from: number; to: number } | null;
  /** The regular expression engine gave up on a match attempt, which ends the reading. */
  readonly failed: boolean;
  advance(sink: ReadSink): void;
}

export interface QueryScanner {
  readonly limits: ScanLimits;
  start(doc: Text): Resume;
  readonly resumable: boolean;
  after(doc: Text, from: number, to: number): Resume;
  /**
   * How a chain whose last match before `pos` is [prevFrom, prevTo] (-1 for
   * none) stands at `pos`, or null when it may never stand there: inside a
   * match, or inside one the cursor turned down and stepped over.
   */
  stateAt(doc: Text, prevFrom: number, prevTo: number, pos: number): Resume | null;
  /** Where to pick the chain up so that it finds again the match starting at `from`. */
  restartAt(doc: Text, from: number): Resume;
  /** A point at or before `pos` such that an edit at `pos` changes no match before it. */
  restartBefore(doc: Text, pos: number): number;
  /** A point at or after `pos` such that an edit ending at `pos` changes nothing found from it on. */
  settleAfter(doc: Text, pos: number): number;
  reader(state: EditorState, at: Resume): ChainReader;
}

export interface ScanLimits {
  /** Characters read between checks, and the size of one window. */
  window: number;
  /** How far a regular expression's matches and lookarounds may reach from where they start. */
  reach: number;
  longLine: number;
  /** Code units a grapheme cluster next to a string match may span. */
  clusterMargin: number;
  /** Characters an edit is read again through in the update that made it; the slices read the rest. */
  editRead: number;
}

export const DEFAULT_SCAN_LIMITS: ScanLimits = {
  window: 64 * 1024,
  reach: 4 * 1024,
  longLine: 64 * 1024,
  clusterMargin: 256,
  editRead: 128 * 1024,
};

const NON_ASCII = /[^\x00-\x7f]/;
const LINE_BREAK = 10;
// RegExpCursor reads a pattern as able to cross a line when it holds one of
// these; such a pattern is matched against the text across lines.
const MULTILINE_PATTERN = /\\[sWDnr]|\n|\r|\[\^/;
const REGEXP_FLAGS = "gmu";

/** SearchQuery.unquote: what the string the person typed searches for. */
export function unquoteSearch(query: SearchQuery): string {
  return query.literal
    ? query.search
    : query.search.replace(/\\([nrt\\])/g, (_, ch: string) =>
        ch === "n" ? "\n" : ch === "r" ? "\r" : ch === "t" ? "\t" : "\\",
      );
}

export function wordCharsKey(state: EditorState): string {
  const chars = state.languageDataAt<string>("wordChars", state.selection.main.head);
  return chars.length ? chars[0] : "";
}

const isHighSurrogate = (code: number) => code >= 0xd800 && code < 0xdc00;
const isLowSurrogate = (code: number) => code >= 0xdc00 && code < 0xe000;

function splitsPair(text: string, i: number): boolean {
  return i > 0 && i < text.length && isHighSurrogate(text.charCodeAt(i - 1)) && isLowSurrogate(text.charCodeAt(i));
}

function alignBack(doc: Text, pos: number): number {
  if (pos <= 0) return 0;
  if (pos >= doc.length) return doc.length;
  return splitsPair(doc.sliceString(pos - 1, pos + 1), 1) ? pos - 1 : pos;
}

export function isLineStart(doc: Text, pos: number): boolean {
  return pos === 0 || (pos <= doc.length && doc.sliceString(pos - 1, pos) === "\n");
}

/** CodeMirror's toCharEnd: past any low surrogate at `pos`. */
function toCharEnd(doc: Text, pos: number): number {
  for (; pos < doc.length; pos++) {
    if (!isLowSurrogate(doc.sliceString(pos, pos + 1).charCodeAt(0))) break;
  }
  return pos;
}

type Categorizer = (char: string) => CharCategory;

const charBefore = (str: string, index: number) => str.slice(findClusterBreak(str, index, false), index);
const charAfter = (str: string, index: number) => str.slice(index, findClusterBreak(str, index));

/**
 * The whole-word test `SearchCursor` runs on a string match, given the chunk
 * of the document the cursor was reading when the match completed.
 */
function makeWordTest(doc: Text, categorizer: Categorizer) {
  return (from: number, to: number, chunk: string, chunkStart: number): boolean => {
    let buf = chunk;
    let bufPos = chunkStart;
    if (bufPos > from || bufPos + buf.length < to) {
      bufPos = Math.max(0, from - 2);
      buf = doc.sliceString(bufPos, Math.min(doc.length, to + 2));
    }
    return (
      (categorizer(charBefore(buf, from - bufPos)) !== CharCategory.Word ||
        categorizer(charAfter(buf, from - bufPos)) !== CharCategory.Word) &&
      (categorizer(charAfter(buf, to - bufPos)) !== CharCategory.Word ||
        categorizer(charBefore(buf, to - bufPos)) !== CharCategory.Word)
    );
  };
}

type WordTest = ReturnType<typeof makeWordTest>;

type RegexpTest = (from: number, to: number, match: RegExpExecArray) => boolean;

/** The whole-word test a regexp cursor runs, over the text the match was found in. */
function regexpWordTest(categorizer: Categorizer): RegexpTest {
  return (_from, _to, match) => {
    const input = match.input;
    const start = match.index;
    const end = start + match[0].length;
    return (
      !match[0].length ||
      ((categorizer(charBefore(input, start)) !== CharCategory.Word ||
        categorizer(charAfter(input, start)) !== CharCategory.Word) &&
        (categorizer(charAfter(input, end)) !== CharCategory.Word ||
          categorizer(charBefore(input, end)) !== CharCategory.Word))
    );
  };
}

// What `SearchCursor` compares each code point as: NFKD, then lower case when
// case does not matter. Kept per code point for the life of the page.
const UNKNOWN = 0;
const UNCHANGED = 1;
const CHANGED = 2;

class CodePointFold {
  private readonly bmp = new Uint8Array(0x10000);
  private readonly folded = new Map<number, string>();

  constructor(private readonly caseInsensitive: boolean) {}

  fold(text: string): string {
    const nfkd = text.normalize("NFKD");
    return this.caseInsensitive ? nfkd.toLowerCase() : nfkd;
  }

  isUnchanged(code: number): boolean {
    let state = this.bmp[code];
    if (state === UNKNOWN) {
      const char = String.fromCharCode(code);
      const folded = this.fold(char);
      state = folded === char ? UNCHANGED : CHANGED;
      if (state === CHANGED) this.folded.set(code, folded);
      this.bmp[code] = state;
    }
    return state === UNCHANGED;
  }

  folding(code: number, char: string): string {
    let folded = this.folded.get(code);
    if (folded === undefined) {
      folded = this.fold(char);
      this.folded.set(code, folded);
    }
    return folded;
  }
}

const FOLD_CASE_SENSITIVE = new CodePointFold(false);
const FOLD_CASE_INSENSITIVE = new CodePointFold(true);

/** The index of the first non-ASCII code unit in `text` at or after `from`, or the text's length. */
function nextNonAscii(pattern: RegExp, text: string, from: number): number {
  pattern.lastIndex = from;
  return pattern.test(text) ? pattern.lastIndex - 1 : text.length;
}

/** The next index after the ASCII code unit at `i` that may hold a non-ASCII one. */
function skipAscii(pattern: RegExp, text: string, i: number): number {
  return i + 1 < text.length && text.charCodeAt(i + 1) < 0x80 ? nextNonAscii(pattern, text, i + 2) : i + 1;
}

// Letters from the most to the least common in English text. A query that
// ignores case is looked for at its least common character, found by a
// native search in either case; a space counts as the most common character,
// any other as common as `u`.
const LETTERS_BY_FREQUENCY = "etaoinshrdlcumwfgypbvkjxqz";
// Past one candidate per this many characters read, the regular expression reads on instead.
const ANCHOR_DENSITY = 64;

interface Anchor {
  /** Where the character stands in the query. */
  offset: number;
  lower: string;
  upper: string;
  /** The query at a given position, ignoring case. */
  sticky: RegExp;
}

function rarity(char: string): number {
  if (char === " ") return -1;
  const rank = LETTERS_BY_FREQUENCY.indexOf(char);
  return rank >= 0 ? rank : LETTERS_BY_FREQUENCY.indexOf("u");
}

function anchorFor(folded: string, pattern: string): Anchor {
  let offset = 0;
  for (let i = 1; i < folded.length; i++) if (rarity(folded[i]) > rarity(folded[offset])) offset = i;
  const lower = folded[offset];
  return { offset, lower, upper: lower.toUpperCase(), sticky: new RegExp(pattern, "iy") };
}

interface StringPlan {
  folded: string;
  ascii: boolean;
  fold: CodePointFold;
  caseInsensitive: boolean;
  asciiPattern: RegExp | null;
  /** For an ASCII query that ignores case, until its character turns out too common to look for. */
  anchor: Anchor | null;
  /** Per BMP code point: 0 not yet known, 1 cannot be part of a match, 2 can. */
  joinsBmp: Uint8Array | null;
  joinsAstral: Map<number, boolean>;
  /** Per census: its code points that can be part of a match. */
  joinableCensus: Map<string, string>;
}

function planString(query: SearchQuery): StringPlan {
  const fold = query.caseSensitive ? FOLD_CASE_SENSITIVE : FOLD_CASE_INSENSITIVE;
  const folded = fold.fold(unquoteSearch(query));
  const ascii = !NON_ASCII.test(folded);
  const source = folded.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const asciiPattern = ascii && !query.caseSensitive ? new RegExp(source, "gi") : null;
  return {
    folded,
    ascii,
    fold,
    caseInsensitive: !query.caseSensitive,
    asciiPattern,
    anchor: asciiPattern ? anchorFor(folded, source) : null,
    joinsBmp: null,
    joinsAstral: new Map(),
    joinableCensus: new Map(),
  };
}

/**
 * Whether some match of `query` can hold folded units of a code point that
 * folds to `folded`: units it covers whole, or a match starting or ending
 * inside them.
 */
function foldJoins(query: string, folded: string): boolean {
  for (let a = 0; a < folded.length; a++) {
    for (let b = a + 1; b <= folded.length; b++) {
      const part = folded.slice(a, b);
      const fits =
        a === 0
          ? b === folded.length
            ? query.includes(part)
            : query.endsWith(part)
          : b === folded.length
            ? query.startsWith(part)
            : query === part;
      if (fits) return true;
    }
  }
  return false;
}

/** Whether the code point can be part of a match of the plan's query. */
function joins(plan: StringPlan, point: number): boolean {
  if (point <= 0xffff) {
    const bmp = (plan.joinsBmp ??= new Uint8Array(0x10000));
    const known = bmp[point];
    if (known !== 0) return known === 2;
    const char = String.fromCharCode(point);
    const result = foldJoins(plan.folded, plan.fold.isUnchanged(point) ? char : plan.fold.folding(point, char));
    bmp[point] = result ? 2 : 1;
    return result;
  }
  let known = plan.joinsAstral.get(point);
  if (known === undefined) {
    known = foldJoins(plan.folded, plan.fold.folding(point, String.fromCodePoint(point)));
    plan.joinsAstral.set(point, known);
  }
  return known;
}

const NON_ASCII_JOINS = /[^\x00-\x7f]/g;

/** The index of the first code point in `text` at or after `from` that can be part of a match, or -1. */
function joinableAt(plan: StringPlan, text: string, from: number): number {
  for (let i = nextNonAscii(NON_ASCII_JOINS, text, from); i < text.length; ) {
    if (text.charCodeAt(i) < 0x80) {
      i = skipAscii(NON_ASCII_JOINS, text, i);
      continue;
    }
    const point = codePointAt(text, i);
    if (joins(plan, point)) return i;
    i += codePointSize(point);
  }
  return -1;
}

// A census lists, sorted, the code points of a node of the document that fold
// (in either case mode) to something holding an ASCII code unit: of the
// code points past ASCII, only these can be part of a match of an ASCII
// query. Kept per node for the node's life, so a find keystroke reads the
// text with native searches alone wherever none of them can join its match.
// A node up to `CENSUS_TEXT_MAX` long is listed from its text and stands for
// any part of it; a longer one is the union of its children.
const CENSUS_POINTS = 128;
const CENSUS_TEXT_MAX = 64 * 1024;
const FOLDS_TO_ASCII_BMP = new Uint8Array(0x10000);
const FOLDS_TO_ASCII_ASTRAL = new Map<number, boolean>();
const ASCII_UNIT = /[\x00-\x7f]/;
const NON_ASCII_CENSUS = /[^\x00-\x7f]/g;
const censuses = new WeakMap<Text, string | null>();

function foldsToAscii(point: number): boolean {
  if (point <= 0xffff && FOLDS_TO_ASCII_BMP[point] !== 0) return FOLDS_TO_ASCII_BMP[point] === 2;
  if (point > 0xffff) {
    const known = FOLDS_TO_ASCII_ASTRAL.get(point);
    if (known !== undefined) return known;
  }
  const char = String.fromCodePoint(point);
  const result =
    ASCII_UNIT.test(FOLD_CASE_SENSITIVE.fold(char)) || ASCII_UNIT.test(FOLD_CASE_INSENSITIVE.fold(char));
  if (point <= 0xffff) FOLDS_TO_ASCII_BMP[point] = result ? 2 : 1;
  else FOLDS_TO_ASCII_ASTRAL.set(point, result);
  return result;
}

/** A census from its code points, or null past `CENSUS_POINTS` of them. */
function censusFrom(points: number[]): string | null {
  if (points.length > CENSUS_POINTS) return null;
  return String.fromCodePoint(...points.sort((a, b) => a - b));
}

function censusOfText(text: string): string | null {
  const points: number[] = [];
  for (let i = nextNonAscii(NON_ASCII_CENSUS, text, 0); i < text.length; ) {
    if (text.charCodeAt(i) < 0x80) {
      i = skipAscii(NON_ASCII_CENSUS, text, i);
      continue;
    }
    const point = codePointAt(text, i);
    i += codePointSize(point);
    if (!foldsToAscii(point) || points.includes(point)) continue;
    points.push(point);
    if (points.length > CENSUS_POINTS) return null;
  }
  return censusFrom(points);
}

/** The node's census, or null when it lists too many code points or the node is a leaf too long to read at once. */
function census(node: Text): string | null {
  const known = censuses.get(node);
  if (known !== undefined) return known;
  let found: string | null;
  const children = node.children;
  if (node.length <= CENSUS_TEXT_MAX) {
    found = censusOfText(node.sliceString(0));
  } else if (!children) {
    found = null;
  } else {
    const points: number[] = [];
    found = "";
    for (const child of children) {
      const part = census(child);
      if (part === null) {
        found = null;
        break;
      }
      for (let i = 0; i < part.length; ) {
        const point = codePointAt(part, i);
        if (!points.includes(point)) points.push(point);
        i += codePointSize(point);
      }
      if (points.length > CENSUS_POINTS) {
        found = null;
        break;
      }
    }
    if (found !== null) found = censusFrom(points);
  }
  censuses.set(node, found);
  return found;
}

// Past this many code points that can join a match, a window is searched for
// every code point past ASCII instead of for each of them.
const JOINABLE_SEARCHES = 6;

/** The code points of a census that can be part of a match of the plan's query. */
function censusJoinable(plan: StringPlan, found: string): string {
  let known = plan.joinableCensus.get(found);
  if (known === undefined) {
    known = "";
    for (let i = 0; i < found.length; ) {
      const point = codePointAt(found, i);
      const size = codePointSize(point);
      if (joins(plan, point)) known += found.slice(i, i + size);
      i += size;
    }
    plan.joinableCensus.set(found, known);
  }
  return known;
}

/** `joinable` with the code points of `more` it lacks, or null past `JOINABLE_SEARCHES` of them. */
function joinJoinable(joinable: string, more: string): string | null {
  let count = 0;
  for (let i = 0; i < joinable.length; i += codePointSize(codePointAt(joinable, i))) count++;
  for (let i = 0; i < more.length; ) {
    const size = codePointSize(codePointAt(more, i));
    const char = more.slice(i, i + size);
    if (!joinable.includes(char)) {
      if (++count > JOINABLE_SEARCHES) return null;
      joinable += char;
    }
    i += size;
  }
  return joinable;
}

/**
 * For an ASCII query, the code points that can be part of a match among
 * those of the censuses covering [from, to) of the node (at `nodeFrom`): ""
 * for none, or null when a census is not known or they are too many to
 * search for one by one.
 */
function joinableRange(plan: StringPlan, node: Text, nodeFrom: number, from: number, to: number): string | null {
  const children = node.children;
  if (!children || node.length <= CENSUS_TEXT_MAX || (from <= nodeFrom && nodeFrom + node.length <= to)) {
    const found = census(node);
    return found === null ? null : joinJoinable("", censusJoinable(plan, found));
  }
  let joinable: string | null = "";
  for (let i = 0, pos = nodeFrom; i < children.length && pos < to; i++) {
    const child = children[i];
    const end = pos + child.length;
    if (end > from) {
      const part = joinableRange(plan, child, pos, from, to);
      if (part === null) return null;
      if (part) joinable = joinJoinable(joinable, part);
      if (joinable === null) return null;
    }
    pos = end + 1;
  }
  return joinable;
}

/** Finds, from a given index on, the next code point of `text` that can be part of a match, or -1. */
function joinableFinder(plan: StringPlan, text: string, joinable: string | null): (from: number) => number {
  if (joinable === null) return (from) => joinableAt(plan, text, from);
  const chars: string[] = [];
  for (let i = 0; i < joinable.length; ) {
    const size = codePointSize(codePointAt(joinable, i));
    chars.push(joinable.slice(i, i + size));
    i += size;
  }
  // The next index of each code point: -2 not yet looked for, -1 none left.
  const next = chars.map(() => -2);
  return (from) => {
    let first = -1;
    for (let i = 0; i < chars.length; i++) {
      if (next[i] !== -1 && next[i] < from) next[i] = text.indexOf(chars[i], from);
      if (next[i] >= 0 && (first < 0 || next[i] < first)) first = next[i];
    }
    return first;
  };
}

/** What the code point of `size` code units at `at` in `text` is compared as. */
function foldedAt(plan: StringPlan, text: string, at: number, size: number): string {
  const char = text.slice(at, at + size);
  return size === 1 && plan.fold.isUnchanged(char.charCodeAt(0)) ? char : plan.fold.folding(codePointAt(char, 0), char);
}

const foldedAscii = (plan: StringPlan, code: number) =>
  plan.caseInsensitive && code >= 65 && code <= 90 ? code + 32 : code;

/**
 * Whether the folded text ending at `end` ends with the query's first
 * `length` units, the first of them anywhere in a code point's folded form.
 * Text before the window does not count: a match starting there is not the
 * window's own.
 */
function fitsBefore(plan: StringPlan, text: string, end: number, length: number): boolean {
  const query = plan.folded;
  for (let i = end, j = length; j > 0; ) {
    if (i <= 0) return false;
    const code = text.charCodeAt(i - 1);
    if (code < 0x80) {
      if (foldedAscii(plan, code) !== query.charCodeAt(j - 1)) return false;
      i--;
      j--;
      continue;
    }
    const size = i >= 2 && isLowSurrogate(code) && isHighSurrogate(text.charCodeAt(i - 2)) ? 2 : 1;
    const folded = foldedAt(plan, text, i - size, size);
    for (let k = folded.length - 1; k >= 0 && j > 0; k--, j--) {
      if (folded.charCodeAt(k) !== query.charCodeAt(j - 1)) return false;
    }
    i -= size;
  }
  return true;
}

/**
 * Whether the folded text starting at `start` starts with the query's units
 * from `from` on, the last of them anywhere in a code point's folded form.
 * The window holds all that its own matches reach.
 */
function fitsAfter(plan: StringPlan, text: string, start: number, from: number): boolean {
  const query = plan.folded;
  for (let i = start, j = from; j < query.length; ) {
    if (i >= text.length) return false;
    const code = text.charCodeAt(i);
    if (code < 0x80) {
      if (foldedAscii(plan, code) !== query.charCodeAt(j)) return false;
      i++;
      j++;
      continue;
    }
    const size = codePointSize(codePointAt(text, i));
    const folded = foldedAt(plan, text, i, size);
    for (let k = 0; k < folded.length && j < query.length; k++, j++) {
      if (folded.charCodeAt(k) !== query.charCodeAt(j)) return false;
    }
    i += size;
  }
  return true;
}

/**
 * Whether the code point at `at`, which can be part of a match, is part of a
 * match of the window's own: whether its folded units, in one of the ways
 * they fit the query, meet the folded text around them.
 */
function mayJoinAt(plan: StringPlan, text: string, at: number): boolean {
  const size = codePointSize(codePointAt(text, at));
  const folded = foldedAt(plan, text, at, size);
  const query = plan.folded;
  for (let a = 0; a < folded.length; a++) {
    for (let b = a + 1; b <= folded.length; b++) {
      const part = folded.slice(a, b);
      if (a > 0 && b < folded.length) {
        if (query === part) return true;
      } else if (a > 0) {
        if (query.startsWith(part) && fitsAfter(plan, text, at + size, part.length)) return true;
      } else if (b < folded.length) {
        if (query.endsWith(part) && fitsBefore(plan, text, at, query.length - part.length)) return true;
      } else {
        for (let i = query.indexOf(part); i >= 0; i = query.indexOf(part, i + 1)) {
          if (fitsBefore(plan, text, at, i) && fitsAfter(plan, text, at + size, i + part.length)) return true;
        }
      }
    }
  }
  return false;
}

/** The chunk `SearchCursor` holds when a match ending at `to` completes. */
type ChunkAt = (to: number) => { chunk: string; start: number };

interface WindowScan {
  restart: number;
  stopped: boolean;
}

// Pieces of a folded window: a run of code units that fold to themselves one
// for one (span 0), or one code point whose folded form differs from it (span
// is its own length; prefix counts the leading folded units equal to its own).
interface Pieces {
  unit: number[];
  offset: number[];
  length: number[];
  span: number[];
  prefix: number[];
}

function pieceAt(pieces: Pieces, unit: number): number {
  let lo = 0;
  let hi = pieces.unit.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (pieces.unit[mid] <= unit) lo = mid;
    else hi = mid - 1;
  }
  return lo;
}

const NON_ASCII_FOLD = /[^\x00-\x7f]/g;

function foldWindow(text: string, plan: StringPlan): { folded: string; pieces: Pieces } {
  const pieces: Pieces = { unit: [], offset: [], length: [], span: [], prefix: [] };
  const parts: string[] = [];
  let unit = 0;
  let runStart = 0;
  const closeRun = (end: number) => {
    if (end <= runStart) return;
    const run = text.slice(runStart, end);
    parts.push(plan.caseInsensitive ? run.toLowerCase() : run);
    pieces.unit.push(unit);
    pieces.offset.push(runStart);
    pieces.length.push(end - runStart);
    pieces.span.push(0);
    pieces.prefix.push(0);
    unit += end - runStart;
  };
  for (let i = nextNonAscii(NON_ASCII_FOLD, text, 0); i < text.length; ) {
    const code = text.charCodeAt(i);
    if (code < 0x80) {
      i = skipAscii(NON_ASCII_FOLD, text, i);
      continue;
    }
    const point = codePointAt(text, i);
    const size = codePointSize(point);
    if (size === 1 && plan.fold.isUnchanged(code)) {
      i++;
      continue;
    }
    closeRun(i);
    const char = text.slice(i, i + size);
    const folded = plan.fold.folding(point, char);
    let prefix = 0;
    while (prefix < size && prefix < folded.length && folded.charCodeAt(prefix) === char.charCodeAt(prefix)) {
      prefix++;
    }
    parts.push(folded);
    pieces.unit.push(unit);
    pieces.offset.push(i);
    pieces.length.push(folded.length);
    pieces.span.push(size);
    pieces.prefix.push(prefix);
    unit += folded.length;
    i += size;
    runStart = i;
  }
  closeRun(text.length);
  return { folded: parts.join(""), pieces };
}

type Take = (from: number, to: number) => boolean | null;

/**
 * The string matches `SearchCursor` takes from `text` (at document position
 * `base`) whose first code point starts in [ownFrom, ownTo) and at or after
 * `restart`, in order. For an ASCII query, `joinable` lists, when known, the
 * code points of the text past ASCII that can be part of a match.
 *
 * `SearchCursor` compares each code point's folded form with the folded query.
 * A match starts at the code point holding its first folded unit (one past it
 * when the code point's own leading unit equals the folded one) and ends at
 * the end of the code point holding its last unit. Once a match is taken, the
 * next one starts at a later code point; a match the word test turns down
 * leaves the overlapping ones in play.
 *
 * A code point past ASCII takes part in a match only when its folded units
 * fit the query and the ASCII next to it does not rule that out. Text with no
 * such code point holds an ASCII query's matches as it stands, found by a
 * native search, and no match of a query holding other code units. A query
 * without a line break matches within a line, so only the lines holding such
 * a code point are folded.
 */
function scanStringWindow(
  plan: StringPlan,
  test: WordTest | null,
  chunkAt: ChunkAt,
  text: string,
  base: number,
  ownFrom: number,
  ownTo: number,
  restart: number,
  sink: MatchSink,
  joinable: string | null = null,
): WindowScan {
  const scan: WindowScan = { restart, stopped: false };
  const take: Take = (from, to) => {
    if (test) {
      const { chunk, start } = chunkAt(to);
      if (!test(from, to, chunk, start)) return null;
    }
    return sink(from, to);
  };
  const nextJoinable = joinable === "" ? () => -1 : joinableFinder(plan, text, plan.ascii ? joinable : null);
  const multiline = plan.folded.includes("\n");
  let from = 0;
  for (let at = nextJoinable(0); at >= 0; ) {
    if (!mayJoinAt(plan, text, at)) {
      at = nextJoinable(at + codePointSize(codePointAt(text, at)));
      continue;
    }
    if (multiline) {
      scanFolded(plan, take, text, base, ownFrom, ownTo, scan);
      return scan;
    }
    const lineStart = text.lastIndexOf("\n", at) + 1;
    const next = text.indexOf("\n", at);
    const lineEnd = next < 0 ? text.length : next;
    if (
      plan.ascii &&
      lineStart > from &&
      !scanPlain(plan, take, text.slice(from, lineStart), base + from, ownFrom, ownTo, scan)
    ) {
      return scan;
    }
    if (!scanFolded(plan, take, text.slice(lineStart, lineEnd), base + lineStart, ownFrom, ownTo, scan)) return scan;
    from = lineEnd;
    at = lineEnd < text.length ? nextJoinable(lineEnd) : -1;
  }
  if (plan.ascii && from < text.length) {
    scanPlain(plan, take, from ? text.slice(from) : text, base + from, ownFrom, ownTo, scan);
  }
  return scan;
}

/**
 * `scanStringWindow` over text that holds an ASCII query's matches as it
 * stands. False once nothing more can be taken from the window.
 */
function scanPlain(
  plan: StringPlan,
  take: Take,
  text: string,
  base: number,
  ownFrom: number,
  ownTo: number,
  scan: WindowScan,
): boolean {
  const query = plan.folded;
  const limit = ownTo - base;
  let at = Math.max(0, Math.max(scan.restart, ownFrom) - base);
  for (;;) {
    const found = plan.asciiPattern ? findIgnoringCase(plan, text, at) : text.indexOf(query, at);
    if (found < 0) return true;
    if (found >= limit) return false;
    const from = base + found;
    const to = from + query.length;
    const taken = take(from, to);
    if (taken === null) {
      at = found + 1;
      continue;
    }
    scan.restart = to;
    if (!taken) {
      scan.stopped = true;
      return false;
    }
    at = found + query.length;
  }
}

/** Where the first match of an ASCII query that ignores case starts in `text` at or after `at`, or -1. */
function findIgnoringCase(plan: StringPlan, text: string, at: number): number {
  const anchor = plan.anchor;
  if (anchor) {
    const { offset, lower, upper, sticky } = anchor;
    let nextLower = text.indexOf(lower, at + offset);
    let nextUpper = upper === lower ? -1 : text.indexOf(upper, at + offset);
    for (let checked = 1; ; checked++) {
      const candidate = nextLower < 0 || (nextUpper >= 0 && nextUpper < nextLower) ? nextUpper : nextLower;
      if (candidate < 0) return -1;
      sticky.lastIndex = candidate - offset;
      if (sticky.test(text)) return candidate - offset;
      if (checked > ANCHOR_DENSITY && checked * ANCHOR_DENSITY > candidate - at) {
        plan.anchor = null;
        break;
      }
      if (candidate === nextLower) nextLower = text.indexOf(lower, candidate + 1);
      else nextUpper = text.indexOf(upper, candidate + 1);
    }
  }
  const pattern = plan.asciiPattern as RegExp;
  pattern.lastIndex = at;
  return pattern.test(text) ? pattern.lastIndex - plan.folded.length : -1;
}

/** `scanStringWindow` over the text folded per code point. False once nothing more can be taken from the window. */
function scanFolded(
  plan: StringPlan,
  take: Take,
  text: string,
  base: number,
  ownFrom: number,
  ownTo: number,
  scan: WindowScan,
): boolean {
  const query = plan.folded;
  const { folded, pieces } = foldWindow(text, plan);
  for (let at = 0; ; ) {
    const found = folded.indexOf(query, at);
    if (found < 0) return true;
    const first = pieceAt(pieces, found);
    const firstSpan = pieces.span[first];
    const startPoint = base + pieces.offset[first] + (firstSpan === 0 ? found - pieces.unit[first] : 0);
    if (startPoint >= ownTo) return false;
    if (startPoint < ownFrom || startPoint < scan.restart) {
      at = found + 1;
      continue;
    }
    const from =
      firstSpan === 0 ? startPoint : startPoint + Math.min(found - pieces.unit[first], pieces.prefix[first]);
    const lastUnit = found + query.length - 1;
    const last = pieceAt(pieces, lastUnit);
    const lastSpan = pieces.span[last];
    const to =
      base +
      pieces.offset[last] +
      (lastSpan === 0 ? lastUnit - pieces.unit[last] + 1 : lastSpan);
    const taken = take(from, to);
    if (taken === null) {
      at = found + 1;
      continue;
    }
    scan.restart = to;
    if (!taken) {
      scan.stopped = true;
      return false;
    }
    at = lastUnit + 1;
  }
}

/**
 * The chunk for a window of `text` at `base` that may start or end inside a
 * line: a line break on its own, or the whole line holding the match's end.
 */
function windowChunk(doc: Text, text: string, base: number): ChunkAt {
  const opensLine = isLineStart(doc, base);
  const textEnd = base + text.length;
  const closesLine = textEnd === doc.length || doc.sliceString(textEnd, textEnd + 1) === "\n";
  return (to) => {
    const end = to - 1 - base;
    if (text.charCodeAt(end) === LINE_BREAK) return { chunk: "\n", start: to - 1 };
    const lineStart = text.lastIndexOf("\n", end) + 1;
    const next = text.indexOf("\n", end);
    if ((lineStart > 0 || opensLine) && (next >= 0 || closesLine)) {
      return { chunk: text.slice(lineStart, next < 0 ? text.length : next), start: base + lineStart };
    }
    const line = doc.lineAt(to - 1);
    return { chunk: line.text, start: line.from };
  };
}

const MATCH = 0;
const MOVED = 1;
const END = 2;
const STOP = 3;
const FAIL = 4;
type Step = typeof MATCH | typeof MOVED | typeof END | typeof STOP | typeof FAIL;

/**
 * A regular expression read over one stretch of the document (a line, or the
 * whole document for a pattern that crosses lines) in windows, picking up
 * after each match where CodeMirror's regexp cursors do: `RegExpCursor` per
 * line, or `MultilineRegExpCursor` across lines.
 *
 * A window reaches `reach` characters behind the search position. A match is
 * taken from a window only when it ends `reach` characters before the window
 * does, or the window holds the rest of the stretch; with nothing found, the
 * search moves to `reach` before the window's end. So a match found here is
 * the expression's own match over the whole stretch whenever matches and
 * lookarounds stay within `reach`, and a match that starts where the search
 * stands and runs on grows the window until it ends. `MultilineRegExpCursor`
 * reads in chunks of its own and can end a greedy run at a chunk's end where
 * this reads on (ADR-013).
 */
class RegexRun {
  from = 0;
  to = 0;
  pos = 0;
  skipEmpty = false;
  private segFrom = 0;
  private segTo = 0;
  private winFrom = 0;
  private winTo = -1;
  private text = "";
  private complete = false;
  private size: number;

  constructor(
    private readonly re: RegExp,
    private readonly flow: boolean,
    private readonly test: RegexpTest | null,
    private readonly doc: Text,
    private readonly limits: ScanLimits,
  ) {
    this.size = limits.window;
  }

  begin(segFrom: number, segTo: number, whole: string | null, at: Resume): void {
    this.segFrom = segFrom;
    this.segTo = segTo;
    this.pos = at.pos;
    this.skipEmpty = at.skipEmpty;
    this.size = this.limits.window;
    if (whole === null) {
      this.winTo = -1;
    } else {
      this.winFrom = segFrom;
      this.winTo = segTo;
      this.text = whole;
      this.complete = true;
    }
  }

  step(): Step {
    if (!this.holds()) this.load();
    const { reach, window } = this.limits;
    const off = this.pos - this.winFrom;
    const re = this.re;
    let match: RegExpExecArray | null;
    try {
      re.lastIndex = off;
      match = re.exec(this.text);
      if (this.flow && this.skipEmpty && match && match[0] === "" && match.index === off) {
        re.lastIndex = off + 1;
        match = re.exec(this.text);
      }
    } catch (error) {
      // V8 throws a RangeError once a match attempt outgrows its backtracking stack.
      if (error instanceof RangeError) return FAIL;
      throw error;
    }
    if (!match) {
      if (this.complete) return END;
      this.moveTo(this.boundary(this.winTo - reach));
      return MOVED;
    }
    const from = this.winFrom + match.index;
    const to = from + match[0].length;
    if (!this.complete && (to > this.winTo - reach || from >= this.winTo - reach)) {
      if (from > this.pos) {
        this.moveTo(from < this.winTo - reach ? from : this.boundary(this.winTo - reach));
      } else {
        this.size *= 2;
        this.winTo = -1;
      }
      return MOVED;
    }
    this.from = from;
    this.to = to;
    const next = this.charEnd(to + (from === to ? 1 : 0));
    if (this.flow) {
      if (this.test && !this.test(from, to, match)) return STOP;
      this.pos = next;
      this.skipEmpty = true;
      this.size = window;
      return MATCH;
    }
    const taken = (from < to || from > this.pos || !this.skipEmpty) && (!this.test || this.test(from, to, match));
    this.pos = next;
    this.skipEmpty = taken && next === to;
    this.size = window;
    return taken ? MATCH : MOVED;
  }

  private holds(): boolean {
    if (this.winTo < 0 || this.pos < this.winFrom) return false;
    return this.complete || this.winTo - this.pos > this.limits.reach;
  }

  private load(): void {
    let from = Math.max(this.segFrom, this.pos - this.limits.reach);
    let to = Math.min(this.segTo, this.pos + this.size);
    let text = this.doc.sliceString(from, to);
    if (from > this.segFrom && isLowSurrogate(text.charCodeAt(0))) {
      from--;
      text = this.doc.sliceString(from, from + 1) + text;
    }
    if (to < this.segTo && isHighSurrogate(text.charCodeAt(text.length - 1))) {
      text += this.doc.sliceString(to, to + 1);
      to++;
    }
    this.winFrom = from;
    this.winTo = to;
    this.text = text;
    this.complete = to === this.segTo;
  }

  private moveTo(pos: number): void {
    this.pos = pos;
    this.skipEmpty = false;
    this.size = this.limits.window;
  }

  private charAt(pos: number): number {
    return pos >= this.winFrom && pos < this.winTo
      ? this.text.charCodeAt(pos - this.winFrom)
      : this.doc.sliceString(pos, pos + 1).charCodeAt(0);
  }

  private boundary(pos: number): number {
    return pos > 0 && isLowSurrogate(this.charAt(pos)) && isHighSurrogate(this.charAt(pos - 1)) ? pos + 1 : pos;
  }

  private charEnd(pos: number): number {
    while (pos < this.doc.length && isLowSurrogate(this.charAt(pos))) pos++;
    return pos;
  }
}

interface LineKind {
  readLine(text: string, lineFrom: number, at: Resume, sink: ReadSink, reader: LineReader): boolean;
}

class LineReader implements ChainReader {
  resume: Resume;
  ended = false;
  failed = false;
  readonly stopped = null;
  private lines: TextIterator | null = null;
  private lineFrom = 0;
  private lineText: string | null = null;

  constructor(
    private readonly kind: LineKind,
    private readonly doc: Text,
    at: Resume,
    private readonly window: number,
  ) {
    this.resume = at;
  }

  advance(sink: ReadSink): void {
    let read = 0;
    while (!this.ended) {
      if (this.lineText === null && !this.nextLine()) return;
      const text = this.lineText as string;
      if (!this.kind.readLine(text, this.lineFrom, this.resume, sink, this)) return;
      this.lineFrom += text.length + 1;
      this.lineText = null;
      if (this.resume.pos < this.lineFrom) this.resume = { pos: this.lineFrom, skipEmpty: false };
      read += text.length + 1;
      if (read >= this.window || this.lineFrom >= sink.watchFrom) {
        read = 0;
        if (!sink.free(this.resume)) return;
      }
    }
  }

  private nextLine(): boolean {
    if (!this.lines) {
      if (this.resume.pos > this.doc.length) {
        this.finish();
        return false;
      }
      const line = this.doc.lineAt(this.resume.pos);
      this.lines = this.doc.iterLines(line.number);
      this.lineFrom = line.from;
    }
    this.lines.next();
    if (this.lines.done) {
      this.finish();
      return false;
    }
    this.lineText = this.lines.value;
    return true;
  }

  fail(): void {
    this.ended = true;
    this.failed = true;
  }

  private finish(): void {
    this.ended = true;
    this.resume = { pos: this.doc.length + 1, skipEmpty: false };
  }
}

class LineRegexKind implements LineKind {
  private readonly run: RegexRun;

  constructor(re: RegExp, test: RegexpTest | null, doc: Text, private readonly limits: ScanLimits) {
    this.run = new RegexRun(re, false, test, doc, limits);
  }

  readLine(text: string, lineFrom: number, at: Resume, sink: ReadSink, reader: LineReader): boolean {
    const run = this.run;
    const long = text.length > this.limits.longLine;
    run.begin(lineFrom, lineFrom + text.length, long ? null : text, at);
    for (;;) {
      const step = run.step();
      if (step === FAIL) {
        reader.fail();
        return false;
      }
      if (step === END) {
        // An empty match at the line's end carries the cursor past any low
        // surrogate the next line starts with.
        reader.resume = { pos: run.pos, skipEmpty: false };
        return true;
      }
      reader.resume = { pos: run.pos, skipEmpty: run.skipEmpty };
      if (step === MATCH) {
        if (!sink.match(run.from, run.to)) return false;
        if (run.pos >= sink.watchFrom && !sink.free(reader.resume)) return false;
      } else if (long && !sink.free(reader.resume)) {
        return false;
      }
    }
  }
}

class FlowRegexReader implements ChainReader {
  resume: Resume;
  ended = false;
  failed = false;
  stopped: { from: number; to: number } | null = null;
  private readonly run: RegexRun;

  constructor(re: RegExp, test: RegexpTest | null, private readonly doc: Text, at: Resume, limits: ScanLimits) {
    this.run = new RegexRun(re, true, test, doc, limits);
    this.run.begin(0, doc.length, null, at);
    this.resume = at;
  }

  advance(sink: ReadSink): void {
    const run = this.run;
    while (!this.ended) {
      const step = run.step();
      if (step === FAIL) {
        this.ended = true;
        this.failed = true;
        return;
      }
      if (step === END) {
        this.ended = true;
        this.resume = { pos: this.doc.length + 1, skipEmpty: false };
        return;
      }
      if (step === STOP) {
        this.ended = true;
        this.stopped = { from: run.from, to: run.to };
        return;
      }
      this.resume = { pos: run.pos, skipEmpty: run.skipEmpty };
      if (step === MATCH && !sink.match(run.from, run.to)) return;
      if (!sink.free(this.resume)) return;
    }
  }
}

class FlowStringReader implements ChainReader {
  resume: Resume;
  ended = false;
  readonly failed = false;
  readonly stopped = null;

  constructor(
    private readonly plan: StringPlan,
    private readonly test: WordTest | null,
    private readonly doc: Text,
    at: Resume,
    private readonly limits: ScanLimits,
  ) {
    this.resume = at;
  }

  advance(sink: ReadSink): void {
    const doc = this.doc;
    const span = 2 * this.plan.folded.length;
    const take: MatchSink = (from, to) => {
      this.resume = { pos: to, skipEmpty: false };
      return sink.match(from, to) && (to < sink.watchFrom || sink.free(this.resume));
    };
    let restart = this.resume.pos;
    for (let start = this.resume.pos; start < doc.length; ) {
      let end = Math.min(doc.length, start + this.limits.window);
      // A window ends where an edit's reading may join what was read before it.
      const watch = sink.watchFrom;
      if (watch > start && watch < end) end = watch;
      const textEnd = Math.min(doc.length, end + span);
      const text = doc.sliceString(start, textEnd);
      if (splitsPair(text, end - start)) end++;
      const last = end === doc.length;
      const scan = scanStringWindow(
        this.plan,
        this.test,
        windowChunk(doc, text, start),
        text,
        start,
        start,
        last ? Infinity : end,
        restart,
        take,
        this.plan.ascii ? joinableRange(this.plan, doc, 0, start, textEnd) : null,
      );
      if (scan.stopped) return;
      restart = scan.restart;
      start = end;
      if (last) break;
      this.resume = { pos: Math.max(end, restart), skipEmpty: false };
      if (!sink.free(this.resume)) return;
    }
    this.ended = true;
    this.resume = { pos: doc.length + 1, skipEmpty: false };
  }
}

/** CodeMirror's own cursor, for a query carrying its own test: read from the start only. */
class CursorReader implements ChainReader {
  resume: Resume = { pos: 0, skipEmpty: false };
  ended = false;
  failed = false;
  readonly stopped = null;
  private readonly cursor: ReturnType<SearchQuery["getCursor"]>;

  constructor(query: SearchQuery, state: EditorState) {
    this.cursor = query.getCursor(state);
  }

  advance(sink: ReadSink): void {
    while (!this.ended) {
      let step: IteratorResult<{ from: number; to: number }>;
      try {
        step = this.cursor.next();
      } catch (error) {
        if (!(error instanceof RangeError)) throw error;
        this.ended = true;
        this.failed = true;
        return;
      }
      if (step.done) {
        this.ended = true;
        return;
      }
      this.resume = { pos: step.value.to, skipEmpty: true };
      if (!sink.match(step.value.from, step.value.to)) return;
      if (!sink.free(this.resume)) return;
    }
  }
}

const CHAIN_START: Resume = { pos: 0, skipEmpty: false };
const chainStart = () => CHAIN_START;

interface ChainRules {
  start(doc: Text): Resume;
  after(doc: Text, from: number, to: number): Resume;
  /** A point between matches is one the chain stands at; otherwise only line starts are. */
  gapsResume: boolean;
  /**
   * `RegExpCursor` steps past an empty match it turns down right after a match
   * with CodeMirror's toCharEnd, so it never stands on the low surrogates
   * after it.
   */
  skipJumps: boolean;
  /**
   * A string match can start on the second half of a surrogate pair; it
   * belongs to the code point it starts in.
   */
  pointStarts: boolean;
}

function stateAt(rules: ChainRules, doc: Text, prevFrom: number, prevTo: number, pos: number): Resume | null {
  if (prevFrom < 0) {
    const start = rules.start(doc);
    if (pos <= start.pos) return pos === start.pos ? start : null;
  } else {
    const after = rules.after(doc, prevFrom, prevTo);
    if (after.pos >= pos) return after.pos === pos ? after : null;
    if (rules.skipJumps && after.skipEmpty && pos < toCharEnd(doc, after.pos + 1)) return null;
  }
  return rules.gapsResume || isLineStart(doc, pos) ? { pos, skipEmpty: false } : null;
}

function chainScanner(
  rules: ChainRules,
  limits: ScanLimits,
  scanner: Pick<QueryScanner, "restartBefore" | "settleAfter" | "reader">,
): QueryScanner {
  return {
    limits,
    start: rules.start,
    resumable: true,
    after: rules.after,
    stateAt: (doc, prevFrom, prevTo, pos) => stateAt(rules, doc, prevFrom, prevTo, pos),
    restartAt: (doc, from) => ({ pos: rules.pointStarts ? alignBack(doc, from) : from, skipEmpty: false }),
    ...scanner,
  };
}

function lineScanner(
  rules: ChainRules,
  limits: ScanLimits,
  reach: number,
  reader: (state: EditorState, at: Resume) => ChainReader,
): QueryScanner {
  return chainScanner(rules, limits, {
    restartBefore(doc, pos) {
      const line = doc.lineAt(pos);
      return line.length <= limits.longLine ? line.from : Math.max(line.from, alignBack(doc, pos - reach));
    },
    settleAfter(doc, pos) {
      const line = doc.lineAt(Math.min(pos, doc.length));
      return line.length <= limits.longLine ? line.to + 1 : Math.min(line.to + 1, pos + reach);
    },
    reader,
  });
}

/** Null for an empty query or a pattern that does not compile. */
export function createQueryScanner(
  query: SearchQuery,
  limits: ScanLimits = DEFAULT_SCAN_LIMITS,
): QueryScanner | null {
  if (!query.valid) return null;
  if (limits.window <= 2 * limits.reach) {
    throw new RangeError(`scan window (${limits.window}) must exceed twice the reach (${limits.reach})`);
  }
  if (query.test) {
    return {
      limits,
      start: chainStart,
      resumable: false,
      after: (_doc, _from, to) => ({ pos: to, skipEmpty: true }),
      stateAt: () => null,
      restartAt: () => CHAIN_START,
      restartBefore: () => 0,
      settleAfter: (doc) => doc.length + 1,
      reader: (state) => new CursorReader(query, state),
    };
  }
  if (query.regexp) {
    const compile = () => new RegExp(query.search, REGEXP_FLAGS + (query.caseSensitive ? "" : "i"));
    const wordTest = (state: EditorState) =>
      query.wholeWord ? regexpWordTest(state.charCategorizer(state.selection.main.head)) : null;
    if (MULTILINE_PATTERN.test(query.search)) {
      const rules: ChainRules = {
        start: (doc) => ({ pos: toCharEnd(doc, 0), skipEmpty: true }),
        after: (doc, from, to) => ({ pos: toCharEnd(doc, to + (from === to ? 1 : 0)), skipEmpty: true }),
        gapsResume: true,
        skipJumps: false,
        pointStarts: false,
      };
      return chainScanner(rules, limits, {
        restartBefore: (doc, pos) => alignBack(doc, pos - limits.reach),
        settleAfter: (_doc, pos) => pos + limits.reach,
        reader: (state, at) => new FlowRegexReader(compile(), wordTest(state), state.doc, at, limits),
      });
    }
    const rules: ChainRules = {
      start: (doc) => ({ pos: toCharEnd(doc, 0), skipEmpty: false }),
      after: (doc, from, to) => {
        const pos = toCharEnd(doc, to + (from === to ? 1 : 0));
        return { pos, skipEmpty: pos === to };
      },
      // A match the word test turns down is stepped over, unseen by the count.
      gapsResume: !query.wholeWord,
      skipJumps: true,
      pointStarts: false,
    };
    return lineScanner(
      rules,
      limits,
      limits.reach,
      (state, at) => new LineReader(new LineRegexKind(compile(), wordTest(state), state.doc, limits), state.doc, at, limits.window),
    );
  }
  const plan = planString(query);
  const reach = 2 * plan.folded.length + limits.clusterMargin;
  const wordTest = (state: EditorState) =>
    query.wholeWord ? makeWordTest(state.doc, state.charCategorizer(state.selection.main.head)) : null;
  const rules: ChainRules = {
    start: chainStart,
    after: (_doc, _from, to) => ({ pos: to, skipEmpty: false }),
    gapsResume: true,
    skipJumps: false,
    pointStarts: true,
  };
  const reader = (state: EditorState, at: Resume) => new FlowStringReader(plan, wordTest(state), state.doc, at, limits);
  if (!plan.folded.includes("\n")) return lineScanner(rules, limits, reach, reader);
  return chainScanner(rules, limits, {
    restartBefore(doc, pos) {
      const back = alignBack(doc, pos - reach);
      const line = doc.lineAt(back);
      return line.length <= limits.longLine ? line.from : back;
    },
    settleAfter: (_doc, pos) => pos + reach,
    reader,
  });
}
