import { createSignal, For } from "solid-js";
import "./Toast.css";

export interface ToastMessage {
  id: number;
  text: string;
  type: "info" | "error" | "warning" | "success";
}

// Singleton state — Writ is single-window, single-instance per component
const [toasts, setToasts] = createSignal<ToastMessage[]>([]);
let nextId = 0;

export function showToast(text: string, type: ToastMessage["type"] = "info", durationMs = 4000) {
  const id = nextId++;
  setToasts(prev => [...prev, { id, text, type }]);
  if (durationMs > 0) {
    setTimeout(() => dismissToast(id), durationMs);
  }
  return id;
}

export function dismissToast(id: number) {
  setToasts(prev => prev.filter(t => t.id !== id));
}

export default function ToastContainer() {
  return (
    <div class="toast-container">
      <For each={toasts()}>
        {(toast) => (
          <div class={`toast toast-${toast.type}`}>
            <span class="toast-text">{toast.text}</span>
            <button
              type="button"
              class="toast-dismiss"
              aria-label="Dismiss notification"
              onClick={() => dismissToast(toast.id)}
            >×</button>
          </div>
        )}
      </For>
    </div>
  );
}
