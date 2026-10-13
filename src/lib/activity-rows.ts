import type { ActivityRecord } from "../services/tauri";
import { isReadAction } from "./tool-phrases";

export const FOLD_WINDOW_MS = 60_000;

export interface ActivityRow {
  record: ActivityRecord;
  calls: number;
}

function timeOf(record: ActivityRecord): number {
  const at = Date.parse(record.at);
  return Number.isNaN(at) ? 0 : at;
}

function actorKey(record: ActivityRecord): string {
  switch (record.actor.kind) {
    case "client":
      return `client:${record.actor.name}`;
    case "chat":
      return `chat:${record.actor.host}`;
    case "app":
      return "app";
  }
}

function sameRead(a: ActivityRecord, b: ActivityRecord): boolean {
  return (
    isReadAction(a.action) &&
    a.action === b.action &&
    a.path === b.path &&
    a.decision === b.decision &&
    actorKey(a) === actorKey(b)
  );
}

export function foldActivity(records: readonly ActivityRecord[]): ActivityRow[] {
  // A run's count line is appended when the run closes, after lines that came
  // later, so rows are put in time order before they fold.
  const ordered = records
    .map((record, index) => ({ record, index }))
    .sort((a, b) => timeOf(b.record) - timeOf(a.record) || a.index - b.index);
  const rows: ActivityRow[] = [];
  for (const { record } of ordered) {
    const calls = record.count ?? 1;
    const last = rows[rows.length - 1];
    if (
      last &&
      sameRead(last.record, record) &&
      timeOf(last.record) - timeOf(record) < FOLD_WINDOW_MS
    ) {
      last.calls += calls;
      continue;
    }
    rows.push({ record, calls });
  }
  return rows;
}
