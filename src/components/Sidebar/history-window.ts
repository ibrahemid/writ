import type { BufferDocument } from "../../types/buffer";
import type { HistoryBucket } from "./grouping";
import { relativeTime } from "./grouping";

// A single virtualized row: either a bucket header or one history entry.
// `trailing` (the relative time) is precomputed once from a single `now`
// snapshot so rows never call `Date.now()` per render.
export type HistoryRow =
  | { kind: "header"; key: string; label: string }
  | { kind: "item"; key: string; item: BufferDocument; trailing: string };

/// Flattens time buckets into a single row list (header, items…, header, …),
/// stamping each item's relative time from one shared `now`.
export function flattenBuckets(
  buckets: readonly HistoryBucket[],
  now: number,
): HistoryRow[] {
  const rows: HistoryRow[] = [];
  for (const bucket of buckets) {
    rows.push({ kind: "header", key: `h:${bucket.label}`, label: bucket.label });
    for (const item of bucket.items) {
      rows.push({
        kind: "item",
        key: item.id,
        item,
        trailing: relativeTime(item.closed_at ?? item.updated_at, now),
      });
    }
  }
  return rows;
}

/// Prefix-sum of row heights: `offsets[i]` is the top edge of row `i`,
/// `offsets[n]` the total content height.
export function buildOffsets<Row>(
  rows: readonly Row[],
  heightFor: (row: Row) => number,
): number[] {
  const offsets = new Array<number>(rows.length + 1);
  offsets[0] = 0;
  for (let i = 0; i < rows.length; i++) {
    offsets[i + 1] = offsets[i] + heightFor(rows[i]);
  }
  return offsets;
}

export interface WindowSlice {
  /// First rendered row index (inclusive).
  start: number;
  /// One-past-last rendered row index (exclusive).
  end: number;
  /// Spacer height (px) standing in for rows above the window.
  padTop: number;
  /// Spacer height (px) standing in for rows below the window.
  padBottom: number;
}

/// Computes the visible row range for the given scroll position from a
/// prefix-sum offsets array. Renders only `[start, end)` plus `overscan` px of
/// margin on each side; everything else collapses into two spacer divs so the
/// scrollbar geometry stays exact while the DOM holds a constant handful of
/// rows.
export function sliceWindow(
  offsets: readonly number[],
  scrollTop: number,
  viewportHeight: number,
  overscan: number,
): WindowSlice {
  const n = offsets.length - 1;
  if (n <= 0) return { start: 0, end: 0, padTop: 0, padBottom: 0 };

  const total = offsets[n];
  const top = Math.max(0, scrollTop - overscan);
  const bottom = Math.min(total, scrollTop + viewportHeight + overscan);

  // Binary searches over the prefix sums, so a list of 20,000 rows costs a
  // handful of steps per scroll frame rather than a walk from the top.
  const start = firstIndex(0, n, (i) => offsets[i + 1] > top);
  let end = firstIndex(start, n, (i) => offsets[i] >= bottom);
  if (end <= start) end = Math.min(start + 1, n);

  return {
    start,
    end,
    padTop: offsets[start],
    padBottom: total - offsets[end],
  };
}

/** The first index in `[lo, hi)` where `holds` is true, or `hi`; `holds` must
 *  be false then true along the range. */
function firstIndex(lo: number, hi: number, holds: (i: number) => boolean): number {
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    if (holds(mid)) hi = mid;
    else lo = mid + 1;
  }
  return lo;
}
