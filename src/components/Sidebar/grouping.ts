import type { BufferDocument } from "../../types/buffer";

export const SCRATCH_GROUP_KEY = "__scratch__";

export interface ActiveGroup {
  key: string;
  label: string;
  items: BufferDocument[];
}

export interface HistoryBucket {
  label: string;
  items: BufferDocument[];
}

function splitPath(p: string): string[] {
  return p.split(/[/\\]/).filter(Boolean);
}

function directoryKey(sourcePath: string): string {
  const segments = splitPath(sourcePath);
  segments.pop();
  return segments.join("/");
}

function directoryLabel(key: string): string {
  const segments = key.split("/").filter(Boolean);
  return segments.length > 0 ? segments[segments.length - 1] : "/";
}

export function groupActiveByDirectory(
  active: readonly BufferDocument[],
  activeTabId: string | null,
): ActiveGroup[] {
  const byKey = new Map<string, ActiveGroup>();

  for (const buffer of active) {
    const key = buffer.source_path
      ? directoryKey(buffer.source_path)
      : SCRATCH_GROUP_KEY;
    const label = key === SCRATCH_GROUP_KEY ? "Scratch" : directoryLabel(key);
    const group = byKey.get(key);
    if (group) {
      group.items.push(buffer);
    } else {
      byKey.set(key, { key, label, items: [buffer] });
    }
  }

  const groups = Array.from(byKey.values());
  for (const group of groups) {
    group.items.sort((a, b) => a.tab_order - b.tab_order);
  }

  const activeKey = activeTabId
    ? groups.find((g) => g.items.some((i) => i.id === activeTabId))?.key
    : undefined;

  return groups.sort((a, b) => {
    if (a.key === activeKey) return -1;
    if (b.key === activeKey) return 1;
    if (a.key === SCRATCH_GROUP_KEY) return 1;
    if (b.key === SCRATCH_GROUP_KEY) return -1;
    const byLabel = a.label.toLowerCase().localeCompare(b.label.toLowerCase());
    return byLabel !== 0 ? byLabel : a.key.localeCompare(b.key);
  });
}

const DAY_MS = 86_400_000;

function startOfDay(ms: number): number {
  const d = new Date(ms);
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

function historyTimestamp(buffer: BufferDocument): number {
  const source = buffer.closed_at ?? buffer.updated_at ?? buffer.created_at;
  const ts = new Date(source).getTime();
  return Number.isNaN(ts) ? 0 : ts;
}

function bucketLabel(ts: number, now: number): string {
  const days = Math.round((startOfDay(now) - startOfDay(ts)) / DAY_MS);
  if (days <= 0) return "Today";
  if (days === 1) return "Yesterday";
  if (days <= 6) return "Last 7 days";
  if (days <= 29) return "Last 30 days";
  return "Older";
}

const BUCKET_ORDER = ["Today", "Yesterday", "Last 7 days", "Last 30 days", "Older"];

export function bucketHistoryByTime(
  history: readonly BufferDocument[],
  now: number,
): HistoryBucket[] {
  const byLabel = new Map<string, BufferDocument[]>();

  for (const buffer of history) {
    const label = bucketLabel(historyTimestamp(buffer), now);
    const bucket = byLabel.get(label);
    if (bucket) {
      bucket.push(buffer);
    } else {
      byLabel.set(label, [buffer]);
    }
  }

  const buckets: HistoryBucket[] = [];
  for (const label of BUCKET_ORDER) {
    const items = byLabel.get(label);
    if (!items) continue;
    items.sort((a, b) => {
      const diff = historyTimestamp(b) - historyTimestamp(a);
      return diff !== 0 ? diff : a.id.localeCompare(b.id);
    });
    buckets.push({ label, items });
  }
  return buckets;
}

export function relativeTime(iso: string, now: number): string {
  const ts = new Date(iso).getTime();
  if (Number.isNaN(ts)) return "";
  const diff = Math.max(0, now - ts);
  if (diff < 60_000) return "now";
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m`;
  if (diff < DAY_MS) return `${Math.floor(diff / 3_600_000)}h`;
  return `${Math.floor(diff / DAY_MS)}d`;
}
