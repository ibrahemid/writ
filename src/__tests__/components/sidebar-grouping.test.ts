import { describe, it, expect } from "vitest";
import type { BufferDocument } from "../../types/buffer";
import {
  groupActiveByDirectory,
  bucketHistoryByTime,
  relativeTime,
  SCRATCH_GROUP_KEY,
} from "../../components/Sidebar/grouping";

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
  };
}

const DAY = 86_400_000;
const NOW = new Date("2026-05-21T12:00:00.000Z").getTime();
const daysAgo = (n: number) => new Date(NOW - n * DAY).toISOString();

describe("groupActiveByDirectory", () => {
  it("puts the group holding the active tab first", () => {
    const a = mk({ id: "a", source_path: "/proj/alpha/one.rs", tab_order: 0 });
    const b = mk({ id: "b", source_path: "/proj/zeta/two.rs", tab_order: 1 });
    const groups = groupActiveByDirectory([a, b], "b");
    expect(groups[0].items.map((i) => i.id)).toContain("b");
  });

  it("alphabetizes other directory groups case-insensitively", () => {
    const a = mk({ id: "a", source_path: "/proj/Zeta/one.rs", tab_order: 0 });
    const b = mk({ id: "b", source_path: "/proj/alpha/two.rs", tab_order: 1 });
    const groups = groupActiveByDirectory([a, b], null);
    expect(groups.map((g) => g.label)).toEqual(["alpha", "Zeta"]);
  });

  it("places the scratch group last", () => {
    const scratch = mk({ id: "s", source_path: null, tab_order: 0 });
    const file = mk({ id: "f", source_path: "/proj/src/x.rs", tab_order: 1 });
    const groups = groupActiveByDirectory([scratch, file], null);
    expect(groups[groups.length - 1].key).toBe(SCRATCH_GROUP_KEY);
    expect(groups[groups.length - 1].label).toBe("Scratch");
  });

  it("orders items within a group by tab_order", () => {
    const a = mk({ id: "a", source_path: "/proj/src/a.rs", tab_order: 2 });
    const b = mk({ id: "b", source_path: "/proj/src/b.rs", tab_order: 0 });
    const c = mk({ id: "c", source_path: "/proj/src/c.rs", tab_order: 1 });
    const groups = groupActiveByDirectory([a, b, c], null);
    expect(groups[0].items.map((i) => i.id)).toEqual(["b", "c", "a"]);
  });

  it("derives the group label from the immediate parent directory", () => {
    const f = mk({ id: "f", source_path: "/home/me/crates/writ-storage/src/x.rs" });
    const groups = groupActiveByDirectory([f], null);
    expect(groups[0].label).toBe("src");
  });

  it("produces no groups for an empty active list", () => {
    expect(groupActiveByDirectory([], null)).toEqual([]);
  });
});

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
