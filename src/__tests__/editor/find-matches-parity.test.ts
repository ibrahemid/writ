import { describe, it, expect } from "vitest";
import { EditorState, type ChangeSpec, type Extension } from "@codemirror/state";
import { SearchQuery } from "@codemirror/search";
import { MatchIndex, MATCH_COUNT_CAP } from "../../editor/search/match-index";
import { DEFAULT_SCAN_LIMITS, createQueryScanner } from "../../editor/search/scanner";
import { generateCorpusText } from "./fixtures/editor-corpus";

// Find's count must be the count CodeMirror's own cursor gives: the reference
// here is an uncapped walk of `SearchQuery.getCursor(state)`, the cursor the
// highlighter and Enter use. Every comparison is of positions, not totals.

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

function engine(state: EditorState, query: SearchQuery): MatchIndex | null {
  const scanner = createQueryScanner(query);
  if (!scanner) return null;
  const index = new MatchIndex(scanner);
  index.advance(state, Infinity);
  return index;
}

function expectSameMatches(state: EditorState, query: SearchQuery, context: string): void {
  const expected = reference(state, query);
  const index = engine(state, query);
  const got = index ? index.ranges() : [];
  const label = `${context} ${JSON.stringify({
    search: query.search,
    caseSensitive: query.caseSensitive,
    wholeWord: query.wholeWord,
    regexp: query.regexp,
  })}`;
  if (expected.length >= MATCH_COUNT_CAP) {
    expect(index?.capped, label).toBe(true);
    expect(got, label).toEqual(expected.slice(0, MATCH_COUNT_CAP));
  } else {
    expect(index?.capped ?? false, label).toBe(false);
    expect(got, label).toEqual(expected);
  }
}

const MODES = [
  { caseSensitive: false, wholeWord: false },
  { caseSensitive: true, wholeWord: false },
  { caseSensitive: false, wholeWord: true },
  { caseSensitive: true, wholeWord: true },
];

function stateOf(doc: string, extensions: Extension = []): EditorState {
  return EditorState.create({ doc, extensions });
}

const UNICODE_FIXTURE = [
  "Café, café, CAFÉ and café on one line; naïve résumé.",
  "ﬀ and ﬁle and ﬄ: the ligatures fold to ff, fi, ffl.",
  "ΟΔΟΣ, οδος, Σίσυφος and σοφός; ΣΣ σσ ςς.",
  "İstanbul, istanbul, ıssız, ISTANBUL; Kelvin K and k and K; long ſ and s.",
  "Straße, STRASSE, strasse, ẞ; x² and x2; Ｆｕｌｌ ｗｉｄｔｈ and full width.",
  "😀 emoji 😀😀 and 𐐀𐐨 Deseret; family 👨‍👩‍👧 and flag 🇸🇦.",
  "مرحبا بالعالم ﻻ لا; 中文 字 豈 豈; ǅungla ǆ Ǆ.",
  "ạ́ ạ́ á ạ́; snake_case foo_bar foo1 1foo foo.bar foo-bar.",
  "",
  "   leading spaces, trailing  ",
  "word word-word word_word wordword WORD Word.",
].join("\n");

const STRING_QUERIES = [
  "cafe", "café", "café", "CAFÉ", "é", "e", "é", "́",
  "ff", "fi", "f", "ffl", "ﬀ", "ﬁle",
  "σ", "ς", "Σ", "οδος", "ΟΔΟΣ", "σσ",
  "i", "I", "İ", "ı", "istanbul", "k", "K", "s", "ss", "ß", "strasse", "ẞ",
  "2", "x2", "full", "ｆｕｌｌ",
  "😀", "😀😀", "𐐀", "𐐨", "👨", "\uD83D", "\uDE00", "\uDE00 ",
  "لا", "ﻻ", "中", "豈", "dz", "Ǆ",
  "a", "á", "ạ", "word", "foo", "_", "-", " ", "  ", ".",
  "word\\nword", "\\n", ".\\n", "\\t", "\\\\",
];

const REGEX_QUERIES = [
  "caf[eé]", "\\w+", "[a-z]+", "word", "^word", "word$", "\\bword\\b", "o*", "x?", "(?=o)", "(?<=f)o",
  ".", "\\p{L}+", "[^a-z ]+", "\\s+", "e\\u0301", "\\d", "😀", "\\n", "foo.bar", "^$", "a|b", "(ff|fi)",
];

