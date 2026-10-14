import { showToast } from "./notifications";
import { basename } from "../../lib/path";
import { logFailure } from "../../lib/log";
import type { OpenOutcome } from "./buffer-registry";

/**
 * The one place an open's answer reaches the person.
 *
 * A refusal is said with its reason. Everything else is silent here: an
 * opened file shows itself, Cancel was the person's own answer, and a file
 * still downloading has its own pane. Several refusals from one gesture (a
 * drop of twenty files) are one line, not twenty.
 */
export function reportOpenOutcomes(outcomes: readonly OpenOutcome[]): void {
  const refused = outcomes.filter(
    (outcome): outcome is Extract<OpenOutcome, { kind: "refused" }> => outcome.kind === "refused",
  );
  if (refused.length === 0) return;
  logFailure(refused.length === 1 ? "a file could not be opened" : "files could not be opened");
  if (refused.length === 1) {
    const [only] = refused;
    showToast(`Couldn't open ${basename(only.path)}: ${only.reason}`, "error");
    return;
  }
  showToast(`Couldn't open ${refused.length} files`, "error");
}

/** [`reportOpenOutcomes`] for one open. */
export function reportOpenOutcome(outcome: OpenOutcome): void {
  reportOpenOutcomes([outcome]);
}
