import { createSignal, type Accessor } from "solid-js";

export type ToastType = "info" | "error" | "warning" | "success";

/** A button on the toast that does one thing, such as Undo. */
export interface ToastAction {
  label: string;
  run: () => void | Promise<void>;
}

export interface ToastRequest {
  text: string;
  type?: ToastType;
  action?: ToastAction;
  /**
   * How long a toast stays when nobody is pointing at or focused on it. An
   * error ignores this and stays until it is dismissed.
   */
  durationMs?: number;
  /**
   * Called once when the toast leaves: `true` when its action ran, `false`
   * when it was dismissed, ran out of time or was pushed out by newer toasts.
   */
  onClose?: (actionTaken: boolean) => void;
}

export interface ToastMessage {
  id: number;
  text: string;
  type: ToastType;
  action: ToastAction | null;
  /**
   * How many times this same line has been raised in a row.
   *
   * A signal rather than a number: `For` keys on the item, so replacing the
   * item to bump a count would tear the toast down and rebuild it inside the
   * live region, re-announcing the whole line when only the count moved.
   */
  repeats: Accessor<number>;
  bump: () => void;
}

/** The column is fixed and does not scroll, so it holds the newest few. */
export const MAX_TOASTS = 4;
export const TOAST_DURATION_MS = 4000;
/** A toast with an action stays longer: the person has to read it and reach it. */
export const ACTION_TOAST_DURATION_MS = 8000;

interface Clock {
  /** Time left when the clock is stopped; 0 for a toast that never expires. */
  remaining: number;
  startedAt: number;
  timer: ReturnType<typeof setTimeout> | null;
  pauses: number;
}

// Singleton state — Writ is single-window
const [toasts, setToasts] = createSignal<ToastMessage[]>([]);
const clocks = new Map<number, Clock>();
const closers = new Map<number, (actionTaken: boolean) => void>();
let nextId = 0;

function makeToast(
  id: number,
  text: string,
  type: ToastType,
  action: ToastAction | null,
): ToastMessage {
  const [repeats, setRepeats] = createSignal(1);
  return { id, text, type, action, repeats, bump: () => setRepeats((n) => n + 1) };
}

function stopClock(id: number): void {
  const clock = clocks.get(id);
  if (clock?.timer) clearTimeout(clock.timer);
  clocks.delete(id);
}

function runClock(id: number, clock: Clock): void {
  if (clock.remaining <= 0 || clock.pauses > 0) return;
  clock.startedAt = Date.now();
  clock.timer = setTimeout(() => leave(id, false), clock.remaining);
}

/** Starts this toast's life over, so a repeat gets its own full duration. */
function armClock(id: number, durationMs: number): void {
  const pauses = clocks.get(id)?.pauses ?? 0;
  stopClock(id);
  const clock: Clock = { remaining: Math.max(0, durationMs), startedAt: 0, timer: null, pauses };
  clocks.set(id, clock);
  runClock(id, clock);
}

function lifetime(request: ToastRequest, type: ToastType): number {
  if (type === "error") return 0;
  if (request.durationMs !== undefined) return request.durationMs;
  return request.action ? ACTION_TOAST_DURATION_MS : TOAST_DURATION_MS;
}

function leave(id: number, actionTaken: boolean): void {
  stopClock(id);
  setToasts((prev) => prev.filter((t) => t.id !== id));
  const closer = closers.get(id);
  closers.delete(id);
  closer?.(actionTaken);
}

/**
 * Raises a toast and returns its id.
 *
 * The short form takes the line and its type. An error stays until it is
 * dismissed; every other toast leaves after its duration, counted only while
 * nobody points at it or has focus in it.
 */
export function showToast(request: ToastRequest): number;
export function showToast(text: string, type?: ToastType, durationMs?: number): number;
export function showToast(
  requestOrText: ToastRequest | string,
  type?: ToastType,
  durationMs?: number,
): number {
  const request: ToastRequest =
    typeof requestOrText === "string"
      ? { text: requestOrText, type, durationMs }
      : requestOrText;
  const kind = request.type ?? "info";
  const current = toasts();
  const last = current[current.length - 1];
  if (
    last &&
    last.text === request.text &&
    last.type === kind &&
    last.action === null &&
    !closers.has(last.id) &&
    !request.action &&
    !request.onClose
  ) {
    last.bump();
    armClock(last.id, lifetime(request, kind));
    return last.id;
  }
  const id = nextId++;
  if (request.onClose) closers.set(id, request.onClose);
  const next = [...current, makeToast(id, request.text, kind, request.action ?? null)];
  const overflow = next.slice(0, Math.max(0, next.length - MAX_TOASTS));
  setToasts(next.slice(-MAX_TOASTS));
  armClock(id, lifetime(request, kind));
  for (const pushedOut of overflow) leave(pushedOut.id, false);
  return id;
}

/** Takes the toast away; its action, if it had one, is not run. */
export function dismissToast(id: number): void {
  if (!toasts().some((t) => t.id === id)) return;
  leave(id, false);
}

/** Runs the toast's action and takes the toast away. */
export function runToastAction(id: number): void {
  const toast = toasts().find((t) => t.id === id);
  if (!toast?.action) return;
  const { run } = toast.action;
  leave(id, true);
  void Promise.resolve()
    .then(run)
    .catch(() => undefined);
}

/** Stops the toast's clock while the pointer or focus is on it. */
export function pauseToast(id: number): void {
  const clock = clocks.get(id);
  if (!clock) return;
  clock.pauses += 1;
  if (clock.pauses > 1 || clock.timer === null) return;
  clearTimeout(clock.timer);
  clock.timer = null;
  clock.remaining = Math.max(0, clock.remaining - (Date.now() - clock.startedAt));
  // A toast whose time ran out under the pointer gets a moment once it leaves.
  if (clock.remaining === 0) clock.remaining = 1000;
}

/** Starts the clock again with the time that was left. */
export function resumeToast(id: number): void {
  const clock = clocks.get(id);
  if (!clock || clock.pauses === 0) return;
  clock.pauses -= 1;
  runClock(id, clock);
}

export { toasts };
