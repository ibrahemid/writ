import { createSignal } from "solid-js";

import type { DiffHunk } from "../../services/tauri";

export interface ConfirmRequest {
  title: string;
  /** Shown under the title; an empty message shows nothing. */
  message: string;
  /** What the answer would change, shown line by line under the message. */
  hunks?: DiffHunk[];
  /** Names the question, so its asker can take it back ([`dismissConfirm`]). */
  key?: string;
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
const freed: Array<() => void> = [];

/**
 * Resolves once no question is on screen, for an asker that must not take
 * the place of the person's own question.
 */
export function whenNoQuestion(): Promise<void> {
  if (!pending()) return Promise.resolve();
  return new Promise((resolve) => freed.push(resolve));
}

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

/** Takes back the question `key` names, if it is the one on screen. */
export function dismissConfirm(key: string): void {
  if (pending()?.key === key) settleConfirm("dismissed");
}

/** Answers the question on screen. */
export function settleConfirm(outcome: ConfirmOutcome): void {
  const current = pending();
  if (!current) return;
  current.resolve(outcome);
  setPending(null);
  for (const resolve of freed.splice(0)) resolve();
}

/** The button that takes focus when `request` opens. */
export function initialFocus(request: ConfirmRequest): "confirm" | "cancel" {
  return request.defaultAction ?? (request.danger ? "cancel" : "confirm");
}
