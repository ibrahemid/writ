import { emit, listen, type UnlistenFn } from "@tauri-apps/api/event";

export type { UnlistenFn };
import type { WritEvent } from "../types/events";

type EventKind = WritEvent["kind"];
type PayloadFor<K extends EventKind> = Extract<WritEvent, { kind: K }>["payload"];
type EventHandler<K extends EventKind> = (payload: PayloadFor<K>) => void;

const EVENT_MAP: Record<EventKind, string> = {
  "buffer:opened": "writ://buffer-opened",
  "pending:opens": "writ://pending-opens",
  "files:dropped": "writ://files-dropped",
  "window:shown": "writ://window-shown",
  "config:changed": "writ://config-changed",
  "buffer:external": "writ://buffer-external",
  "recovery:dirty": "writ://recovery-dirty",
  "menu:action": "writ://menu-action",
  "workspace:changed": "writ://workspace-changed",
  "inbox:file-arrived": "writ://inbox-file-arrived",
  "update:status": "writ://update-status",
  "preview:rendered": "writ://preview-rendered",
  "preview:error": "writ://preview-error",
  "preview:layout_changed": "writ://preview-layout-changed",
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
