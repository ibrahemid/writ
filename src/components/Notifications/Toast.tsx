import { createSignal, For, Show } from "solid-js";
import Button from "../Button/Button";
import "./Toast.css";

export interface ToastMessage {
  id: number;
  text: string;
  type: "info" | "error" | "warning" | "success";
  /** How many times this same line has been raised in a row. */
  repeats: number;
}

/** The column is fixed and does not scroll, so it holds the newest few. */
const MAX_TOASTS = 4;

// Singleton state — Writ is single-window, single-instance per component
const [toasts, setToasts] = createSignal<ToastMessage[]>([]);
let nextId = 0;

export function showToast(text: string, type: ToastMessage["type"] = "info", durationMs = 4000) {
  const id = nextId++;
  setToasts((prev) => {
    const last = prev[prev.length - 1];
    // The merged toast takes a new id, so the timer the first one scheduled
    // finds nothing and the count stays up for its own full duration.
    if (last && last.text === text && last.type === type) {
      return [...prev.slice(0, -1), { id, text, type, repeats: last.repeats + 1 }];
    }
    return [...prev, { id, text, type, repeats: 1 }].slice(-MAX_TOASTS);
  });
  if (durationMs > 0) {
    setTimeout(() => dismissToast(id), durationMs);
  }
  return id;
}

export function dismissToast(id: number) {
  setToasts(prev => prev.filter(t => t.id !== id));
}

/** Drops the whole column. For a test that raised toasts with no timer. */
export function clearToasts() {
  setToasts([]);
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
            <Show when={toast.repeats > 1}>
              <span class="toast-count">{toast.repeats} times</span>
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
