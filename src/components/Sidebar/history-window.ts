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
export function buildOffsets(
  rows: readonly HistoryRow[],
  heightFor: (row: HistoryRow) => number,
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

  let start = 0;
  while (start < n && offsets[start + 1] <= top) start++;
  let end = start;
  while (end < n && offsets[end] < bottom) end++;
  if (end <= start) end = Math.min(start + 1, n);

  return {
    start,
    end,
    padTop: offsets[start],
    padBottom: total - offsets[end],
  };
}
