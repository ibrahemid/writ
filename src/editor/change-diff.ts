import { ChangeSet, Text, type ChangeSpec } from "@codemirror/state";
import { smallestChange } from "./external-reload";

/** A leaf of a document's tree and where it starts. */
interface Leaf {
  node: Text;
  from: number;
}

function leavesOf(doc: Text): Leaf[] {
  const out: Leaf[] = [];
  let pos = 0;
  const walk = (node: Text) => {
    const children = node.children;
    if (children === null) {
      out.push({ node, from: pos });
      // Leaves are joined by the line break between them.
      pos += node.length + 1;
      return;
    }
    for (const child of children) walk(child);
  };
  walk(doc);
  return out;
}

/** Leaves `[from, to)` of one document against leaves of the other. */
interface Region {
  aFrom: number;
  aTo: number;
  bFrom: number;
  bTo: number;
}

/**
 * The stretches of leaves that differ between two documents, in order.
 *
 * CodeMirror keeps the leaves an edit did not touch, so leaves both trees hold
 * are the same objects. A leaf of `b` that `a` holds further on marks the end
 * of a stretch; everything between two shared leaves is a stretch. Shared
 * leaves are matched in order on both sides, so the text between two
 * stretches is the same text in both documents.
 */
function differingRegions(a: Leaf[], b: Leaf[]): Region[] {
  let prefix = 0;
  while (prefix < a.length && prefix < b.length && a[prefix].node === b[prefix].node) prefix++;
  let suffix = 0;
  while (
    suffix < a.length - prefix &&
    suffix < b.length - prefix &&
    a[a.length - 1 - suffix].node === b[b.length - 1 - suffix].node
  ) {
    suffix++;
  }
  const aEnd = a.length - suffix;
  const bEnd = b.length - suffix;
  if (prefix === aEnd && prefix === bEnd) return [];

  const positions = new Map<Text, number[]>();
  for (let i = prefix; i < aEnd; i++) {
    const list = positions.get(a[i].node);
    if (list) list.push(i);
    else positions.set(a[i].node, [i]);
  }

  const regions: Region[] = [];
  let ai = prefix;
  let bStart = prefix;
  for (let bi = prefix; bi < bEnd; bi++) {
    const list = positions.get(b[bi].node);
    if (!list) continue;
    while (list.length > 0 && list[0] < ai) list.shift();
    if (list.length === 0) continue;
    const match = list.shift()!;
    if (match > ai || bi > bStart) {
      regions.push({ aFrom: ai, aTo: match, bFrom: bStart, bTo: bi });
    }
    ai = match + 1;
    bStart = bi + 1;
  }
  if (ai < aEnd || bStart < bEnd) {
    regions.push({ aFrom: ai, aTo: aEnd, bFrom: bStart, bTo: bEnd });
  }
  return regions;
}

/**
 * Widens a stretch that holds no leaf on one side by the shared leaf beside
 * it, so both sides are text ranges, and merges stretches the widening made
 * touch.
 */
function widenRegions(regions: Region[]): Region[] {
  const widened = regions.map((region) => {
    if (region.aFrom < region.aTo && region.bFrom < region.bTo) return region;
    if (region.aFrom > 0 && region.bFrom > 0) {
      return { ...region, aFrom: region.aFrom - 1, bFrom: region.bFrom - 1 };
    }
    return { ...region, aTo: region.aTo + 1, bTo: region.bTo + 1 };
  });
  const merged: Region[] = [];
  for (const region of widened) {
    const last = merged[merged.length - 1];
    if (last && region.aFrom <= last.aTo) {
      last.aTo = Math.max(last.aTo, region.aTo);
      last.bTo = Math.max(last.bTo, region.bTo);
    } else {
      merged.push({ ...region });
    }
  }
  return merged;
}

/**
 * The changes that turn `base` into `doc`, as one CodeMirror change set.
 *
 * Reads only the leaves the two documents do not share, and narrows each
 * stretch that differs to the characters that differ, so an edit to a large
 * document is a change set the size of the edit. Applied to `base` it gives
 * `doc` whatever the two trees look like: where they share nothing the change
 * set replaces everything.
 */
export function changesBetween(base: Text, doc: Text): ChangeSet {
  if (base === doc) return ChangeSet.empty(base.length);
  const a = leavesOf(base);
  const b = leavesOf(doc);
  const specs: ChangeSpec[] = [];
  for (const region of widenRegions(differingRegions(a, b))) {
    const aFrom = a[region.aFrom].from;
    const aLast = a[region.aTo - 1];
    const aTo = aLast.from + aLast.node.length;
    const bFrom = b[region.bFrom].from;
    const bLast = b[region.bTo - 1];
    const bTo = bLast.from + bLast.node.length;
    const before = base.sliceString(aFrom, aTo);
    const after = doc.sliceString(bFrom, bTo);
    if (before === after) continue;
    const change = smallestChange(before, after);
    specs.push({ from: aFrom + change.from, to: aFrom + change.to, insert: change.insert });
  }
  return ChangeSet.of(specs, base.length);
}

/** A save of the changes since a write that landed, as the save command takes them. */
export interface EncodedChanges {
  /** `ChangeSet.toJSON()`. */
  changes: unknown;
  /** The length of the document they produce, in UTF-16 code units. */
  length: number;
}

/**
 * Encodes the changes from the document a write landed with to the one being
 * written, or answers null when either is not a CodeMirror document.
 */
export function encodeChangesSince(base: unknown, doc: unknown): EncodedChanges | null {
  if (!(base instanceof Text) || !(doc instanceof Text)) return null;
  return { changes: changesBetween(base, doc).toJSON(), length: doc.length };
}
