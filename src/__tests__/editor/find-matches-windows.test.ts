import { describe, it, expect } from "vitest";
import { EditorState, type ChangeSpec, type Transaction } from "@codemirror/state";
import { SearchQuery } from "@codemirror/search";
import { MatchIndex, MATCH_COUNT_CAP } from "../../editor/search/match-index";
import { createQueryScanner, type ScanLimits } from "../../editor/search/scanner";

// The same comparison as find-matches-parity.test.ts, with windows a few dozen
// characters wide so that small documents cross every window edge: lines
// longer than `longLine`, patterns that cross lines, reads cut short between
// slices, and edits joined back onto what was read before them. Regular
// expressions here keep their matches and lookarounds within `reach`.
const SMALL: ScanLimits = { window: 48, reach: 12, longLine: 40, clusterMargin: 8, editRead: 20 };

interface Range {
  from: number;
  to: number;
}

function reference(state: EditorState, query: SearchQuery): Range[] {
  const out: Range[] = [];
  const cursor = query.getCursor(state);
  for (let step = cursor.next(); !step.done; step = cursor.next()) {
    out.push({ from: step.value.from, to: step.value.to });
  }
  return out;
}

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

const ALPHABET = [
  "a", "a", "b", "A", "B", "s", "S", "k", "K", "i", "I", "f", "e", "E", "x", "1", "_", " ", " ", "-", ".",
  "é", "É", "é", "́", "ﬀ", "ﬁ", "Σ", "σ", "ς", "İ", "ı", "ß", "😀", "𐐀", "中", "²", "\uD801", "\uDC00",
];

function randomLine(rand: () => number, length: number): string {
  let out = "";
  for (let i = 0; i < length; i++) out += ALPHABET[Math.floor(rand() * ALPHABET.length)];
  return out;
}

/** Lines of mixed length, a few of them past `SMALL.longLine`. */
function randomDocument(rand: () => number, lines: number): string {
  const out: string[] = [];
  for (let i = 0; i < lines; i++) {
    const long = rand() < 0.25;
    out.push(randomLine(rand, long ? 41 + Math.floor(rand() * 120) : Math.floor(rand() * 30)));
  }
  return out.join("\n");
}

const MODES = [
  { caseSensitive: false, wholeWord: false },
  { caseSensitive: true, wholeWord: false },
  { caseSensitive: false, wholeWord: true },
  { caseSensitive: true, wholeWord: true },
];

const FLOW_PATTERNS = [
  "a\\s+b", "\\s+", "\\s*", "[^a]b", "\\Wa", "\\d\\D", "a\\nb", "^\\s*$", "x\\s?\\n", "(?<=a)\\s", "\\s(?=b)",
  "[^\\s]+", "[^a-z ]+", "\\W+", "\\D{2}", "\\n", "[^\\n]$", "é\\s", "😀\\W",
];
const LINE_PATTERNS = [
  "a+", "x*", "\\bA\\w*", "[a-z]+", "a.b", "^a", "a$", "(?=a)", "(?<=b)a", "\\w+", "[aA]", "K|k", ".", "^", "$",
  "é", "😀", "\\p{L}{2}", "a-\\w+|-\\w+", "\\bs|s\\b",
];

function randomString(rand: () => number, doc: string): string {
  if (rand() < 0.7 && doc.length > 0) {
    const start = Math.floor(rand() * doc.length);
    const slice = doc.slice(start, start + 1 + Math.floor(rand() * 4)).replace(/\\/g, "");
    if (slice.trim().length > 0) return slice.replace(/\n/g, "\\n");
  }
  return randomLine(rand, 1 + Math.floor(rand() * 3)).trim() || "a";
}

