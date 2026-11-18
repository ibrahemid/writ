import { emit, listen, type UnlistenFn } from "@tauri-apps/api/event";

export type { UnlistenFn };
import type { JotraEvent } from "../types/events";

type EventKind = JotraEvent["kind"];
type PayloadFor<K extends EventKind> = Extract<JotraEvent, { kind: K }>["payload"];
type EventHandler<K extends EventKind> = (payload: PayloadFor<K>) => void;

const EVENT_MAP: Record<EventKind, string> = {
  "pending:opens": "jotra://pending-opens",
  "files:dropped": "jotra://files-dropped",
  "link:new": "jotra://link-new",
  "context:asked": "jotra://context-asked",
  "review:asked": "jotra://review-asked",
  "window:shown": "jotra://window-shown",
  "config:changed": "jotra://config-changed",
  "buffer:external": "jotra://buffer-external",
  "menu:action": "jotra://menu-action",
  "hotkey:status": "jotra://hotkey-status",
  "hotkey:capture-status": "jotra://hotkey-capture-status",
  "workspace:changed": "jotra://workspace-changed",
  "workspace:swept": "jotra://workspace-swept",
  "notes:changed": "jotra://notes-changed",
  "notes:swept": "jotra://notes-swept",
  "notes:indexed": "jotra://notes-indexed",
  "inbox:file-arrived": "jotra://inbox-file-arrived",
  "note:download": "jotra://note-download",
  "update:status": "jotra://update-status",
  "ai:rewrite": "jotra://ai-rewrite",
  "ai:chat": "jotra://ai-chat",
  "quit:flush": "jotra://flush-before-quit",
  "activity:changed": "jotra://activity",
  "mcp:pending": "jotra://mcp-pending",
  "mcp:off-call": "jotra://mcp-off-call",
  "titlebar:maximize-hit": "jotra://titlebar-maximize-hit",
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
