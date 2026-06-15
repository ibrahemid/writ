import { describe, it, expect } from "vitest";
import {
  flattenBuckets,
  buildOffsets,
  sliceWindow,
  type HistoryRow,
} from "../../components/Sidebar/history-window";
import type { HistoryBucket } from "../../components/Sidebar/grouping";
import type { BufferDocument } from "../../types/buffer";

let n = 0;
function buf(overrides: Partial<BufferDocument> = {}): BufferDocument {
  n++;
  return {
    id: overrides.id ?? `b-${n}`,
    title: overrides.title ?? `Buffer ${n}`,
    filename: `b-${n}.md`,
    status: "history",
    language: null,
    source_path: null,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: n,
    created_at: "2026-06-13T00:00:00Z",
    updated_at: overrides.updated_at ?? "2026-06-13T00:00:00Z",
    closed_at: overrides.closed_at ?? null,
    read_only: false,
    size_bytes: 0,
  };
}

const NOW = new Date("2026-06-13T12:00:00Z").getTime();

describe("flattenBuckets", () => {
  it("interleaves a header before each bucket's items", () => {
    const buckets: HistoryBucket[] = [
      { label: "Today", items: [buf({ id: "a" }), buf({ id: "b" })] },
      { label: "Older", items: [buf({ id: "c" })] },
    ];
    const rows = flattenBuckets(buckets, NOW);
    expect(rows.map((r) => r.kind)).toEqual(["header", "item", "item", "header", "item"]);
    expect((rows[0] as Extract<HistoryRow, { kind: "header" }>).label).toBe("Today");
    expect((rows[3] as Extract<HistoryRow, { kind: "header" }>).label).toBe("Older");
  });

  it("precomputes each item's relative time from the shared now", () => {
    const closed = new Date(NOW - 2 * 3_600_000).toISOString(); // 2h ago
    const buckets: HistoryBucket[] = [
      { label: "Today", items: [buf({ id: "a", closed_at: closed })] },
    ];
    const rows = flattenBuckets(buckets, NOW);
    const item = rows[1] as Extract<HistoryRow, { kind: "item" }>;
    expect(item.trailing).toBe("2h");
  });

  it("is empty for no buckets", () => {
    expect(flattenBuckets([], NOW)).toEqual([]);
  });
});

describe("buildOffsets + sliceWindow", () => {
  // 1 header (20px) + 100 items (30px each): total = 20 + 3000 = 3020.
  function makeRows(count: number): HistoryRow[] {
    const rows: HistoryRow[] = [{ kind: "header", key: "h", label: "Today" }];
    for (let i = 0; i < count; i++) {
      rows.push({ kind: "item", key: `i${i}`, item: buf({ id: `i${i}` }), trailing: "now" });
    }
    return rows;
  }
  const heightFor = (r: HistoryRow) => (r.kind === "header" ? 20 : 30);

  it("builds a correct prefix sum", () => {
    const rows = makeRows(2);
    expect(buildOffsets(rows, heightFor)).toEqual([0, 20, 50, 80]);
  });

  it("renders only the visible window at the top", () => {
    const rows = makeRows(100);
    const offsets = buildOffsets(rows, heightFor);
    const slice = sliceWindow(offsets, 0, 300, 0);
    expect(slice.start).toBe(0);
    // header(20) + items until >= 300px: 20 + 30*k >= 300 -> k=10 (320). end index ~ 11.
    expect(slice.padTop).toBe(0);
    expect(slice.end).toBeLessThan(rows.length);
    // Spacer accounts for everything not rendered.
    const total = offsets[offsets.length - 1];
    const rendered = offsets[slice.end] - offsets[slice.start];
    expect(slice.padTop + rendered + slice.padBottom).toBe(total);
  });

  it("windows around a deep scroll position", () => {
    const rows = makeRows(100);
    const offsets = buildOffsets(rows, heightFor);
    const slice = sliceWindow(offsets, 1500, 300, 0);
    // First rendered row's top edge must be <= scrollTop, and the previous
    // row's bottom edge <= scrollTop (it's above the viewport).
    expect(offsets[slice.start]).toBeLessThanOrEqual(1500);
    expect(offsets[slice.start + 1]).toBeGreaterThan(1500);
    const total = offsets[offsets.length - 1];
    const rendered = offsets[slice.end] - offsets[slice.start];
    expect(slice.padTop + rendered + slice.padBottom).toBe(total);
  });

  it("includes overscan margin on both sides", () => {
    const rows = makeRows(100);
    const offsets = buildOffsets(rows, heightFor);
    const tight = sliceWindow(offsets, 1500, 300, 0);
    const loose = sliceWindow(offsets, 1500, 300, 120);
    expect(loose.start).toBeLessThanOrEqual(tight.start);
    expect(loose.end).toBeGreaterThanOrEqual(tight.end);
  });

  it("returns an empty slice for no rows", () => {
    expect(sliceWindow([0], 0, 300, 0)).toEqual({ start: 0, end: 0, padTop: 0, padBottom: 0 });
  });
});
