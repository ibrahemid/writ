import { describe, it, expect } from "vitest";
import { Text } from "@codemirror/state";
import {
  TextCounter,
  countText,
  isWordSpace,
  projectCounts,
  utf8Length,
  type CountProgress,
} from "../../editor/document-counts";

function doc(text: string): Text {
  return Text.of(text.split("\n"));
}

// What the status bar counted before it counted the document's own strings.
function wordsByRegExp(text: string): number {
  const trimmed = text.trim();
  return trimmed === "" ? 0 : trimmed.split(/\s+/).length;
}

function charactersBySpread(text: string): number {
  return [...text].length;
}

const CJK = /[\u{1100}-\u{11FF}\u{3040}-\u{30FF}\u{3130}-\u{318F}\u{3400}-\u{4DBF}\u{4E00}-\u{9FFF}\u{AC00}-\u{D7AF}\u{F900}-\u{FAFF}\u{20000}-\u{2FFFF}]/u;
const ARABIC = /[\u{0600}-\u{06FF}\u{0750}-\u{077F}\u{08A0}-\u{08FF}\u{FB50}-\u{FDFF}\u{FE70}-\u{FEFC}]/u;

function bySpread(text: string) {
  const points = [...text];
  return {
    words: wordsByRegExp(text),
    characters: points.length,
    cjk: points.filter((point) => CJK.test(point)).length,
    arabic: points.filter((point) => ARABIC.test(point)).length,
  };
}

