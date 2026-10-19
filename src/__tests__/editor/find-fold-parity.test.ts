import { describe, it, expect } from "vitest";
import { CharCategory, EditorState, codePointAt, codePointSize, findClusterBreak, type Text } from "@codemirror/state";
import { SearchQuery } from "@codemirror/search";
import { MatchIndex, MATCH_COUNT_CAP } from "../../editor/search/match-index";
import { DEFAULT_SCAN_LIMITS, createQueryScanner, unquoteSearch, type ScanLimits } from "../../editor/search/scanner";

// A literal query's matches over text in many scripts, against the scanner as
// it stood before ASCII runs were skipped and plain text was read by a native
// search: the per-line fold below is a copy of that code, kept as the
// reference. Every comparison is of match starts and ends.

interface Range {
  from: number;
  to: number;
}

const SMALL: ScanLimits = { window: 48, reach: 12, longLine: 40, clusterMargin: 8, editRead: 20 };
// A window holding whole nodes of the document past the size their own text is listed from.
const WIDE: ScanLimits = { ...DEFAULT_SCAN_LIMITS, window: 256 * 1024 };

// --- Reference: the fold and the line scan before the change, unchanged. ---

const NON_ASCII = /[^\x00-\x7f]/;
const UNKNOWN = 0;
const UNCHANGED = 1;
const CHANGED = 2;

