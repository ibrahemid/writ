import { createSignal, Show, createEffect, onCleanup } from "solid-js";
import { installFocusTrap } from "../../lib/focus-trap";
import { useWindow } from "../WindowProvider/WindowProvider";
import "./ConfirmDialog.css";

export interface ConfirmRequest {
  title: string;
  message: string;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
}

interface PendingConfirm extends ConfirmRequest {
  resolve: (confirmed: boolean) => void;
}

// Singleton state — Writ is single-window
const [pending, setPending] = createSignal<PendingConfirm | null>(null);

export function requestConfirm(request: ConfirmRequest): Promise<boolean> {
  return new Promise<boolean>((resolve) => {
    setPending((prev) => {
      if (prev) prev.resolve(false);
      return { ...request, resolve };
    });
  });
}

function settle(confirmed: boolean) {
  const current = pending();
  if (!current) return;
  current.resolve(confirmed);
  setPending(null);
}

export default function ConfirmDialog() {
  const win = useWindow();
  let dialogRef: HTMLDivElement | undefined;
  let confirmRef: HTMLButtonElement | undefined;

  createEffect(() => {
    const current = pending();
    if (!current || !dialogRef) return;
    const teardown = installFocusTrap(dialogRef, {
      onEscape: () => settle(false),
      fallbackRestore: () => {
        win.editor.focusEditor();
        return null;
      },
    });
    requestAnimationFrame(() => confirmRef?.focus());
    onCleanup(teardown);
  });

  return (
    <Show when={pending()}>
      {(req) => (
        <div class="confirm-overlay" onClick={() => settle(false)}>
          <div
            ref={dialogRef}
            class="confirm-dialog"
            onClick={(e) => e.stopPropagation()}
            role="alertdialog"
            aria-modal="true"
            aria-labelledby="confirm-title"
            aria-describedby="confirm-message"
          >
            <div id="confirm-title" class="confirm-title">
              {req().title}
            </div>
            <div id="confirm-message" class="confirm-message">
              {req().message}
            </div>
            <div class="confirm-actions">
              <button
                type="button"
                class="confirm-button confirm-cancel"
                onClick={() => settle(false)}
              >
                {req().cancelLabel ?? "Cancel"}
              </button>
              <button
                ref={confirmRef}
                type="button"
                class={`confirm-button confirm-accept ${req().danger ? "is-danger" : ""}`}
                onClick={() => settle(true)}
              >
                {req().confirmLabel ?? "Confirm"}
              </button>
            </div>
          </div>
        </div>
      )}
    </Show>
  );
}