function queriesFor(rand: () => number, doc: string): SearchQuery[] {
  const out: SearchQuery[] = [];
  for (const mode of MODES) {
    out.push(new SearchQuery({ search: randomString(rand, doc), ...mode }));
    out.push(new SearchQuery({ search: FLOW_PATTERNS[Math.floor(rand() * FLOW_PATTERNS.length)], regexp: true, ...mode }));
    out.push(new SearchQuery({ search: LINE_PATTERNS[Math.floor(rand() * LINE_PATTERNS.length)], regexp: true, ...mode }));
  }
  return out;
}

function everyQuery(rand: () => number, doc: string): SearchQuery[] {
  const out: SearchQuery[] = [];
  for (const mode of MODES) {
    for (const search of FLOW_PATTERNS) out.push(new SearchQuery({ search, regexp: true, ...mode }));
    for (const search of LINE_PATTERNS) out.push(new SearchQuery({ search, regexp: true, ...mode }));
    for (let i = 0; i < 3; i++) out.push(new SearchQuery({ search: randomString(rand, doc), ...mode }));
  }
  return out;
}

function describeQuery(query: SearchQuery): string {
  return JSON.stringify({
    search: query.search,
    regexp: query.regexp,
    caseSensitive: query.caseSensitive,
    wholeWord: query.wholeWord,
  });
}

function readAll(query: SearchQuery, state: EditorState, limits: ScanLimits): MatchIndex {
  const index = new MatchIndex(createQueryScanner(query, limits)!);
  index.advance(state, Infinity);
  return index;
}

function expectReference(index: MatchIndex, state: EditorState, query: SearchQuery, label: string): void {
  const expected = reference(state, query);
  expect(index.settled, label).toBe(true);
  expect(index.ranges(), label).toEqual(expected.slice(0, MATCH_COUNT_CAP));
}

/** What has been read so far is the start of what a full read finds. */
function expectPrefix(index: MatchIndex, state: EditorState, query: SearchQuery, label: string): void {
  const got = index.ranges();
  expect(got, label).toEqual(reference(state, query).slice(0, got.length));
}

function randomChange(rand: () => number, docLength: number): ChangeSpec {
  const from = Math.floor(rand() * (docLength + 1));
  const to = Math.min(docLength, from + (rand() < 0.5 ? 0 : Math.floor(rand() * 20)));
  const roll = rand();
  const insert =
    roll < 0.25 ? "" : roll < 0.4 ? randomDocument(rand, 2 + Math.floor(rand() * 3)) : randomLine(rand, 1 + Math.floor(rand() * 8));
  return { from, to, insert };
}

describe("find reads in windows what CodeMirror's cursor finds", () => {
  it("over documents with long lines, for strings, line patterns and patterns that cross lines", () => {
    const rand = prng(20261013);
    for (let round = 0; round < 250; round++) {
      const doc = randomDocument(rand, 3 + Math.floor(rand() * 8));
      const state = EditorState.create({ doc });
      for (const query of queriesFor(rand, doc)) {
        expectReference(readAll(query, state, SMALL), state, query, `round ${round} ${describeQuery(query)}`);
      }
    }
  });

  it("when every read stops at its first chance", () => {
    const rand = prng(31);
    for (let round = 0; round < 120; round++) {
      const doc = randomDocument(rand, 4 + Math.floor(rand() * 8));
      const state = EditorState.create({ doc });
      for (const query of queriesFor(rand, doc)) {
        const label = `round ${round} ${describeQuery(query)}`;
        const index = new MatchIndex(createQueryScanner(query, SMALL)!);
        for (let reads = 0; !index.settled; reads++) {
          expect(reads, label).toBeLessThan(10_000);
          index.advance(state, -Infinity);
          expectPrefix(index, state, query, label);
        }
        expectReference(index, state, query, label);
      }
    }
  });
});

