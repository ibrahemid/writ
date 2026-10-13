import type { Text, TextIterator } from "@codemirror/state";

/** How much a document holds, the way a writer counts it. */
export interface TextCounts {
  /** Runs of characters between whitespace, as `text.trim().split(/\s+/)` counts them. */
  words: number;
  /** Code points, line breaks included, as `[...text].length` counts them. */
  characters: number;
}

/** Where a count stands after a slice of work. */
export interface CountProgress extends TextCounts {
  /** Whether the whole document is counted. Otherwise the totals cover `covered` positions of it. */
  done: boolean;
  /** Document positions the totals cover, line breaks included. */
  covered: number;
}

// The characters `\s` matches in a JavaScript regular expression.
const ASCII_SPACE = new Uint8Array(128);
for (const code of [0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x20]) ASCII_SPACE[code] = 1;

export function isWordSpace(code: number): boolean {
  if (code < 0x80) return ASCII_SPACE[code] === 1;
  return (
    code === 0xa0 ||
    code === 0x1680 ||
    (code >= 0x2000 && code <= 0x200a) ||
    code === 0x2028 ||
    code === 0x2029 ||
    code === 0x202f ||
    code === 0x205f ||
    code === 0x3000 ||
    code === 0xfeff
  );
}

function isHighSurrogate(code: number): boolean {
  return code >= 0xd800 && code <= 0xdbff;
}

function isLowSurrogate(code: number): boolean {
  return code >= 0xdc00 && code <= 0xdfff;
}

interface ScanState {
  inWord: boolean;
  words: number;
  characters: number;
}

function scanSpan(line: string, from: number, to: number, state: ScanState): void {
  let { inWord, words, characters } = state;
  for (let i = from; i < to; i++) {
    const code = line.charCodeAt(i);
    if (isLowSurrogate(code) && i > 0 && isHighSurrogate(line.charCodeAt(i - 1))) continue;
    characters++;
    if (isWordSpace(code)) inWord = false;
    else if (!inWord) {
      inWord = true;
      words++;
    }
  }
  state.inWord = inWord;
  state.words = words;
  state.characters = characters;
}

// Counts of finished subtrees. A document shares every subtree an edit did
// not touch with the document before it, so a recount after a keystroke
// reads only the path down to the changed lines.
const subtreeCounts = new WeakMap<Text, TextCounts>();

// Leaves hold at most 32 lines and are cheap to read again, so only a leaf
// carrying long lines is worth an entry.
const CACHED_LEAF_LENGTH = 32_768;

// Characters a leaf scan reads between looks at the clock.
const CHARACTERS_PER_CHECK = 16_384;

interface Frame {
  node: Text;
  children: readonly Text[];
  index: number;
  words: number;
  characters: number;
  covered: number;
}

interface LeafScan extends ScanState {
  leaf: Text;
  cursor: TextIterator;
  line: string | null;
  offset: number;
  covered: number;
}

function frameFor(node: Text, children: readonly Text[]): Frame {
  return { node, children, index: 0, words: 0, characters: 0, covered: 0 };
}

function startLeafScan(scan: LeafScan | null, leaf: Text): LeafScan {
  if (scan === null) {
    return { leaf, cursor: leaf.iter(), line: null, offset: 0, inWord: false, words: 0, characters: 0, covered: 0 };
  }
  scan.leaf = leaf;
  scan.cursor = leaf.iter();
  scan.line = null;
  scan.offset = 0;
  scan.inWord = false;
  scan.words = 0;
  scan.characters = 0;
  scan.covered = 0;
  return scan;
}

/**
 * Counts a document a slice at a time.
 *
 * `count` walks the document's tree until the clock it is handed says stop,
 * and picks the walk up where it stopped on the next call, for the same
 * document or a later version of it. Finished subtrees are kept in a cache
 * keyed by the subtree, so nothing an edit left alone is read twice, and
 * nothing is allocated in proportion to the document's size.
 */
export class TextCounter {
  private doc: Text | null = null;
  private frames: Frame[] = [];
  private leaf: LeafScan | null = null;
  // A leaf scan a newer version of the document interrupted, resumed if the
  // walk of that version reaches the same leaf.
  private paused: LeafScan | null = null;
  // A finished leaf scan, reused for the next leaf.
  private spare: LeafScan | null = null;
  private result: TextCounts | null = null;

