import { createSignal } from "solid-js";

export interface ConfirmRequest {
  title: string;
  message: string;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
  /**
   * Which button takes focus when the dialog opens. Unset, a `danger` dialog
   * focuses Cancel and any other focuses the confirm button.
   */
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

/** The question on screen, if there is one. */
export function pendingConfirm(): ConfirmRequest | null {
  return pending();
}

/** Answers the question on screen. */
export function settleConfirm(outcome: ConfirmOutcome): void {
  const current = pending();
  if (!current) return;
  current.resolve(outcome);
  setPending(null);
}

/** The button that takes focus when `request` opens. */
export function initialFocus(request: ConfirmRequest): "confirm" | "cancel" {
  return request.defaultAction ?? (request.danger ? "cancel" : "confirm");
}
