import type { UpdatePhase } from "./update";

export type WritEvent =
  | { kind: "buffer:opened"; payload: { id: string; title: string } }
  | { kind: "pending:opens"; payload: { paths: string[] } }
  | { kind: "files:dropped"; payload: { paths: string[] } }
  | { kind: "window:shown"; payload: { rust_elapsed_us: number } }
  | { kind: "config:changed"; payload: { keys: string[] } }
  | { kind: "buffer:external"; payload: { bufferId: string; change: "modified" | "deleted" } }
  | { kind: "menu:action"; payload: { action: string } }
  | { kind: "workspace:changed"; payload: { path: string; removed: boolean } }
  | { kind: "inbox:file-arrived"; payload: { path: string } }
  | { kind: "update:status"; payload: UpdatePhase }
  | {
      kind: "ai:rewrite";
      payload: { request_id: string; kind: "chunk" | "done" | "error"; text?: string };
    }
  | {
      kind: "preview:rendered";
      payload: {
        buffer_id: string;
        window_id: number;
        used_fallback_stylesheet: boolean;
        parser_warnings: string[];
      };
    }
  | { kind: "preview:error"; payload: { buffer_id: string; window_id: number; message: string } }
  | {
      kind: "preview:layout_changed";
      payload: { buffer_id: string; window_id: number; layout: string; ratio: number | null };
    };