  count(doc: Text, outOfTime: () => boolean): CountProgress {
    if (doc !== this.doc) this.start(doc);
    let progressed = false;
    for (;;) {
      if (this.result) return { ...this.result, done: true, covered: doc.length };
      if (this.leaf) {
        if (!this.scanLeaf(outOfTime)) return this.progress();
        progressed = true;
        const scan = this.leaf;
        this.leaf = null;
        this.spare = scan;
        if (scan.leaf.length >= CACHED_LEAF_LENGTH) {
          subtreeCounts.set(scan.leaf, { words: scan.words, characters: scan.characters });
        }
        if (this.frames.length === 0) this.result = { words: scan.words, characters: scan.characters };
        else this.finishChild(scan.words, scan.characters, scan.leaf.length);
        continue;
      }
      const top = this.frames[this.frames.length - 1];
      if (top.index === top.children.length) {
        const counts = { words: top.words, characters: top.characters };
        subtreeCounts.set(top.node, counts);
        this.frames.pop();
        if (this.frames.length === 0) this.result = counts;
        else this.finishChild(counts.words, counts.characters, top.node.length);
        continue;
      }
      if (progressed && outOfTime()) return this.progress();
      const child = top.children[top.index];
      const known = subtreeCounts.get(child);
      if (known) {
        this.finishChild(known.words, known.characters, child.length);
        progressed = true;
        continue;
      }
      if (child.children) this.frames.push(frameFor(child, child.children));
      else this.leaf = this.resume(child);
    }
  }

  private start(doc: Text): void {
    if (this.leaf) this.paused = this.leaf;
    this.doc = doc;
    this.frames = [];
    this.leaf = null;
    this.result = subtreeCounts.get(doc) ?? null;
    if (this.result) return;
    if (doc.children) this.frames.push(frameFor(doc, doc.children));
    else this.leaf = this.resume(doc);
  }

  private resume(leaf: Text): LeafScan {
    const paused = this.paused;
    if (paused !== null && paused.leaf === leaf) {
      this.paused = null;
      return paused;
    }
    const spare = this.spare;
    this.spare = null;
    return startLeafScan(spare, leaf);
  }

  private finishChild(words: number, characters: number, length: number): void {
    const top = this.frames[this.frames.length - 1];
    const lineBreak = top.index > 0 ? 1 : 0;
    top.words += words;
    top.characters += characters + lineBreak;
    top.covered += length + lineBreak;
    top.index++;
  }

  private scanLeaf(outOfTime: () => boolean): boolean {
    const scan = this.leaf!;
    let budget = CHARACTERS_PER_CHECK;
    for (;;) {
      if (scan.line === null) {
        if (scan.cursor.next().done) return true;
        if (scan.cursor.lineBreak) {
          scan.characters++;
          scan.covered++;
          scan.inWord = false;
          budget--;
        } else {
          scan.line = scan.cursor.value;
          scan.offset = 0;
        }
      }
      if (scan.line !== null) {
        const line = scan.line;
        const end = Math.min(line.length, scan.offset + Math.max(budget, 1));
        scanSpan(line, scan.offset, end, scan);
        scan.covered += end - scan.offset;
        budget -= Math.max(end - scan.offset, 1);
        scan.offset = end;
        if (end === line.length) scan.line = null;
      }
      if (budget <= 0) {
        if (outOfTime()) return false;
        budget = CHARACTERS_PER_CHECK;
      }
    }
  }

  private progress(): CountProgress {
    let words = 0;
    let characters = 0;
    let covered = 0;
    for (const frame of this.frames) {
      words += frame.words;
      characters += frame.characters;
      covered += frame.covered;
    }
    if (this.leaf) {
      words += this.leaf.words;
      characters += this.leaf.characters;
      covered += this.leaf.covered;
    }
    return { words, characters, covered, done: false };
  }
}

/** Counts the whole of `doc` now. */
export function countText(doc: Text): TextCounts {
  const { words, characters } = new TextCounter().count(doc, () => false);
  return { words, characters };
}

/**
 * The counts of the whole document, projected from the part counted so far,
 * for a count that has not finished.
 */
export function projectCounts(progress: CountProgress, length: number): TextCounts {
  if (progress.done || progress.covered <= 0) return { words: progress.words, characters: progress.characters };
  const scale = length / progress.covered;
  return {
    words: Math.round(progress.words * scale),
    characters: Math.round(progress.characters * scale),
  };
}

/**
 * The bytes `doc` takes as UTF-8, a line break as one, read from the
 * document's own strings. Stops counting once the total is past `stopAbove`
 * and answers with what it has, which is enough to say it is past it.
 */
export function utf8Length(doc: Text, stopAbove = Infinity): number {
  if (doc.length > stopAbove) return doc.length;
  let bytes = 0;
  for (const cursor = doc.iter(); !cursor.next().done; ) {
    if (cursor.lineBreak) {
      bytes += 1;
      continue;
    }
    const chunk = cursor.value;
    for (let i = 0; i < chunk.length; i++) {
      const code = chunk.charCodeAt(i);
      if (code < 0x80) bytes += 1;
      else if (code < 0x800) bytes += 2;
      else if (isHighSurrogate(code) && i + 1 < chunk.length && isLowSurrogate(chunk.charCodeAt(i + 1))) {
        bytes += 4;
        i++;
      } else bytes += 3;
    }
    if (bytes > stopAbove) return bytes;
  }
  return bytes;
}
