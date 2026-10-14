import type { DiffHunk, DiffLine } from "../services/tauri";

/** One diff row, with the line number each side gives it. */
export interface DiffRow {
  kind: DiffLine["kind"];
  text: string;
  before: number | null;
  after: number | null;
}

/** Numbers the lines of one hunk, counting each side only where it has one. */
export function hunkRows(hunk: DiffHunk): DiffRow[] {
  let before = hunk.before_start;
  let after = hunk.after_start;
  return hunk.lines.map((line) => {
    const row: DiffRow = {
      kind: line.kind,
      text: line.text,
      before: line.kind === "added" ? null : before,
      after: line.kind === "removed" ? null : after,
    };
    if (line.kind !== "added") before += 1;
    if (line.kind !== "removed") after += 1;
    return row;
  });
}
