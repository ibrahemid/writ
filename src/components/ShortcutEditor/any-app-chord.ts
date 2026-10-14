import type { RecorderOutcome } from "./recorder";
import { sameChord, worksFromAnyApp } from "../../lib/chord";
import type { Platform } from "../../lib/platform";
import { configStore } from "../../stores/global/config";
import { captureHotkeyStore, SHIPPED_CAPTURE_CHORD } from "../../stores/global/hotkey";

/** The window's chord, as its rows in Settings name it. */
export const GLOBAL_ROW_LABEL = "Show and hide Writ";

/** The quick capture chord (ADR-055), as its row names it. */
export const CAPTURE_ROW_LABEL = "Quick capture";

/** What the window's chord cannot do on a Wayland desktop. */
export const WAYLAND_LINE = "Wayland doesn't pass this shortcut to Writ from other apps.";

/** The rule a chord for the window has to keep, as the row states it. */
export function windowChordRule(platform: Platform): string {
  return platform === "mac"
    ? "A shortcut that works from any app needs Command, Control or Option. F13 to F20 work on their own."
    : "A shortcut that works from any app needs Ctrl or Alt. F13 to F24 work on their own.";
}

/** The capture chord the config holds, or the shipped one. */
export function savedCaptureChord(): string {
  return configStore.config().hotkey.capture ?? SHIPPED_CAPTURE_CHORD;
}

/** The capture chord the OS holds, or the saved one before it answers. */
export function heldCaptureChord(): string {
  return captureHotkeyStore.chord() || savedCaptureChord();
}

/** What a key pressed on a row the OS holds for every app comes to: a chord
 * to take, a rule it breaks while the row keeps listening, or nothing yet. */
export type AnyAppChordReading =
  | { kind: "take"; binding: string }
  | { kind: "refuse"; text: string; resetRecorder: boolean }
  | { kind: "wait" };

/**
 * Reads a recorded key for a chord the OS would hold for every app. It has to
 * leave the other apps their keys, and cannot take `other`'s chord: the OS
 * holds one chord for one of the two rows.
 */
export function readAnyAppChord(
  outcome: RecorderOutcome,
  platform: Platform,
  other: { chord: string; label: string },
): AnyAppChordReading {
  if (outcome.kind === "bare") {
    return { kind: "refuse", text: windowChordRule(platform), resetRecorder: false };
  }
  if (outcome.kind !== "captured") return { kind: "wait" };
  if (!worksFromAnyApp(outcome.binding)) {
    return { kind: "refuse", text: windowChordRule(platform), resetRecorder: true };
  }
  if (sameChord(outcome.binding, other.chord)) {
    return { kind: "refuse", text: `${other.label} uses this shortcut.`, resetRecorder: true };
  }
  return { kind: "take", binding: outcome.binding };
}
