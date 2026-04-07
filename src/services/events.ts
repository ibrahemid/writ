import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type { UnlistenFn };
import type { WritEvent } from "../types/events";

type EventKind = WritEvent["kind"];
type PayloadFor<K extends EventKind> = Extract<WritEvent, { kind: K }>["payload"];
type EventHandler<K extends EventKind> = (payload: PayloadFor<K>) => void;

const EVENT_MAP: Record<EventKind, string> = {
  "config:changed": "writ://config-changed",
  "buffer:external": "writ://buffer-external",
  "recovery:dirty": "writ://recovery-dirty",
  "menu:action": "writ://menu-action",
};

export async function onEvent<K extends EventKind>(
  kind: K,
  handler: EventHandler<K>,
): Promise<UnlistenFn> {
  return listen(EVENT_MAP[kind], (event) => {
    handler(event.payload as PayloadFor<K>);
  });
}
