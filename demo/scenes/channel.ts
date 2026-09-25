import { SCENE_NAMES, type SceneName, type ScenePlayer, type SceneState } from "./types";

export const SCENE_MESSAGE = "writ-demo-scene";
export const READY_MESSAGE = "writ-demo-ready";
export const ENGAGED_MESSAGE = "writ-demo-engaged";

export interface SceneMessage {
  data: unknown;
  origin: string;
  source: unknown;
}

export interface SceneChannelOptions {
  parent: Pick<Window, "postMessage">;
  origin: string;
}

export type SceneChannel = ReturnType<typeof createSceneChannel>;

export function isSceneName(value: unknown): value is SceneName {
  return typeof value === "string" && (SCENE_NAMES as readonly string[]).includes(value);
}

function readRequestedScene(data: unknown): SceneName | null {
  if (typeof data !== "object" || data === null) return null;
  const message = data as Record<string, unknown>;
  if (message.type !== SCENE_MESSAGE || "state" in message) return null;
  return isSceneName(message.name) ? message.name : null;
}

export function createSceneChannel(options: SceneChannelOptions) {
  const { parent, origin } = options;
  let player: ScenePlayer | null = null;
  let pending: SceneName | null = null;
  let isEngaged = false;

  function receive(message: SceneMessage): void {
    if (isEngaged || message.source !== parent || message.origin !== origin) return;
    const name = readRequestedScene(message.data);
    if (!name) return;
    if (player) void player.play(name);
    else pending = name;
  }

  function ready(next: ScenePlayer): void {
    parent.postMessage({ type: READY_MESSAGE }, origin);
    player = next;
    const name = pending;
    pending = null;
    if (name && !isEngaged) void player.play(name);
  }

  function engage(): void {
    if (isEngaged) return;
    isEngaged = true;
    pending = null;
    parent.postMessage({ type: ENGAGED_MESSAGE }, origin);
    player?.cancel();
  }

  function report(name: SceneName, state: SceneState): void {
    parent.postMessage({ type: SCENE_MESSAGE, name, state }, origin);
  }

  return { receive, ready, engage, report };
}