describe("find counts what CodeMirror's cursor finds", () => {
  it("over a Unicode fixture, for every string query and mode", () => {
    const state = stateOf(UNICODE_FIXTURE);
    for (const search of STRING_QUERIES) {
      for (const mode of MODES) {
        expectSameMatches(state, new SearchQuery({ search, ...mode }), "unicode string");
      }
    }
  });

  it("over a Unicode fixture, for every regular expression and mode", () => {
    const state = stateOf(UNICODE_FIXTURE);
    for (const search of REGEX_QUERIES) {
      for (const mode of MODES) {
        expectSameMatches(state, new SearchQuery({ search, regexp: true, ...mode }), "unicode regexp");
      }
    }
  });

  it("over the generated documents, for words, rare words, absent words and patterns", () => {
    const documents = [
      generateCorpusText({ name: "prose", kind: "prose", size: 256 * 1024 }),
      generateCorpusText({ name: "notes", kind: "markdown", size: 256 * 1024, sectionBytes: 10_480 }),
      generateCorpusText({ name: "server", kind: "log", size: 0, lines: 3_000 }),
      generateCorpusText({ name: "one-line", kind: "single-line-json", size: 64 * 1024 }),
    ];
    const strings = ["zebrafish", "the", "THE", "résumé", "resume", "café", "naïve", "qxjzv", "e", "status=500", "\\n\\n"];
    const patterns = ["zebra\\w+", "status=5\\d\\d", "\\[\\[Note \\d+\\]\\]", "^#+ ", "\\s{2,}", "[^\\n]{200,}", "é"];
    for (const doc of documents) {
      const state = stateOf(doc);
      for (const mode of MODES) {
        for (const search of strings) expectSameMatches(state, new SearchQuery({ search, ...mode }), "corpus string");
        for (const search of patterns) {
          expectSameMatches(state, new SearchQuery({ search, regexp: true, ...mode }), "corpus regexp");
        }
      }
    }
  });

  it("tries the overlapping candidates a whole-word match turns down", () => {
    const state = stateOf(["ba-a-a", "ba-a-a é", "xa-a-a-a", "é ba-a-a-a"].join("\n"));
    for (const search of ["a-a", "a-a-a"]) {
      for (const mode of MODES) expectSameMatches(state, new SearchQuery({ search, ...mode }), "overlap");
    }
  });

  it("carries a match across the blocks a multi-line query is read in", () => {
    const ascii = ["b", ...Array.from({ length: 4500 }, () => "a")].join("\n");
    const mixed = ["b", ...Array.from({ length: 4500 }, (_, i) => (i % 700 === 0 ? "é" : "a"))].join("\n");
    for (const doc of [ascii, mixed]) {
      const state = stateOf(doc);
      for (const search of ["a\\na", "a\\na\\na", "b\\na", "é\\na"]) {
        for (const mode of MODES) expectSameMatches(state, new SearchQuery({ search, ...mode }), "blocks");
      }
    }
  });

  it("reads word characters the language adds, at the selection head", () => {
    const doc = "foo-bar foo bar-foo foo_bar";
    const hyphenated = EditorState.languageData.of(() => [{ wordChars: "-" }]);
    for (const extensions of [[], hyphenated]) {
      const state = stateOf(doc, extensions);
      for (const search of ["foo", "bar", "-"]) {
        for (const mode of MODES) {
          expectSameMatches(state, new SearchQuery({ search, ...mode }), "wordChars string");
          expectSameMatches(state, new SearchQuery({ search, regexp: true, ...mode }), "wordChars regexp");
        }
      }
    }
  });
});

// Deterministic randomness, so a failure names the seed that reproduces it.
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
  "a", "a", "b", "A", "B", "s", "S", "k", "K", "i", "I", "f", "e", "E", "x", "1", "_", " ", " ", "-", ".", "\n",
  "é", "É", "é", "́", "̣", "ﬀ", "ﬁ", "Σ", "σ", "ς", "Ο", "ο", "İ", "ı", "ß", "ẞ", "K",
  "ſ", "Ａ", "ａ", "😀", "𐐀", "𐐨", "ع", "ﻻ", "中", "豈", "²", "ǅ",
];

