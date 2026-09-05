import { createSignal, Show, createEffect, onCleanup } from "solid-js";
import { installFocusTrap } from "../../lib/focus-trap";
import Button from "../Button/Button";
import { useWindow } from "../WindowProvider/WindowProvider";
import "./ConfirmDialog.css";

export interface ConfirmRequest {
  title: string;
  message: string;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
  // Which button takes focus when the dialog opens. Cancel for a choice whose
  // confirm side destroys something the user cannot get back.
  defaultAction?: "confirm" | "cancel";
}

/**
 * Which of the three ways out the person took.
 *
 * `cancel` is the labelled button; `dismissed` is Escape, a click outside, or
 * another dialog taking the screen. They are the same answer only where the
 * cancel side does nothing — a caller whose cancel side still changes
 * something has to read `dismissed` as "stop", never as a choice.
 */
export type ConfirmOutcome = "confirm" | "cancel" | "dismissed";

interface PendingConfirm extends ConfirmRequest {
  resolve: (outcome: ConfirmOutcome) => void;
}

// Singleton state — Writ is single-window
const [pending, setPending] = createSignal<PendingConfirm | null>(null);

/** The dialog's full answer. See [`ConfirmOutcome`]. */
export function requestChoice(request: ConfirmRequest): Promise<ConfirmOutcome> {
  return new Promise<ConfirmOutcome>((resolve) => {
    setPending((prev) => {
      if (prev) prev.resolve("dismissed");
      return { ...request, resolve };
    });
  });
}

export function requestConfirm(request: ConfirmRequest): Promise<boolean> {
  return requestChoice(request).then((outcome) => outcome === "confirm");
}

function settle(outcome: ConfirmOutcome) {
  const current = pending();
  if (!current) return;
  current.resolve(outcome);
  setPending(null);
}

export default function ConfirmDialog() {
  const win = useWindow();
  let dialogRef: HTMLDivElement | undefined;
  let confirmRef: HTMLButtonElement | undefined;
  let cancelRef: HTMLButtonElement | undefined;

  createEffect(() => {
    const current = pending();
    if (!current || !dialogRef) return;
    const teardown = installFocusTrap(dialogRef, {
      onEscape: () => settle("dismissed"),
      fallbackRestore: () => {
        win.editor.focusEditor();
        return null;
      },
    });
    const focusTarget = () =>
      current.defaultAction === "cancel" ? cancelRef : confirmRef;
    requestAnimationFrame(() => focusTarget()?.focus());
    onCleanup(teardown);
  });

  return (
    <Show when={pending()}>
      {(req) => (
        <div class="confirm-overlay" onClick={() => settle("dismissed")}>
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
              <Button
                ref={cancelRef}
                variant="secondary"
                class="confirm-cancel"
                onClick={() => settle("cancel")}
              >
                {req().cancelLabel ?? "Cancel"}
              </Button>
              <Button
                ref={confirmRef}
                variant="primary"
                danger={req().danger}
                class="confirm-accept"
                onClick={() => settle("confirm")}
              >
                {req().confirmLabel ?? "Confirm"}
              </Button>
            </div>
          </div>
        </div>
      )}
    </Show>
  );
}
