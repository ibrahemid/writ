export type WritEvent =
  | { kind: "config:changed"; payload: { keys: string[] } }
  | { kind: "buffer:external"; payload: { bufferId: string; change: "modified" | "deleted" } }
  | { kind: "recovery:dirty"; payload: { snapshotId: string; bufferCount: number } };
