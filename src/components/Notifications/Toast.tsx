import { For, Show } from "solid-js";
import Button from "../Button/Button";
import {
  dismissToast,
  pauseToast,
  resumeToast,
  runToastAction,
  toasts,
} from "../../stores/global/notifications";
import { FOCUS_TRAP_EXEMPT } from "../../lib/focus-trap";
import "./Toast.css";

export default function ToastContainer() {
  return (
    <div
      class="toast-container"
      aria-live="polite"
      aria-atomic="false"
      {...{ [FOCUS_TRAP_EXEMPT]: "" }}
    >
      <For each={toasts()}>
        {(toast) => (
          <div
            class={`toast toast-${toast.type}`}
            role={toast.type === "error" ? "alert" : "status"}
            onPointerEnter={() => pauseToast(toast.id)}
            onPointerLeave={() => resumeToast(toast.id)}
            onFocusIn={(event) => {
              const previous = event.relatedTarget;
              if (previous instanceof Node && event.currentTarget.contains(previous)) return;
              pauseToast(toast.id);
            }}
            onFocusOut={(event) => {
              const next = event.relatedTarget;
              if (next instanceof Node && event.currentTarget.contains(next)) return;
              resumeToast(toast.id);
            }}
          >
            <span class="toast-text">{toast.text}</span>
            <Show when={toast.repeats() > 1}>
              <span class="toast-count">{toast.repeats()} times</span>
            </Show>
            <Show when={toast.action}>
              {(action) => (
                <Button
                  variant="ghost"
                  class="toast-action"
                  onClick={() => runToastAction(toast.id)}
                >
                  {action().label}
                </Button>
              )}
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
