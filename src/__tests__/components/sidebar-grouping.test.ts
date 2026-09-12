import { describe, it, expect } from "vitest";
import type { BufferDocument } from "../../types/buffer";
import { bucketHistoryByTime, relativeTime } from "../../components/Sidebar/grouping";

function mk(over: Partial<BufferDocument>): BufferDocument {
  return {
    id: over.id ?? "id",
    title: over.title ?? "t",
    filename: over.filename ?? "t",
    status: over.status ?? "active",
    language: null,
    source_path: over.source_path ?? null,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: over.tab_order ?? 0,
    created_at: over.created_at ?? "2026-05-21T00:00:00.000Z",
    updated_at: over.updated_at ?? "2026-05-21T00:00:00.000Z",
    closed_at: over.closed_at ?? null,
    read_only: over.read_only ?? false,
    size_bytes: over.size_bytes ?? 0,
    line_ending: "lf",
  };
}

const DAY = 86_400_000;
const NOW = new Date("2026-05-21T12:00:00.000Z").getTime();
const daysAgo = (n: number) => new Date(NOW - n * DAY).toISOString();

describe("bucketHistoryByTime", () => {
  it("buckets across today, yesterday, last 7, last 30, older", () => {
    const items = [
      mk({ id: "today", closed_at: daysAgo(0) }),
      mk({ id: "yest", closed_at: daysAgo(1) }),
      mk({ id: "week", closed_at: daysAgo(4) }),
      mk({ id: "month", closed_at: daysAgo(20) }),
      mk({ id: "old", closed_at: daysAgo(90) }),
    ];
    const buckets = bucketHistoryByTime(items, NOW);
    expect(buckets.map((b) => b.label)).toEqual([
      "Today",
      "Yesterday",
      "Last 7 days",
      "Last 30 days",
      "Older",
    ]);
  });

  it("omits empty buckets", () => {
    const items = [mk({ id: "today", closed_at: daysAgo(0) })];
    const buckets = bucketHistoryByTime(items, NOW);
    expect(buckets.map((b) => b.label)).toEqual(["Today"]);
  });

  it("falls back to updated_at when closed_at is null", () => {
    const items = [mk({ id: "u", closed_at: null, updated_at: daysAgo(0) })];
    const buckets = bucketHistoryByTime(items, NOW);
    expect(buckets[0].label).toBe("Today");
  });

  it("orders newest first within a bucket", () => {
    const items = [
      mk({ id: "older", closed_at: daysAgo(4) }),
      mk({ id: "newer", closed_at: daysAgo(2) }),
    ];
    const buckets = bucketHistoryByTime(items, NOW);
    expect(buckets[0].items.map((i) => i.id)).toEqual(["newer", "older"]);
  });

  it("uses local-day boundaries, not a rolling 24h window", () => {
    const localMidnightToday = new Date(2026, 4, 21, 0, 0, 0, 0).getTime();
    const justAfterMidnight = localMidnightToday + 30 * 60_000;
    const lateYesterday = new Date(localMidnightToday - 30 * 60_000).toISOString();
    const buckets = bucketHistoryByTime(
      [mk({ id: "y", closed_at: lateYesterday })],
      justAfterMidnight,
    );
    expect(buckets[0].label).toBe("Yesterday");
  });
});

describe("relativeTime", () => {
  it("formats minutes, hours, and days", () => {
    expect(relativeTime(new Date(NOW - 2 * 60_000).toISOString(), NOW)).toBe("2m");
    expect(relativeTime(new Date(NOW - 3 * 3_600_000).toISOString(), NOW)).toBe("3h");
    expect(relativeTime(new Date(NOW - 12 * DAY).toISOString(), NOW)).toBe("12d");
  });

  it("shows 'now' under a minute", () => {
    expect(relativeTime(new Date(NOW - 5_000).toISOString(), NOW)).toBe("now");
  });
});