function randomText(rand: () => number, length: number): string {
  let out = "";
  for (let i = 0; i < length; i++) out += ALPHABET[Math.floor(rand() * ALPHABET.length)];
  return out;
}

function randomQuery(rand: () => number, doc: string): string {
  if (rand() < 0.6 && doc.length > 0) {
    const start = Math.floor(rand() * doc.length);
    const length = 1 + Math.floor(rand() * 4);
    const slice = doc.slice(start, start + length).replace(/\\/g, "");
    if (slice.trim().length > 0 && !/[\uD800-\uDFFF]/.test(slice)) return slice.replace(/\n/g, "\\n");
  }
  let query = "";
  const length = 1 + Math.floor(rand() * 3);
  for (let i = 0; i < length; i++) query += ALPHABET[Math.floor(rand() * ALPHABET.length)];
  return query.trim() ? query.replace(/\n/g, "\\n") : "a";
}

describe("find agrees with the cursor on random text", () => {
  it("for random documents and queries in every mode", () => {
    const rand = prng(20261013);
    for (let round = 0; round < 400; round++) {
      const doc = randomText(rand, 20 + Math.floor(rand() * 120));
      const state = stateOf(doc);
      const search = randomQuery(rand, doc);
      for (const mode of MODES) {
        expectSameMatches(state, new SearchQuery({ search, ...mode }), `round ${round}`);
      }
    }
  });
});

function randomChange(rand: () => number, docLength: number): ChangeSpec {
  const from = Math.floor(rand() * (docLength + 1));
  const to = Math.min(docLength, from + (rand() < 0.5 ? 0 : Math.floor(rand() * 12)));
  const insert = rand() < 0.3 ? "" : randomText(rand, 1 + Math.floor(rand() * 6));
  return { from, to, insert };
}

describe("an edit leaves the count where a fresh read would put it", () => {
  const queries = [
    new SearchQuery({ search: "a" }),
    new SearchQuery({ search: "aa" }),
    new SearchQuery({ search: "é", caseSensitive: true }),
    new SearchQuery({ search: "ff" }),
    new SearchQuery({ search: "σ" }),
    new SearchQuery({ search: "a", wholeWord: true }),
    new SearchQuery({ search: "a\\na" }),
    new SearchQuery({ search: "a+", regexp: true }),
    new SearchQuery({ search: "\\bA\\w*", regexp: true }),
    new SearchQuery({ search: "a\\s+b", regexp: true }),
    new SearchQuery({ search: "x*", regexp: true }),
  ];

  it("through random edit sequences", () => {
    const rand = prng(7);
    for (const query of queries) {
      for (let run = 0; run < 12; run++) {
        let state = stateOf(randomText(rand, 60 + Math.floor(rand() * 200)));
        const scanner = createQueryScanner(query)!;
        const index = new MatchIndex(scanner);
        index.advance(state, Infinity);
        for (let step = 0; step < 25; step++) {
          const changes = [randomChange(rand, state.doc.length)];
          if (rand() < 0.3) {
            const second = randomChange(rand, state.doc.length) as { from: number; to?: number; insert: string };
            const first = changes[0] as { from: number; to?: number };
            if (second.from > (first.to ?? first.from)) changes.push(second);
          }
          const tr = state.update({ changes });
          index.applyChanges(tr.changes, tr.startState.doc, tr.state);
          index.advance(tr.state, Infinity);
          state = tr.state;
          const expected = reference(state, query);
          expect(index.ranges(), `${query.search} run ${run} step ${step}`).toEqual(
            expected.slice(0, MATCH_COUNT_CAP),
          );
        }
      }
    }
  });

  it("past the cap, with the frontier holding the line of the last counted match", () => {
    const line = "a ".repeat(500);
    let state = stateOf(Array.from({ length: 30 }, () => line).join("\n"));
    const query = new SearchQuery({ search: "a" });
    const index = new MatchIndex(createQueryScanner(query)!);
    index.advance(state, Infinity);
    expect(index.capped).toBe(true);
    expect(index.count).toBe(MATCH_COUNT_CAP);
    for (const changes of [
      { from: 0, to: 2 },
      { from: 0, insert: "a a " },
      { from: state.doc.line(25).from, to: state.doc.line(25).to },
      { from: 0, to: state.doc.line(12).to },
    ]) {
      const tr = state.update({ changes });
      index.applyChanges(tr.changes, tr.startState.doc, tr.state);
      index.advance(tr.state, Infinity);
      state = tr.state;
      const expected = reference(state, query);
      expect(index.capped).toBe(expected.length >= MATCH_COUNT_CAP);
      expect(index.ranges()).toEqual(expected.slice(0, MATCH_COUNT_CAP));
    }
  });
});

