import type { ChangeSet, EditorState, SelectionRange, Text } from "@codemirror/state";
import type { ChainReader, QueryScanner, ReadSink, Resume } from "./scanner";
import type { MatchState, MatchTick } from "./types";

export const MATCH_COUNT_CAP = 10_000;

/** The matches an edit left standing past itself, waiting for the reading to catch up with them. */
interface Tail {
  /** The last match before the tail's own, or -1. */
  prevFrom: number;
  prevTo: number;
  from: number[];
  to: number[];
  /** The reading may join these matches only at or after this point. */
  floor: number;
  /** Where the old reading stood, or null when it had reached the end. */
  end: Resume | null;
  stopped: { from: number; to: number } | null;
}

function lowerBound(values: number[], pos: number): number {
  let lo = 0;
  let hi = values.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (values[mid] < pos) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

/**
 * Every match of one query in one document, in order, kept current through
 * edits.
 *
 * Matches are held up to the frontier, where the reading picks up. An edit
 * reads again from a point before it where the old reading stood free, and
 * keeps reading until it stands where the old reading stood past the edit;
 * from there the old matches are moved by the edit and kept. Once the count
 * reaches the cap, the frontier stays right after the last counted match.
 */
export class MatchIndex {
  private from: number[] = [];
  private to: number[] = [];
  private frontier: Resume | null = null;
  private reader: ChainReader | null = null;
  private done = false;
  private stopped: { from: number; to: number } | null = null;
  private tail: Tail | null = null;
  // Set when a reading ends with the engine giving up; it stands through the
  // reading an edit starts until that one settles.
  private gaveUp = false;

  constructor(private readonly scanner: QueryScanner) {}

  get count(): number {
    return this.from.length;
  }

  get capped(): boolean {
    return this.from.length >= MATCH_COUNT_CAP;
  }

  get settled(): boolean {
    return this.done || this.capped;
  }

  /** The regular expression engine gave up reading this document. */
  get failed(): boolean {
    return this.gaveUp;
  }

  /** Reads on until `deadline` (a `performance.now()` time) or until settled. */
  advance(state: EditorState, deadline: number): void {
    this.read(state, deadline, Infinity);
  }

  /** Follows one transaction's changes; `state` is the state after them. */
  applyChanges(changes: ChangeSet, oldDoc: Text, state: EditorState): void {
    this.reader = null;
    const scanner = this.scanner;
    if (!scanner.resumable || this.gaveUp) {
      this.from = [];
      this.to = [];
      this.frontier = null;
      this.done = false;
      this.stopped = null;
      this.tail = null;
      return;
    }
    let firstFrom = -1;
    let lastOld = 0;
    let lastNew = 0;
    changes.iterChangedRanges((fromA, toA, _fromB, toB) => {
      if (firstFrom < 0) firstFrom = fromA;
      lastOld = toA;
      lastNew = toB;
    });
    if (firstFrom < 0) return;
    const doc = state.doc;
    const restart = scanner.restartBefore(oldDoc, firstFrom);
    const readTo = this.done ? (this.stopped ? this.stopped.to : Infinity) : (this.frontier?.pos ?? 0);
    if (restart >= readTo) {
      this.moveTail(changes, oldDoc, restart, lastNew, doc);
      return;
    }
    const delta = doc.length - oldDoc.length;
    // A chain that ended at a match its test turned down stands nowhere past that match's start.
    const { at, keep } = this.restartPoint(doc, this.stopped ? Math.min(restart, this.stopped.from) : restart);
    const floor = scanner.settleAfter(doc, lastNew);
    const frontier = this.frontier;
    const reachesPast = this.done
      ? !this.stopped || this.stopped.from + delta >= floor
      : frontier !== null && lastOld <= frontier.pos && frontier.pos + delta >= floor;
    this.tail = null;
    if (reachesPast) {
      const count = this.from.length - keep;
      const from = new Array<number>(count);
      const to = new Array<number>(count);
      for (let i = 0; i < count; i++) {
        from[i] = this.from[keep + i] + delta;
        to[i] = this.to[keep + i] + delta;
      }
      this.tail = {
        prevFrom: keep > 0 ? this.from[keep - 1] : -1,
        prevTo: keep > 0 ? this.to[keep - 1] : -1,
        from,
        to,
        floor,
        end: frontier && !this.done ? { pos: frontier.pos + delta, skipEmpty: frontier.skipEmpty } : null,
        stopped: this.stopped ? { from: this.stopped.from + delta, to: this.stopped.to + delta } : null,
      };
    }
    this.from.length = keep;
    this.to.length = keep;
    this.frontier = at;
    this.done = false;
    this.stopped = null;
    this.read(state, Infinity, at.pos + scanner.limits.editRead);
  }

  matchState(selection: SelectionRange): MatchState {
    if (this.gaveUp) {
      return { current: 0, total: 0, capped: false, pending: !this.settled, invalid: false, failed: true };
    }
    const total = Math.min(this.from.length, MATCH_COUNT_CAP);
    const at = lowerBound(this.from, selection.from);
    const current =
      at < total && this.from[at] === selection.from && this.to[at] === selection.to ? at + 1 : 0;
    return { current, total, capped: this.capped, pending: !this.settled, invalid: false, failed: false };
  }

  ticks(limit: number, length: number): MatchTick[] {
    if (this.gaveUp) return [];
    const span = length || 1;
    const count = Math.min(limit, this.from.length);
    const ticks: MatchTick[] = new Array(count);
    for (let i = 0; i < count; i++) ticks[i] = { fraction: this.from[i] / span };
    return ticks;
  }

  ranges(): { from: number; to: number }[] {
    return this.from.map((from, i) => ({ from, to: this.to[i] }));
  }

  private read(state: EditorState, deadline: number, stopAt: number): void {
    const index = this;
    const inTime = () => deadline === Infinity || performance.now() < deadline;
    while (!this.settled) {
      const reader = (this.reader ??= this.scanner.reader(state, this.frontier ?? this.scanner.start(state.doc)));
      let joined = false;
      const sink: ReadSink = {
        get watchFrom() {
          return index.tail ? index.tail.floor : Infinity;
        },
        match: (from, to) => {
          this.from.push(from);
          this.to.push(to);
          if (this.from.length < MATCH_COUNT_CAP) return true;
          this.tail = null;
          return false;
        },
        free: (at) => {
          if (this.tail && at.pos >= this.tail.floor && this.join(at, state.doc)) {
            joined = true;
            return false;
          }
          return at.pos < stopAt && inTime();
        },
      };
      reader.advance(sink);
      if (joined) {
        this.reader = null;
        if (!this.done && (this.frontier as Resume).pos < stopAt && inTime()) continue;
        return;
      }
      if (reader.ended) {
        this.reader = null;
        this.done = true;
        this.stopped = reader.stopped;
        this.tail = null;
        this.gaveUp = reader.failed;
        if (reader.failed) {
          this.from = [];
          this.to = [];
        }
      } else {
        this.frontier = reader.resume;
        if (this.capped) this.gaveUp = false;
      }
      return;
    }
  }

  /**
   * Takes the tail's matches from `at` on when the old reading stood at `at`
   * exactly as the new one does; true once it has.
   */
  private join(at: Resume, doc: Text): boolean {
    const tail = this.tail as Tail;
    if ((tail.end && at.pos > tail.end.pos) || (tail.stopped && at.pos > tail.stopped.from)) {
      this.tail = null;
      return false;
    }
    const i = lowerBound(tail.from, at.pos);
    const old =
      i > 0
        ? this.scanner.stateAt(doc, tail.from[i - 1], tail.to[i - 1], at.pos)
        : this.scanner.stateAt(doc, tail.prevFrom, tail.prevTo, at.pos);
    if (!old || old.skipEmpty !== at.skipEmpty) return false;
    for (let j = i; j < tail.from.length && this.from.length < MATCH_COUNT_CAP; j++) {
      this.from.push(tail.from[j]);
      this.to.push(tail.to[j]);
    }
    this.tail = null;
    if (this.from.length >= MATCH_COUNT_CAP) {
      const last = this.from.length - 1;
      this.frontier = this.scanner.after(doc, this.from[last], this.to[last]);
      this.done = false;
      this.stopped = null;
    } else if (tail.end) {
      this.frontier = tail.end;
      this.done = false;
      this.stopped = null;
    } else {
      this.done = true;
      this.stopped = tail.stopped;
    }
    return true;
  }

  /** A point at or before `pos` the reading so far stood at, and how many matches lie before it. */
  private restartPoint(doc: Text, pos: number): { at: Resume; keep: number } {
    const scanner = this.scanner;
    for (let p = pos; ; ) {
      const keep = lowerBound(this.from, p);
      const prevFrom = keep > 0 ? this.from[keep - 1] : -1;
      const at = scanner.stateAt(doc, prevFrom, keep > 0 ? this.to[keep - 1] : -1, p);
      if (at) return { at, keep };
      const lineStart = doc.lineAt(p).from;
      if (lineStart < p && lineStart > prevFrom) {
        p = lineStart;
        continue;
      }
      if (keep === 0) return { at: scanner.start(doc), keep: 0 };
      return { at: scanner.restartAt(doc, prevFrom), keep: keep - 1 };
    }
  }

  /** Moves a waiting tail through an edit made past everything read so far. */
  private moveTail(changes: ChangeSet, oldDoc: Text, restart: number, lastNew: number, doc: Text): void {
    const tail = this.tail;
    if (!tail) return;
    const end = tail.end ? tail.end.pos : tail.stopped ? tail.stopped.to : Infinity;
    if (restart >= end) return;
    const delta = doc.length - oldDoc.length;
    const map = (pos: number) => (pos > oldDoc.length ? pos + delta : changes.mapPos(pos, 1));
    for (let i = 0; i < tail.from.length; i++) {
      tail.from[i] = map(tail.from[i]);
      tail.to[i] = map(tail.to[i]);
    }
    tail.floor = Math.max(map(tail.floor), this.scanner.settleAfter(doc, lastNew));
    if (tail.end) tail.end = { pos: map(tail.end.pos), skipEmpty: tail.end.skipEmpty };
    if (tail.stopped) tail.stopped = { from: map(tail.stopped.from), to: map(tail.stopped.to) };
    if ((tail.end && tail.floor > tail.end.pos) || (tail.stopped && tail.floor > tail.stopped.from)) {
      this.tail = null;
    }
  }
}