class ReferenceFold {
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

const REFERENCE_SENSITIVE = new ReferenceFold(false);
const REFERENCE_INSENSITIVE = new ReferenceFold(true);

interface ReferencePlan {
  folded: string;
  ascii: boolean;
  fold: ReferenceFold;
  caseInsensitive: boolean;
  asciiPattern: RegExp | null;
}

function referencePlan(query: SearchQuery): ReferencePlan {
  const fold = query.caseSensitive ? REFERENCE_SENSITIVE : REFERENCE_INSENSITIVE;
  const folded = fold.fold(unquoteSearch(query));
  const ascii = !NON_ASCII.test(folded);
  const asciiPattern =
    ascii && !query.caseSensitive ? new RegExp(folded.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "gi") : null;
  return { folded, ascii, fold, caseInsensitive: !query.caseSensitive, asciiPattern };
}

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

function referenceFoldWindow(text: string, plan: ReferencePlan): { folded: string; pieces: Pieces } {
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

type Categorizer = (char: string) => CharCategory;

const charBefore = (str: string, index: number) => str.slice(findClusterBreak(str, index, false), index);
const charAfter = (str: string, index: number) => str.slice(index, findClusterBreak(str, index));

function referenceWordTest(doc: Text, categorizer: Categorizer) {
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

/** The old `scanStringWindow` over one whole line, as the old line reader called it. */
function referenceLine(
  plan: ReferencePlan,
  test: ReturnType<typeof referenceWordTest> | null,
  text: string,
  base: number,
  out: Range[],
): void {
  const query = plan.folded;
  const take = (from: number, to: number): boolean => {
    if (test && !test(from, to, text, base)) return false;
    out.push({ from, to });
    return true;
  };
  let restart = base;
  if (!NON_ASCII.test(text)) {
    if (!plan.ascii) return;
    let at = 0;
    for (;;) {
      let found: number;
      if (plan.asciiPattern) {
        plan.asciiPattern.lastIndex = at;
        const match = plan.asciiPattern.exec(text);
        found = match ? match.index : -1;
      } else {
        found = text.indexOf(query, at);
      }
      if (found < 0) return;
      const from = base + found;
      if (!take(from, from + query.length)) {
        at = found + 1;
        continue;
      }
      at = found + query.length;
    }
  }
  const { folded, pieces } = referenceFoldWindow(text, plan);
  for (let at = 0; ; ) {
    const found = folded.indexOf(query, at);
    if (found < 0) return;
    const first = pieceAt(pieces, found);
    const firstSpan = pieces.span[first];
    const startPoint = base + pieces.offset[first] + (firstSpan === 0 ? found - pieces.unit[first] : 0);
    if (startPoint < restart) {
      at = found + 1;
      continue;
    }
    const from =
      firstSpan === 0 ? startPoint : startPoint + Math.min(found - pieces.unit[first], pieces.prefix[first]);
    const lastUnit = found + query.length - 1;
    const last = pieceAt(pieces, lastUnit);
    const lastSpan = pieces.span[last];
    const to = base + pieces.offset[last] + (lastSpan === 0 ? lastUnit - pieces.unit[last] + 1 : lastSpan);
    if (!take(from, to)) {
      at = found + 1;
      continue;
    }
    restart = to;
    at = lastUnit + 1;
  }
}

function reference(state: EditorState, query: SearchQuery): Range[] {
  const plan = referencePlan(query);
  const test = query.wholeWord
    ? referenceWordTest(state.doc, state.charCategorizer(state.selection.main.head))
    : null;
  const out: Range[] = [];
  let lineFrom = 0;
  for (const lines = state.doc.iterLines(); !lines.next().done; ) {
    referenceLine(plan, test, lines.value, lineFrom, out);
    lineFrom += lines.value.length + 1;
  }
  return out;
}

// --- The comparison. ---

function current(state: EditorState, query: SearchQuery, limits: ScanLimits): MatchIndex {
  const index = new MatchIndex(createQueryScanner(query, limits)!);
  index.advance(state, Infinity);
  return index;
}

function expectSameMatches(state: EditorState, query: SearchQuery, limits: ScanLimits, context: string): void {
  const expected = reference(state, query);
  const index = current(state, query, limits);
  const label = `${context} ${JSON.stringify({
    search: query.search,
    caseSensitive: query.caseSensitive,
    wholeWord: query.wholeWord,
    window: limits.window,
  })}`;
  expect(index.capped, label).toBe(expected.length >= MATCH_COUNT_CAP);
  expect(index.ranges(), label).toEqual(expected.slice(0, MATCH_COUNT_CAP));
}

const MODES = [
  { caseSensitive: false, wholeWord: false },
  { caseSensitive: true, wholeWord: false },
  { caseSensitive: false, wholeWord: true },
  { caseSensitive: true, wholeWord: true },
];

const FRAGMENTS = [
  // ASCII
  "the", "The", "zebrafish", "ZEBRAFISH", "cafe", "resume", "naive", "strasse", "istanbul", "fi", "ffi", "ss", "s",
  "e", "i", "a-a", "foo_bar", "x1", "status=500", "kg", "tel", "xii", "zebra", ",", ".", ":", "(", ")", "-", "  ",
  // German
  "Straße", "STRASSE", "groß", "GROẞ", "Fußgänger", "Äpfel", "über", "ÜBER", "ß", "ẞ",
  // Turkish
  "İstanbul", "ISTANBUL", "ıslak", "IŞIK", "işık", "Diyarbakır", "İ", "ı", "i\u0307",
  // Greek, final sigma
  "ΟΔΟΣ", "οδος", "οδός", "Σίσυφος", "ΣΊΣΥΦΟΣ", "ς", "σ", "Σ", "ΣΑΣ",
  // Arabic with harakat, presentation forms
  "مَرْحَبًا", "السَّلَامُ", "عَلَيْكُمْ", "كِتَابٌ", "كتاب", "ﻻ", "ﷺ", "ﷲ", "\u0652", "\u064e",
  // Combining marks, precomposed and decomposed
  "café", "cafe\u0301", "naïve", "résumé", "e\u0301", "a\u0323\u0301", "a\u0301\u0323", "n\u0303", "ñ", "\u0301", "\u00c5", "\u212b",
  "\u212a", "k", "Ǆ", "ǅ", "ǆ", "ﬁ", "ﬀ", "ﬃ", "ﬁle", "²", "½", "Ⅻ", "℡", "㎏",
  // Surrogate pairs and emoji
  "😀", "👩\u200d💻", "🇩🇪", "𐐀", "𐐨", "𝐀𝐁𝐂", "𝕫𝕖𝕓𝕣𝕒", "𝕫", "１２３", "ｚｅｂｒａ", "Ａ",
  // CJK, fullwidth punctuation and compatibility forms
  "中文", "日本語", "한국어", "東京", "，", "：", "（", "）", "。", "\uf900", "ｶﾀｶﾅ", "カタカナ",
];

function prng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const pick = <T,>(rand: () => number, values: readonly T[]): T => values[Math.floor(rand() * values.length)];

/** Lines of mixed scripts, some of them past `SMALL.longLine`, a few of plain ASCII, about `size` characters. */
function mixedCorpus(rand: () => number, size: number): string {
  const lines: string[] = [];
  let length = 0;
  while (length < size) {
    const roll = rand();
    const words = roll < 0.1 ? 0 : roll < 0.8 ? 2 + Math.floor(rand() * 10) : 20 + Math.floor(rand() * 60);
    const asciiOnly = rand() < 0.2;
    let line = "";
    for (let i = 0; i < words; i++) {
      let fragment = pick(rand, FRAGMENTS);
      if (asciiOnly) while (NON_ASCII.test(fragment)) fragment = pick(rand, FRAGMENTS);
      line += i === 0 ? fragment : (rand() < 0.8 ? " " : "") + fragment;
    }
    lines.push(line);
    length += line.length + 1;
  }
  return lines.join("\n");
}

const QUERIES = [
  "the", "THE", "zebrafish", "zebrafis", "zebrafi", "zebra", "cafe", "café", "cafe\u0301", "resume", "résumé", "naive",
  "naïve", "e", "i", "s", "ss", "fi", "ffi", "file", "ﬁ", "ﬁle", "strasse", "straße", "STRASSE", "groß", "ß", "ẞ", "ü",
  "istanbul", "İstanbul", "i\u0307stanbul", "ı", "İ", "işık", "IŞIK", "οδος", "ΟΔΟΣ", "σ", "ς", "Σ", "σας", "مرحبا",
  "مَرْحَبًا", "كتاب", "\u0652", "\u064e", "لا", "ﻻ", "صلى", " ", "\u0301", "\u0323\u0301", "n\u0303", "ñ", "å", "Å", "\u212b", "k", "K", "\u212a",
  "dž", "ǆ", "Ǆ", "2", "1⁄2", "xii", "tel", "kg", "😀", "💻", "🇩", "𐐨", "𐐀", "abc", "ABC", "𝐀", "𝕫𝕖", "123", "ｚ",
  "中文", "日本", "한국어", "東京", ",", "，", ":", "(", "()", "カタ", "ｶﾀ", "a-a", "foo_bar", "x1", "status=500", "qxjzv",
];

function randomQueries(rand: () => number, doc: string, count: number): string[] {
  const out: string[] = [];
  while (out.length < count) {
    const start = Math.floor(rand() * doc.length);
    const slice = doc.slice(start, start + 1 + Math.floor(rand() * 6));
    if (slice.includes("\n") || slice.includes("\\") || !slice.trim() || /[\uD800-\uDFFF]/.test(slice)) continue;
    out.push(slice);
  }
  return out;
}

describe("a literal query over text in many scripts finds what the fold found before", () => {
  const rand = prng(20261019);
  const doc = mixedCorpus(rand, 96 * 1024);
  const queries = [...QUERIES, ...randomQueries(rand, doc, 40)];

  for (const limits of [DEFAULT_SCAN_LIMITS, SMALL, WIDE]) {
    it(`for every query and mode, read in windows of ${limits.window}`, () => {
      const state = EditorState.create({ doc });
      for (const search of queries) {
        for (const mode of MODES) expectSameMatches(state, new SearchQuery({ search, ...mode }), limits, "mixed");
      }
    });
  }

  it("for every query and mode, a second time over the same document and after edits to it", () => {
    let state = EditorState.create({ doc });
    const editRand = prng(7);
    for (let round = 0; round < 3; round++) {
      for (const search of QUERIES) {
        for (const mode of MODES) {
          expectSameMatches(state, new SearchQuery({ search, ...mode }), DEFAULT_SCAN_LIMITS, `round ${round}`);
        }
      }
      const changes = Array.from({ length: 6 }, () => {
        const from = Math.floor(editRand() * state.doc.length);
        return { from, to: Math.min(state.doc.length, from + Math.floor(editRand() * 40)), insert: mixedCorpus(editRand, 30) };
      }).sort((a, b) => a.from - b.from);
      const disjoint = changes.filter((change, i) => i === 0 || change.from >= changes[i - 1].to);
      state = state.update({ changes: disjoint }).state;
    }
  });

  it("over lines of plain ASCII with the odd code point that folds to ASCII", () => {
    const lines = Array.from({ length: 3000 }, (_, i) => {
      const extra = i % 97 === 0 ? " ｚebrafish" : i % 89 === 0 ? " zebraﬁsh" : i % 83 === 0 ? " cafe\u0301" : i % 7 === 0 ? " café" : "";
      return `line ${i} of the zebrafish notes${extra}`;
    });
    const state = EditorState.create({ doc: lines.join("\n") });
    const searches = ["zebrafish", "zebrafis", "zebrafi", "afis", "ish", "fish", "cafe", "notes cafe", "s cafe", "fe", "qxjzve"];
    for (const search of [...searches, "café", "e", "f", "ﬁ", "ｚ", "z", "s n"]) {
      for (const mode of MODES) {
        for (const limits of [DEFAULT_SCAN_LIMITS, SMALL]) {
          expectSameMatches(state, new SearchQuery({ search, ...mode }), limits, "ascii lines");
        }
      }
    }
  });
});
