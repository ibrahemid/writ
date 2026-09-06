import { logFailure } from "./log";

/**
 * How long the reveal waits for the boot steps that shape the first frame.
 *
 * Long enough that a healthy launch reveals a window with its tab in it, short
 * enough that a launch which is not healthy still puts a window on screen well
 * before the Rust timer has to.
 */
export const REVEAL_DEADLINE_MS = 1000;

export interface ArmedReveal {
  /** Reveals now, unless the deadline already did. */
  now(): void;
}

/**
 * Arms the reveal of the window Writ starts hidden, before the work that
 * shapes its first frame runs.
 *
 * The window ships hidden to kill the cold-start flash, so nothing is on
 * screen until something shows it. Sequencing that show behind the first tab
 * and the saved maximized state is what leaves an app running with no window:
 * a step that rejects skips the show, and a step that never settles skips it
 * too, which no `catch` and no `finally` can answer for. Arming it first turns
 * both into a late reveal instead of no reveal, and the deadline is the only
 * thing either of them can cost.
 *
 * Reveals exactly once however the two arrive.
 */
export function armReveal(
  reveal: () => Promise<void>,
  deadlineMs: number = REVEAL_DEADLINE_MS,
): ArmedReveal {
  let revealed = false;
  const fire = () => {
    if (revealed) return;
    revealed = true;
    clearTimeout(timer);
    reveal().catch(() => logFailure("the window could not be shown"));
  };
  const timer = setTimeout(fire, deadlineMs);
  return { now: fire };
}
