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

interface StringPlan {
  folded: string;
  ascii: boolean;
  fold: CodePointFold;
  caseInsensitive: boolean;
  asciiPattern: RegExp | null;
}

function planString(query: SearchQuery): StringPlan {
  const fold = query.caseSensitive ? FOLD_CASE_SENSITIVE : FOLD_CASE_INSENSITIVE;
  const folded = fold.fold(unquoteSearch(query));
  const ascii = !NON_ASCII.test(folded);
  const asciiPattern =
    ascii && !query.caseSensitive
      ? new RegExp(folded.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "gi")
      : null;
  return { folded, ascii, fold, caseInsensitive: !query.caseSensitive, asciiPattern };
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
  for (let i = 0; i < text.length; ) {
    const code = text.charCodeAt(i);
    if (code < 0x80) {
      i++;
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

/**
 * The string matches `SearchCursor` takes from `text` (at document position
 * `base`) whose first code point starts in [ownFrom, ownTo) and at or after
 * `restart`, in order.
 *
 * `SearchCursor` compares each code point's folded form with the folded query.
 * A match starts at the code point holding its first folded unit (one past it
 * when the code point's own leading unit equals the folded one) and ends at
 * the end of the code point holding its last unit. Once a match is taken, the
 * next one starts at a later code point; a match the word test turns down
 * leaves the overlapping ones in play.
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
): WindowScan {
  const query = plan.folded;
  const take = (from: number, to: number): boolean | null => {
    if (test) {
      const { chunk, start } = chunkAt(to);
      if (!test(from, to, chunk, start)) return null;
    }
    return sink(from, to);
  };

  if (!NON_ASCII.test(text)) {
    if (!plan.ascii) return { restart, stopped: false };
    const limit = ownTo - base;
    let at = Math.max(restart, ownFrom) - base;
    for (;;) {
      let found: number;
      if (plan.asciiPattern) {
        plan.asciiPattern.lastIndex = at;
        const match = plan.asciiPattern.exec(text);
        found = match ? match.index : -1;
      } else {
        found = text.indexOf(query, at);
      }
      if (found < 0 || found >= limit) return { restart, stopped: false };
      const from = base + found;
      const to = from + query.length;
      const taken = take(from, to);
      if (taken === null) {
        at = found + 1;
        continue;
      }
      restart = to;
      if (!taken) return { restart, stopped: true };
      at = found + query.length;
    }
  }

  const { folded, pieces } = foldWindow(text, plan);
  for (let at = 0; ; ) {
    const found = folded.indexOf(query, at);
    if (found < 0) return { restart, stopped: false };
    const first = pieceAt(pieces, found);
    const firstSpan = pieces.span[first];
    const startPoint = base + pieces.offset[first] + (firstSpan === 0 ? found - pieces.unit[first] : 0);
    if (startPoint >= ownTo) return { restart, stopped: false };
    if (startPoint < ownFrom || startPoint < restart) {
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
    restart = to;
    if (!taken) return { restart, stopped: true };
    at = lastUnit + 1;
  }
}

function lineChunk(text: string, base: number): ChunkAt {
  return () => ({ chunk: text, start: base });
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

class LineStringKind implements LineKind {
  constructor(
    private readonly plan: StringPlan,
    private readonly test: WordTest | null,
    private readonly limits: ScanLimits,
  ) {}

  readLine(text: string, lineFrom: number, at: Resume, sink: ReadSink, reader: LineReader): boolean {
    const take: MatchSink = (from, to) => {
      reader.resume = { pos: to, skipEmpty: false };
      return sink.match(from, to) && (to < sink.watchFrom || sink.free(reader.resume));
    };
    const chunk = lineChunk(text, lineFrom);
    if (text.length <= this.limits.longLine) {
      if (text.length < this.plan.folded.length && !NON_ASCII.test(text)) return true;
      return !scanStringWindow(this.plan, this.test, chunk, text, lineFrom, at.pos, Infinity, at.pos, take).stopped;
    }
    const lineTo = lineFrom + text.length;
    const span = 2 * this.plan.folded.length;
    let restart = at.pos;
    for (let start = at.pos; start < lineTo; ) {
      let end = Math.min(lineTo, start + this.limits.window);
      if (splitsPair(text, end - lineFrom)) end++;
      const windowText = text.slice(start - lineFrom, Math.min(text.length, end - lineFrom + span));
      const last = end === lineTo;
      const scan = scanStringWindow(this.plan, this.test, chunk, windowText, start, start, last ? Infinity : end, restart, take);
      if (scan.stopped) return false;
      restart = scan.restart;
      start = end;
      if (last) break;
      reader.resume = { pos: Math.max(end, restart), skipEmpty: false };
      if (!sink.free(reader.resume)) return false;
    }
    return true;
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
      const text = doc.sliceString(start, Math.min(doc.length, end + span));
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
  kind: (state: EditorState) => LineKind,
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
    reader: (state, at) => new LineReader(kind(state), state.doc, at, limits.window),
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
    return lineScanner(rules, limits, limits.reach, (state) => new LineRegexKind(compile(), wordTest(state), state.doc, limits));
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
  if (!plan.folded.includes("\n")) {
    return lineScanner(rules, limits, reach, (state) => new LineStringKind(plan, wordTest(state), limits));
  }
  return chainScanner(rules, limits, {
    restartBefore(doc, pos) {
      const back = alignBack(doc, pos - reach);
      const line = doc.lineAt(back);
      return line.length <= limits.longLine ? line.from : back;
    },
    settleAfter: (_doc, pos) => pos + reach,
    reader: (state, at) => new FlowStringReader(plan, wordTest(state), state.doc, at, limits),
  });
}