describe("an edit joins what was read past it", () => {
  it("through random edits, between reads cut short and full reads", () => {
    type Op = { kind: "edit"; tr: Transaction } | { kind: "slice" } | { kind: "full" };
    const rand = prng(97);
    for (let run = 0; run < 24; run++) {
      const first = EditorState.create({ doc: randomDocument(rand, 4 + Math.floor(rand() * 7)) });
      const ops: Op[] = [];
      let state = first;
      for (let step = 0; step < 24; step++) {
        const roll = rand();
        if (roll < 0.5) {
          const changes = [randomChange(rand, state.doc.length)];
          if (rand() < 0.3) {
            const second = randomChange(rand, state.doc.length) as { from: number; to?: number };
            const head = changes[0] as { from: number; to?: number };
            if (second.from > (head.to ?? head.from)) changes.push(second);
          }
          const tr = state.update({ changes });
          ops.push({ kind: "edit", tr });
          state = tr.state;
        } else {
          ops.push({ kind: roll < 0.9 ? "slice" : "full" });
        }
      }
      for (const query of everyQuery(rand, first.doc.toString())) {
        const references = new Map<EditorState, Range[]>();
        const referenceOf = (at: EditorState) => {
          let found = references.get(at);
          if (!found) references.set(at, (found = reference(at, query)));
          return found;
        };
        const index = new MatchIndex(createQueryScanner(query, SMALL)!);
        let current = first;
        if (rand() < 0.5) index.advance(current, Infinity);
        ops.forEach((op, step) => {
          if (op.kind === "edit") {
            index.applyChanges(op.tr.changes, op.tr.startState.doc, op.tr.state);
            current = op.tr.state;
          } else {
            index.advance(current, op.kind === "slice" ? -Infinity : Infinity);
          }
          const got = index.ranges();
          const expected = referenceOf(current);
          const label = `run ${run} step ${step} ${describeQuery(query)}`;
          expect(got, label).toEqual(expected.slice(0, got.length));
          if (op.kind === "full") expect(got, label).toEqual(expected.slice(0, MATCH_COUNT_CAP));
        });
        index.advance(current, Infinity);
        expect(index.ranges(), `run ${run} end ${describeQuery(query)}`).toEqual(referenceOf(current).slice(0, MATCH_COUNT_CAP));
      }
    }
  });

  it("never picks a regexp cursor up on the low surrogate it stepped over", () => {
    // `x*` takes "xx", turns down the empty match right after it and steps
    // past the low surrogate that follows. The edit's restart point falls on it.
    const doc = `${"y".repeat(30)}xxy\uDC00${"y".repeat(60)}`;
    const query = new SearchQuery({ search: "x*", regexp: true });
    let state = EditorState.create({ doc });
    const index = readAll(query, state, SMALL);
    const tr = state.update({ changes: { from: 33 + SMALL.reach, insert: "y" } });
    index.applyChanges(tr.changes, tr.startState.doc, tr.state);
    state = tr.state;
    index.advance(state, Infinity);
    expectReference(index, state, query, "x* over a lone low surrogate");
  });

  it("picks a string query up at the code point a match starts in", () => {
    // "\uDE00x" matches from the second half of the emoji; the edit's restart
    // point falls inside that match and the line becomes short enough to read whole.
    const doc = `ab😀x${"y".repeat(56)}`;
    const query = new SearchQuery({ search: "\uDE00x" });
    let state = EditorState.create({ doc });
    const index = readAll(query, state, SMALL);
    expect(index.ranges()).toEqual([{ from: 3, to: 5 }]);
    const tr = state.update({ changes: { from: 16, to: 41 } });
    index.applyChanges(tr.changes, tr.startState.doc, tr.state);
    state = tr.state;
    index.advance(state, Infinity);
    expectReference(index, state, query, "a match starting inside a code point");
  });

  it("never picks a whole-word pattern up inside a match the word test turned down", () => {
    // "a-bc" is turned down (it starts inside a word) and stepped over; read
    // from inside it, "-bc" would pass.
    const doc = `${"z ".repeat(15)}xa-bc${" z".repeat(40)}`;
    const query = new SearchQuery({ search: "a-\\w+|-\\w+", regexp: true, wholeWord: true });
    let state = EditorState.create({ doc });
    const index = readAll(query, state, SMALL);
    expect(index.count).toBe(0);
    const tr = state.update({ changes: { from: 32 + SMALL.reach, insert: "z" } });
    index.applyChanges(tr.changes, tr.startState.doc, tr.state);
    state = tr.state;
    index.advance(state, Infinity);
    expectReference(index, state, query, "whole word inside a turned-down match");
  });

  it("past the cap, where the reading stops right after the last counted match", () => {
    const rand = prng(5);
    const doc = Array.from({ length: 5200 }, () => "a b a b c").join("\n");
    for (const query of [
      new SearchQuery({ search: "a" }),
      new SearchQuery({ search: "\\s", regexp: true }),
      new SearchQuery({ search: "[ab]", regexp: true }),
      new SearchQuery({ search: "c\\na" }),
    ]) {
      let state = EditorState.create({ doc });
      const index = readAll(query, state, SMALL);
      for (let step = 0; step < 12; step++) {
        const label = `${describeQuery(query)} step ${step}`;
        const at = Math.floor(rand() * state.doc.length);
        const tr = state.update({ changes: { from: at, to: Math.min(state.doc.length, at + Math.floor(rand() * 40)), insert: rand() < 0.5 ? "" : "a b\nc" } });
        index.applyChanges(tr.changes, tr.startState.doc, tr.state);
        state = tr.state;
        expectPrefix(index, state, query, label);
        index.advance(state, Infinity);
        const expected = reference(state, query);
        expect(index.capped, label).toBe(expected.length >= MATCH_COUNT_CAP);
        expectReference(index, state, query, label);
      }
    }
  });
});

