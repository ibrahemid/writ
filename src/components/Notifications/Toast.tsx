import { createSignal, For, Show, type Accessor } from "solid-js";
import Button from "../Button/Button";
import "./Toast.css";

export interface ToastMessage {
  id: number;
  text: string;
  type: "info" | "error" | "warning" | "success";
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
const MAX_TOASTS = 4;

// Singleton state — Writ is single-window, single-instance per component
const [toasts, setToasts] = createSignal<ToastMessage[]>([]);
const timers = new Map<number, ReturnType<typeof setTimeout>>();
let nextId = 0;

function makeToast(id: number, text: string, type: ToastMessage["type"]): ToastMessage {
  const [repeats, setRepeats] = createSignal(1);
  return { id, text, type, repeats, bump: () => setRepeats((n) => n + 1) };
}

/** Starts this toast's life over, so a repeat gets its own full duration. */
function armDismiss(id: number, durationMs: number): void {
  const running = timers.get(id);
  if (running) clearTimeout(running);
  timers.delete(id);
  if (durationMs <= 0) return;
  timers.set(
    id,
    setTimeout(() => dismissToast(id), durationMs),
  );
}

export function showToast(text: string, type: ToastMessage["type"] = "info", durationMs = 4000) {
  const current = toasts();
  const last = current[current.length - 1];
  if (last && last.text === text && last.type === type) {
    last.bump();
    armDismiss(last.id, durationMs);
    return last.id;
  }
  const id = nextId++;
  setToasts((prev) => [...prev, makeToast(id, text, type)].slice(-MAX_TOASTS));
  armDismiss(id, durationMs);
  return id;
}

export function dismissToast(id: number) {
  const running = timers.get(id);
  if (running) clearTimeout(running);
  timers.delete(id);
  setToasts(prev => prev.filter(t => t.id !== id));
}

export default function ToastContainer() {
  return (
    <div class="toast-container" aria-live="polite" aria-atomic="false">
      <For each={toasts()}>
        {(toast) => (
          <div
            class={`toast toast-${toast.type}`}
            role={toast.type === "error" ? "alert" : "status"}
          >
            <span class="toast-text">{toast.text}</span>
            <Show when={toast.repeats() > 1}>
              <span class="toast-count">{toast.repeats()} times</span>
            </Show>
            <Button
              variant="ghost"
              icon="x"
              iconSize={14}
              class="toast-dismiss"
              aria-label="Dismiss notification"
              onClick={() => dismissToast(toast.id)}
            />
          </div>
        )}
      </For>
    </div>
  );
}
