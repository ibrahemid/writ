import { canonicalChord, chordForRecording, isFunctionKey, type ChordEvent } from "../../lib/chord";
import { detectPlatform, type Platform } from "../../lib/platform";

export const DOUBLE_TAP_WINDOW_MS = 300;

const BARE_MODIFIERS: ReadonlySet<string> = new Set([
  "Shift",
  "Control",
  "Meta",
  "Alt",
]);

export type RecorderEvent = ChordEvent;

export type RecorderOutcome =
  | { kind: "captured"; binding: string }
  /** A key that types text, pressed with no modifier: no chord, and the row
   * keeps listening. */
  | { kind: "bare"; binding: string }
  | { kind: "cancelled" }
  | { kind: "waiting" };

interface PendingBareTap {
  modifier: "Shift" | "Control" | "Meta" | "Alt";
  at: number;
}

export class ShortcutRecorder {
  private pendingTap: PendingBareTap | null = null;
  private readonly windowMs: number;
  private readonly platform: Platform;

  constructor(windowMs: number = DOUBLE_TAP_WINDOW_MS, platform: Platform = detectPlatform()) {
    this.windowMs = windowMs;
    this.platform = platform;
  }

  reset(): void {
    this.pendingTap = null;
  }

  handle(event: RecorderEvent, nowMs: number = Date.now()): RecorderOutcome {
    if (event.key === "Escape") {
      this.pendingTap = null;
      return { kind: "cancelled" };
    }

    if (BARE_MODIFIERS.has(event.key)) {
      const modifier = event.key as PendingBareTap["modifier"];
      const previous = this.pendingTap;
      if (
        previous &&
        previous.modifier === modifier &&
        nowMs - previous.at <= this.windowMs
      ) {
        this.pendingTap = null;
        return { kind: "captured", binding: `${modifier}+${modifier}` };
      }
      this.pendingTap = { modifier, at: nowMs };
      return { kind: "waiting" };
    }

    this.pendingTap = null;

    const binding = chordForRecording(event, this.platform);
    // A key with no modifier types text, so it is no chord; a function key
    // types nothing, so it is one on its own (F2 renames).
    const bare = !binding.includes("+") || binding === "+";
    if (bare && !isFunctionKey(binding)) {
      return { kind: "bare", binding };
    }

    return { kind: "captured", binding };
  }
}

export function normalizeBinding(
  binding: string | undefined,
  platform: Platform = detectPlatform(),
): string {
  if (!binding) return "";
  return canonicalChord(binding, platform);
}

export function findConflicts(
  draft: Readonly<Record<string, string>>,
  platform: Platform = detectPlatform(),
): Map<string, string[]> {
  const groups = new Map<string, string[]>();
  for (const [id, binding] of Object.entries(draft)) {
    const normalized = normalizeBinding(binding, platform);
    if (!normalized) continue;
    const existing = groups.get(normalized) ?? [];
    existing.push(id);
    groups.set(normalized, existing);
  }

  const conflicts = new Map<string, string[]>();
  for (const [, ids] of groups) {
    if (ids.length < 2) continue;
    for (const id of ids) {
      conflicts.set(id, ids.filter((other) => other !== id));
    }
  }
  return conflicts;
}

/** A chord the window holds for an item that is no command, such as Quit. */
export interface FixedChord {
  chord: string;
  label: string;
}

/**
 * The rows whose chord a fixed item already holds, by command id, with that
 * item's name. The macOS menu bar answers its own chords before Writ sees the
 * key, so a command bound to one would never run.
 */
export function findFixedConflicts(
  draft: Readonly<Record<string, string>>,
  fixed: readonly FixedChord[],
  platform: Platform = detectPlatform(),
): Map<string, string> {
  const held = new Map(fixed.map((item) => [normalizeBinding(item.chord, platform), item.label]));
  const conflicts = new Map<string, string>();
  for (const [id, binding] of Object.entries(draft)) {
    const label = held.get(normalizeBinding(binding, platform));
    if (label !== undefined) conflicts.set(id, label);
  }
  return conflicts;
}