/**
 * `MultilineRegExpCursor`'s chain (where it picks up after a match, its skip
 * of an empty match there) over the whole text as one chunk.
 */
function wholeTextMatches(text: string, search: string, caseSensitive: boolean): Range[] {
  const re = new RegExp(search, "gmu" + (caseSensitive ? "" : "i"));
  const charEnd = (pos: number) => {
    while (pos < text.length && /[\uDC00-\uDFFF]/.test(text[pos])) pos++;
    return pos;
  };
  const out: Range[] = [];
  for (let pos = charEnd(0); ; ) {
    re.lastIndex = pos;
    let match = re.exec(text);
    if (match && match[0] === "" && match.index === pos) {
      re.lastIndex = pos + 1;
      match = re.exec(text);
    }
    if (!match) return out;
    const from = match.index;
    const to = from + match[0].length;
    out.push({ from, to });
    pos = charEnd(to + (from === to ? 1 : 0));
  }
}

describe("a regular expression that crosses lines is read over the whole text", () => {
  it("past the end of the chunk CodeMirror's cursor reads, where its greedy runs stop", () => {
    // MultilineRegExpCursor reads from where it starts in a chunk ending at the
    // end of the line holding the position 5,000 characters on, and takes a
    // match that ends 10 characters before the chunk does.
    const doc = "q".repeat(4990) + " a b" + " ".repeat(20) + "\nb a b z\ntail";
    const state = stateOf(doc);
    for (const search of ["a[^z]*b", "a[\\s\\S]*b", "a\\D*b"]) {
      const query = new SearchQuery({ search, regexp: true });
      expect(wholeTextMatches(doc, search, false), search).toEqual([{ from: 4991, to: 5020 }]);
      expect(engine(state, query)?.ranges(), search).toEqual([{ from: 4991, to: 5020 }]);
      expect(reference(state, query), search).toEqual([
        { from: 4991, to: 4994 },
        { from: 5017, to: 5020 },
      ]);
    }
  });

  it("over generated documents, while every match stays within the reach", () => {
    const log = generateCorpusText({ name: "server", kind: "log", size: 0, lines: 8_000 });
    const prose = generateCorpusText({ name: "prose", kind: "prose", size: 512 * 1024 });
    const states = new Map([log, prose].map((doc) => [doc, stateOf(doc)] as const));
    const cases: [string, string, boolean, number?][] = [
      [log, "e[^z]*t", false, 8_009],
      [log, "e[^Z]*t", false],
      [log, "e[^Z]*t", true],
      [log, "a[\\s\\S]*?b", false],
      [log, "a[\\s\\S]*?b", true],
      [log, "n[^.]*\\.", false],
      [log, "s[^=]*=", true],
      [log, "e[^\\]]*\\]", false],
      [prose, "a[\\s\\S]*?b", false],
      [prose, "n[^.]*\\.", true],
    ];
    for (const [doc, search, caseSensitive, count] of cases) {
      const label = `${search} caseSensitive=${caseSensitive} over ${doc.length} characters`;
      const expected = wholeTextMatches(doc, search, caseSensitive);
      expect(expected.length, label).toBeGreaterThan(0);
      if (count !== undefined) expect(expected.length, label).toBe(count);
      expect(Math.max(...expected.map((m) => m.to - m.from)), label).toBeLessThan(DEFAULT_SCAN_LIMITS.reach);
      const index = engine(states.get(doc)!, new SearchQuery({ search, regexp: true, caseSensitive }));
      expect(index?.capped, label).toBe(expected.length >= MATCH_COUNT_CAP);
      expect(index?.ranges(), label).toEqual(expected.slice(0, MATCH_COUNT_CAP));
    }
  });
});
