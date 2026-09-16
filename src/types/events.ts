import type { UpdatePhase } from "./update";
import type {
  ChatDroppedProposal,
  ChatErrorFrame,
  ChatProposal,
  RequestIdentity,
} from "../services/tauri";

export type WritEvent =
  | { kind: "buffer:opened"; payload: { id: string; title: string } }
  | { kind: "pending:opens"; payload: { paths: string[] } }
  | { kind: "files:dropped"; payload: { paths: string[] } }
  | { kind: "window:shown"; payload: { rust_elapsed_us: number } }
  | { kind: "config:changed"; payload: { keys: string[] } }
  | {
      kind: "buffer:external";
      payload: {
        bufferId: string;
        path: string;
        change: "modified" | "removed" | "moved";
        newPath: string | null;
        diskHash: string | null;
      };
    }
  | { kind: "menu:action"; payload: { action: string } }
  | { kind: "hotkey:status"; payload: { chord: string; registered: boolean } }
  | { kind: "workspace:changed"; payload: { path: string; removed: boolean } }
  | { kind: "notes:changed"; payload: { path: string; removed: boolean } }
  | { kind: "notes:swept"; payload: { root: string } }
  | { kind: "inbox:file-arrived"; payload: { path: string } }
  | { kind: "update:status"; payload: UpdatePhase }
  | {
      kind: "ai:rewrite";
      payload: { request_id: string; kind: "chunk" | "done" | "error"; text?: string };
    }
  | {
      kind: "ai:chat";
      payload: {
        conversation_id: string;
        /** The send this frame belongs to, as `chat_send` was given it. A
         * frame naming a request the pane has moved on from is stale. */
        request_id: string;
        kind: "chunk" | "done" | "stopped" | "error";
        text?: string;
        /** Whole-note text a reply asked for, carried by the `done` frame and
         * applied by nobody until a person says so (ADR-031 rule 4.3). */
        proposals?: ChatProposal[];
        /** Which connection answered, carried by the `done` frame. */
        identity?: RequestIdentity;
        /** The typed failure, carried by the `error` frame. Its `message` is
         * the same sentence `text` holds. */
        error?: ChatErrorFrame;
        /** Blocks the reply wrote that could not become a proposal, carried by
         * the `done` frame with the path the model named and the reason. */
        dropped?: ChatDroppedProposal[];
        /** The reply stopped at the model's token ceiling. */
        truncated?: boolean;
      };
    }
  | {
      kind: "note:download";
      payload: {
        path: string;
        state: "started" | "done" | "failed" | "cancelled" | "timed_out";
        message?: string;
      };
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
    }
  | { kind: "quit:flush"; payload: Record<string, never> }
  | { kind: "activity:changed"; payload: Record<string, never> }
  | { kind: "titlebar:maximize-hit"; payload: { phase: CaptionHitPhase } };

export type CaptionHitPhase = "enter" | "leave" | "press" | "click";
