import { SceneCancelledError } from "./errors";
import type { Scene, SceneApp, SceneEditor, SceneName, ScenePalette, ScenePlayer, SceneState } from "./types";

export const KEY_DELAY_MIN_MS = 35;
export const KEY_DELAY_MAX_MS = 70;
export const LINE_END_DELAY_MS = 250;
// CodeMirror's completion keeps its 100 ms typing timer armed when a tab switch drops the extension, and it throws when it fires.
export const TYPING_COOLDOWN_MS = 250;
const POLL_MS = 16;

const lastKeystrokeAt = new WeakMap<AbortSignal, number>();

export type Random = () => number;

export function throwIfCancelled(signal: AbortSignal): void {
  if (signal.aborted) throw new SceneCancelledError();
}

export function sleep(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal.aborted) {
      reject(new SceneCancelledError());
      return;
    }
    const onAbort = () => {
      clearTimeout(timer);
      reject(new SceneCancelledError());
    };
    const timer = setTimeout(() => {
      signal.removeEventListener("abort", onAbort);
      resolve();
    }, ms);
    signal.addEventListener("abort", onAbort, { once: true });
  });
}

export async function checked<T>(pending: Promise<T>, signal: AbortSignal): Promise<T> {
  const value = await pending;
  throwIfCancelled(signal);
  return value;
}

export async function waitUntil(isMet: () => boolean, timeoutMs: number, signal: AbortSignal): Promise<boolean> {
  throwIfCancelled(signal);
  const deadline = Date.now() + timeoutMs;
  while (!isMet()) {
    if (Date.now() >= deadline) return false;
    await sleep(POLL_MS, signal);
  }
  return true;
}

export function computeKeyDelay(char: string, random: Random): number {
  if (char === "\n") return LINE_END_DELAY_MS;
  return KEY_DELAY_MIN_MS + Math.round(random() * (KEY_DELAY_MAX_MS - KEY_DELAY_MIN_MS));
}

export async function typeText(editor: SceneEditor, text: string, signal: AbortSignal, random: Random): Promise<void> {
  editor.setTyping(true);
  try {
    for (const char of text) {
      await sleep(computeKeyDelay(char, random), signal);
      editor.insert(char);
      lastKeystrokeAt.set(signal, Date.now());
    }
  } finally {
    editor.setTyping(false);
  }
}

export async function typeQuery(palette: ScenePalette, query: string, signal: AbortSignal, random: Random): Promise<void> {
  let typed = "";
  for (const char of query) {
    await sleep(computeKeyDelay(char, random), signal);
    typed += char;
    palette.setQuery(typed);
  }
}

export interface SceneRunnerOptions {
  app: SceneApp;
  scenes: Readonly<Record<SceneName, Scene>>;
  settle: Scene;
  report: (name: SceneName, state: SceneState) => void;
  reportFailure?: (name: SceneName, error: unknown) => void;
}

interface SceneRun {
  controller: AbortController;
  finished: Promise<void>;
}

function logSceneFailure(name: SceneName, error: unknown): void {
  console.error(`[writ-demo] scene "${name}" stopped before its end`, error);
}

export function createSceneRunner(options: SceneRunnerOptions): ScenePlayer {
  const { app, scenes, settle, report } = options;
  const reportFailure = options.reportFailure ?? logSceneFailure;
  let latest: SceneRun | null = null;
  let quietUntil = 0;

  function holdAfterRecentKeystroke(signal: AbortSignal): void {
    const keystrokeAt = lastKeystrokeAt.get(signal);
    const now = Date.now();
    if (keystrokeAt === undefined || now - keystrokeAt >= TYPING_COOLDOWN_MS) return;
    quietUntil = Math.max(quietUntil, now + TYPING_COOLDOWN_MS);
  }

  async function perform(name: SceneName, previous: SceneRun | null, signal: AbortSignal): Promise<void> {
    if (previous) await previous.finished;
    try {
      throwIfCancelled(signal);
      const quietForMs = quietUntil - Date.now();
      if (quietForMs > 0) await sleep(quietForMs, signal);
      await settle(app, signal);
      throwIfCancelled(signal);
      await scenes[name](app, signal);
      throwIfCancelled(signal);
      report(name, "done");
    } catch (error) {
      holdAfterRecentKeystroke(signal);
      if (!(error instanceof SceneCancelledError) && !signal.aborted) reportFailure(name, error);
      report(name, "cancelled");
    }
  }

  function play(name: SceneName): Promise<void> {
    const previous = latest;
    previous?.controller.abort();
    const controller = new AbortController();
    const run: SceneRun = { controller, finished: perform(name, previous, controller.signal) };
    latest = run;
    return run.finished;
  }

  function cancel(): void {
    latest?.controller.abort();
  }

  return { play, cancel };
}
