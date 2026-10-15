import { answerHeldWrite, type HeldWrite } from "../../services/tauri";
import { displayProgramName } from "../../lib/program-names";
import { logFailure } from "../../lib/log";
import {
  dismissConfirm,
  pendingConfirm,
  requestChoice,
  whenNoQuestion,
  type ConfirmRequest,
} from "./dialogs";
import { showToast } from "./notifications";

// Singleton state — Writ is single-window
const waiting: Array<{ held: HeldWrite; until: number }> = [];
let showing = false;

function nameOf(path: string): string {
  const parts = path.split("/");
  return parts[parts.length - 1] || path;
}

/** The question a held write is shown as (ADR-053). */
export function heldWriteQuestion(held: HeldWrite): ConfirmRequest {
  const program = displayProgramName(held.program);
  const name = nameOf(held.path);
  const title =
    held.kind === "create"
      ? `${program} wants to create ${name}`
      : held.kind === "rename"
        ? `${program} wants to rename ${name} to ${held.rename_to ?? name}`
        : `${program} wants to change ${name}`;
  return {
    key: held.token,
    title,
    message: held.path === name ? "" : held.path,
    hunks: held.hunks,
    confirmLabel: "Apply",
    cancelLabel: "Discard",
    defaultAction: "cancel",
  };
}

async function showNext(): Promise<void> {
  if (showing) return;
  const next = waiting.shift();
  if (!next) return;
  showing = true;
  const { held, until } = next;
  // A question the person already has on screen is theirs to answer first.
  while (pendingConfirm()) await whenNoQuestion();
  // The program stops waiting at `until`, and the question goes with it.
  const left = until - Date.now();
  let apply = false;
  if (left > 0) {
    const expiry = setTimeout(() => dismissConfirm(held.token), left);
    const outcome = await requestChoice(heldWriteQuestion(held));
    clearTimeout(expiry);
    apply = outcome === "confirm";
  }
  try {
    await answerHeldWrite(held.token, apply);
  } catch {
    logFailure("a held write could not be answered");
    if (apply) {
      showToast(
        `${displayProgramName(held.program)} stopped waiting. Nothing was written to ${nameOf(held.path)}.`,
        "warning",
      );
    }
  }
  showing = false;
  void showNext();
}

/**
 * Shows a program's held write, after any already on screen: Apply lets it
 * land, and Discard, Escape or the program giving up writes nothing.
 */
export function receiveHeldWrite(held: HeldWrite): void {
  waiting.push({ held, until: Date.now() + held.wait_ms });
  void showNext();
}
