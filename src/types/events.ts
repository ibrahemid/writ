import type { UpdatePhase } from "./update";

export type WritEvent =
  | { kind: "buffer:opened"; payload: { id: string; title: string } }
  | { kind: "pending:opens"; payload: { paths: string[] } }
  | { kind: "window:shown"; payload: { rust_elapsed_us: number } }
  | { kind: "config:changed"; payload: { keys: string[] } }
  | { kind: "buffer:external"; payload: { bufferId: string; change: "modified" | "deleted" } }
  | { kind: "recovery:dirty"; payload: { snapshotId: string; bufferCount: number } }
  | { kind: "menu:action"; payload: { action: string } }
  | { kind: "update:status"; payload: UpdatePhase };