describe("how much one read and one edit take", () => {
  const LIMITS: ScanLimits = { window: 4096, reach: 256, longLine: 4096, clusterMargin: 64, editRead: 8192 };
  const filler = "lorem ipsum dolor sit amet, consectetur adipiscing elit sed do\n";
  const document = filler.repeat(Math.ceil((2 * 1024 * 1024) / filler.length));
  const singleLine = `{"items":[${Array.from({ length: 40_000 }, (_, i) => `{"id":${i},"name":"café ${i}"}`).join(",")}]}`;
  const cases = [
    { name: "a pattern that crosses lines", doc: document, query: new SearchQuery({ search: "\\s+qxjzv", regexp: true }) },
    { name: "a pattern with a negated class", doc: document, query: new SearchQuery({ search: "[^ ]qxjzv", regexp: true }) },
    { name: "a string with a line break", doc: document, query: new SearchQuery({ search: "elit\\nqxjzv" }) },
    { name: "a word on one long line with accents", doc: singleLine, query: new SearchQuery({ search: "qxjzv" }) },
    { name: "a pattern on one long line", doc: singleLine, query: new SearchQuery({ search: "qx\\w+", regexp: true }) },
  ];

  for (const { name, doc, query } of cases) {
    it(`${name}: reads a window at a time, and an edit reads only around itself`, () => {
      let state = EditorState.create({ doc });
      const index = new MatchIndex(createQueryScanner(query, LIMITS)!);
      let reads = 0;
      while (!index.settled) {
        index.advance(state, -Infinity);
        reads++;
      }
      expect(reads).toBeGreaterThan(doc.length / (2 * LIMITS.window));
      expect(index.count).toBe(0);

      for (const at of [state.doc.length, 0, Math.floor(state.doc.length / 2)]) {
        const tr = state.update({ changes: { from: at, insert: " qxjzv" } });
        index.applyChanges(tr.changes, tr.startState.doc, tr.state);
        state = tr.state;
        expect(index.settled, `edit at ${at}`).toBe(true);
      }
      expectReference(index, state, query, name);
    });
  }
});
