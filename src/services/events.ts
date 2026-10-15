import { emit, listen, type UnlistenFn } from "@tauri-apps/api/event";

export type { UnlistenFn };
import type { WritEvent } from "../types/events";

type EventKind = WritEvent["kind"];
type PayloadFor<K extends EventKind> = Extract<WritEvent, { kind: K }>["payload"];
type EventHandler<K extends EventKind> = (payload: PayloadFor<K>) => void;

const EVENT_MAP: Record<EventKind, string> = {
  "pending:opens": "writ://pending-opens",
  "files:dropped": "writ://files-dropped",
  "link:new": "writ://link-new",
  "context:asked": "writ://context-asked",
  "review:asked": "writ://review-asked",
  "window:shown": "writ://window-shown",
  "config:changed": "writ://config-changed",
  "buffer:external": "writ://buffer-external",
  "menu:action": "writ://menu-action",
  "hotkey:status": "writ://hotkey-status",
  "hotkey:capture-status": "writ://hotkey-capture-status",
  "workspace:changed": "writ://workspace-changed",
  "workspace:swept": "writ://workspace-swept",
  "notes:changed": "writ://notes-changed",
  "notes:swept": "writ://notes-swept",
  "notes:indexed": "writ://notes-indexed",
  "inbox:file-arrived": "writ://inbox-file-arrived",
  "note:download": "writ://note-download",
  "update:status": "writ://update-status",
  "ai:rewrite": "writ://ai-rewrite",
  "ai:chat": "writ://ai-chat",
  "quit:flush": "writ://flush-before-quit",
  "activity:changed": "writ://activity",
  "mcp:pending": "writ://mcp-pending",
  "mcp:off-call": "writ://mcp-off-call",
  "titlebar:maximize-hit": "writ://titlebar-maximize-hit",
};

export async function onEvent<K extends EventKind>(
  kind: K,
  handler: EventHandler<K>,
): Promise<UnlistenFn> {
  return listen(EVENT_MAP[kind], (event) => {
    const envelope = event.payload as unknown as { payload?: unknown } | null;
    handler(envelope?.payload as PayloadFor<K>);
  });
}

export async function emitFrontendReady(): Promise<void> {
  await emit("frontend-ready");
}
