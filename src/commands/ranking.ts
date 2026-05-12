import type { Command } from "../types/commands";
import type { CommandUsage } from "../types/config";

export const RECENCY_HALF_LIFE_MS = 7 * 24 * 60 * 60 * 1000;
export const RECENT_SECTION_LIMIT = 5;
export const RECENT_SECTION_WINDOW_MS = 30 * 24 * 60 * 60 * 1000;
const FREQUENCY_WEIGHT = 0.25;

export interface RankingInput {
  commands: ReadonlyArray<Command>;
  query: string;
  usage: Readonly<Record<string, CommandUsage>>;
  now?: number;
}

export interface EmptyQueryPartition {
  recent: Command[];
  rest: Command[];
}

function recencyFactor(lastUsedMs: number, now: number): number {
  if (!lastUsedMs) return 0;
  const elapsed = Math.max(0, now - lastUsedMs);
  return Math.pow(0.5, elapsed / RECENCY_HALF_LIFE_MS);
}

function frequencyBoost(count: number): number {
  if (count <= 0) return 0;
  return Math.log2(1 + count);
}

export function usageBoost(entry: CommandUsage | undefined, now: number): number {
  if (!entry) return 0;
  return recencyFactor(entry.last_used_ms, now) + FREQUENCY_WEIGHT * frequencyBoost(entry.count);
}

function matchScore(cmd: Command, queryLower: string): number {
  if (!queryLower) return 0;
  const label = cmd.label.toLowerCase();
  const id = cmd.id.toLowerCase();
  const desc = cmd.description?.toLowerCase() ?? "";

  if (label === queryLower) return 5;
  if (label.startsWith(queryLower)) return 4;
  if (label.includes(queryLower)) return 3;
  if (id.includes(queryLower)) return 2;
  if (desc.includes(queryLower)) return 1;
  return -1;
}

function compareLabels(a: Command, b: Command): number {
  return a.label.localeCompare(b.label, undefined, { sensitivity: "base" });
}

export function partitionEmptyQuery(
  commands: ReadonlyArray<Command>,
  usage: Readonly<Record<string, CommandUsage>>,
  now: number = Date.now(),
): EmptyQueryPartition {
  const horizon = now - RECENT_SECTION_WINDOW_MS;
  const candidatesWithUsage = commands
    .map((cmd) => ({ cmd, entry: usage[cmd.id] }))
    .filter((row): row is { cmd: Command; entry: CommandUsage } =>
      Boolean(row.entry?.last_used_ms && row.entry.last_used_ms >= horizon),
    );

  candidatesWithUsage.sort((a, b) => {
    if (b.entry.last_used_ms !== a.entry.last_used_ms) {
      return b.entry.last_used_ms - a.entry.last_used_ms;
    }
    return compareLabels(a.cmd, b.cmd);
  });

  const recent = candidatesWithUsage.slice(0, RECENT_SECTION_LIMIT).map((r) => r.cmd);
  const recentIds = new Set(recent.map((c) => c.id));
  const rest = commands
    .filter((c) => !recentIds.has(c.id))
    .slice()
    .sort(compareLabels);

  return { recent, rest };
}

export function rankWithQuery(
  commands: ReadonlyArray<Command>,
  query: string,
  usage: Readonly<Record<string, CommandUsage>>,
  now: number = Date.now(),
): Command[] {
  const q = query.toLowerCase().trim();
  if (!q) return commands.slice().sort(compareLabels);

  const scored = commands
    .map((cmd) => ({ cmd, base: matchScore(cmd, q) }))
    .filter((row) => row.base > 0)
    .map((row) => ({
      cmd: row.cmd,
      score: row.base + usageBoost(usage[row.cmd.id], now),
    }));

  scored.sort((a, b) => {
    if (b.score !== a.score) return b.score - a.score;
    return compareLabels(a.cmd, b.cmd);
  });

  return scored.map((s) => s.cmd);
}