function seeded(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const PIECES = [
  "word", "Writ", "ب", "مرحبا", "مُلاحَظةٌ", "日本語", "한국어", "𠀀", "😀", "👩‍💻", "é", " ", "　", " ", "﻿",
  " ", "  ", "\t", "\n", "\n\n", "-", "x".repeat(40), "\ud83d", "\ude00",
];

function generated(seed: number, pieces: number): string {
  const next = seeded(seed);
  let out = "";
  for (let i = 0; i < pieces; i++) out += PIECES[Math.floor(next() * PIECES.length)];
  return out;
}

/** Counts in slices that each stop at the first look at the clock. */
function countInSlices(counter: TextCounter, text: Text): { progress: CountProgress; slices: number } {
  let slices = 0;
  for (;;) {
    slices++;
    const progress = counter.count(text, () => true);
    if (progress.done) return { progress, slices };
    if (slices > 1_000_000) throw new Error("the count stopped moving");
  }
}

describe("isWordSpace", () => {
  it("is true for exactly the characters \\s matches", () => {
    const differences: number[] = [];
    for (let code = 0; code <= 0xffff; code++) {
      if (isWordSpace(code) !== /\s/.test(String.fromCharCode(code))) differences.push(code);
    }
    expect(differences).toEqual([]);
  });
});

describe("countText", () => {
  it("counts runs between whitespace as words", () => {
    expect(countText(doc("one two three")).words).toBe(3);
    expect(countText(doc("  one   two \n\n three  ")).words).toBe(3);
    expect(countText(doc("one\ntwo\nthree")).words).toBe(3);
  });

  it("is zero for an empty or blank document", () => {
    expect(countText(doc(""))).toEqual({ words: 0, characters: 0, cjk: 0, arabic: 0 });
    expect(countText(doc("   \n\t  ")).words).toBe(0);
  });

  it("counts a character outside the Basic Multilingual Plane once", () => {
    expect(countText(doc("a😀b"))).toEqual({ words: 1, characters: 3, cjk: 0, arabic: 0 });
  });

  it("counts a surrogate with no partner as one character", () => {
    expect(countText(doc("\ud83d"))).toEqual({ words: 1, characters: 1, cjk: 0, arabic: 0 });
    expect(countText(doc("\ud83d\n\ude00"))).toEqual({ words: 2, characters: 3, cjk: 0, arabic: 0 });
  });

  it("gives the numbers the regular expression and the spread gave, on mixed text", () => {
    for (let seed = 1; seed <= 40; seed++) {
      const text = generated(seed, 200 + seed * 150);
      expect(countText(doc(text)), `seed ${seed}`).toEqual(bySpread(text));
    }
  });

  it("gives the same numbers across the joins of a deep document", () => {
    const text = generated(7, 60_000);
    const tree = doc(text);
    expect(tree.children).not.toBeNull();
    expect(countText(tree)).toEqual(bySpread(text));
  });
});

describe("TextCounter", () => {
  it("picks a count up where the last slice stopped, inside a long line", () => {
    const line = `${"a".repeat(16_383)}😀${"b c ".repeat(20_000)}`;
    const text = doc(`${line}\n${line}`);
    const { progress, slices } = countInSlices(new TextCounter(), text);
    expect(slices).toBeGreaterThan(4);
    expect(progress).toMatchObject({
      words: wordsByRegExp(`${line}\n${line}`),
      characters: charactersBySpread(`${line}\n${line}`),
      covered: text.length,
    });
  });

  it("does not split a word or a surrogate pair at a slice boundary", () => {
    const text = `${"w".repeat(16_383)}😀${"w".repeat(40_000)}`;
    const { progress } = countInSlices(new TextCounter(), doc(text));
    expect(progress.words).toBe(1);
    expect(progress.characters).toBe(16_383 + 1 + 40_000);
  });

  it("finishes a newer version of the document after a slice of the older one", () => {
    const counter = new TextCounter();
    const before = doc(generated(3, 40_000));
    counter.count(before, () => true);
    const after = before.replace(before.length - 10, before.length, Text.of(["new words at the end"]));
    const { progress } = countInSlices(counter, after);
    const text = after.toString();
    expect(progress).toMatchObject({ words: wordsByRegExp(text), characters: charactersBySpread(text) });
  });

  it("reads only the part of the document an edit touched on the recount", () => {
    const lines = Array.from({ length: 100_000 }, (_, i) => `line ${i} of the document`);
    const before = doc(lines.join("\n"));
    const counter = new TextCounter();
    counter.count(before, () => false);

    const at = before.line(50_000).from;
    const after = before.replace(at, at, Text.of(["more "]));
    const looks: number[] = [];
    let reads = 0;
    const progress = counter.count(after, () => {
      looks.push(reads++);
      return false;
    });

    expect(progress).toMatchObject({ done: true, words: 100_000 * 5 + 1 });
    expect(looks.length).toBeLessThan(200);
  });

  it("reports what it has covered while a count is unfinished", () => {
    const text = doc(generated(11, 50_000));
    const progress = new TextCounter().count(text, () => true);
    expect(progress.done).toBe(false);
    expect(progress.covered).toBeGreaterThan(0);
    expect(progress.covered).toBeLessThan(text.length);
  });
});

describe("projectCounts", () => {
  it("scales the counted part to the whole document", () => {
    const progress: CountProgress = { words: 100, characters: 600, cjk: 20, arabic: 4, covered: 500, done: false };
    expect(projectCounts(progress, 5_000)).toEqual({ words: 1_000, characters: 6_000, cjk: 200, arabic: 40 });
  });

  it("answers a finished count as it is", () => {
    const progress: CountProgress = { words: 3, characters: 13, cjk: 0, arabic: 0, covered: 13, done: true };
    expect(projectCounts(progress, 13)).toEqual({ words: 3, characters: 13, cjk: 0, arabic: 0 });
  });
});

describe("utf8Length", () => {
  it("gives the length TextEncoder gives", () => {
    const encoder = new TextEncoder();
    for (let seed = 1; seed <= 30; seed++) {
      const text = generated(seed, 100 + seed * 50);
      expect(utf8Length(doc(text)), `seed ${seed}`).toBe(encoder.encode(text).length);
    }
  });

  it("counts a character outside ASCII as the bytes UTF-8 gives it", () => {
    expect(utf8Length(doc("é"))).toBe(2);
    expect(utf8Length(doc("日"))).toBe(3);
    expect(utf8Length(doc("😀"))).toBe(4);
    expect(utf8Length(doc("\ud83d"))).toBe(3);
    expect(utf8Length(doc("a\nb"))).toBe(3);
  });

  it("stops once the total is past the limit it is given", () => {
    const text = doc(Array.from({ length: 100 }, () => "é".repeat(9)).join("\n"));
    expect(utf8Length(text)).toBe(1_899);
    const measured = utf8Length(text, 1_000);
    expect(measured).toBeGreaterThan(1_000);
    expect(measured).toBeLessThan(1_899);
  });

  it("answers from the length alone for a document longer than the limit", () => {
    expect(utf8Length(doc("é".repeat(2_000)), 1_000)).toBe(2_000);
  });
});
