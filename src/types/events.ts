export type WritEvent =
  | { kind: "buffer:opened"; payload: { id: string; title: string } }
  | { kind: "config:changed"; payload: { keys: string[] } }
  | { kind: "buffer:external"; payload: { bufferId: string; change: "modified" | "deleted" } }
  | { kind: "recovery:dirty"; payload: { snapshotId: string; bufferCount: number } }
  | { kind: "menu:action"; payload: { action: string } };
