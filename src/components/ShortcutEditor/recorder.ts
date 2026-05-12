export const DOUBLE_TAP_WINDOW_MS = 300;

const BARE_MODIFIERS: ReadonlySet<string> = new Set([
  "Shift",
  "Control",
  "Meta",
  "Alt",
]);

export interface RecorderEvent {
  key: string;
  metaKey?: boolean;
  ctrlKey?: boolean;
  shiftKey?: boolean;
  altKey?: boolean;
}

export type RecorderOutcome =
  | { kind: "captured"; binding: string }
  | { kind: "cancelled" }
  | { kind: "waiting" };

interface PendingBareTap {
  modifier: "Shift" | "Control" | "Meta" | "Alt";
  at: number;
}

export class ShortcutRecorder {
  private pendingTap: PendingBareTap | null = null;
  private readonly windowMs: number;

  constructor(windowMs: number = DOUBLE_TAP_WINDOW_MS) {
    this.windowMs = windowMs;
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

    const parts: string[] = [];
    if (event.metaKey || event.ctrlKey) parts.push("CmdOrCtrl");
    if (event.shiftKey) parts.push("Shift");
    if (event.altKey) parts.push("Alt");

    let key = event.key;
    if (key === " ") key = "Space";
    if (key.length === 1) key = key.toUpperCase();
    parts.push(key);

    if (parts.length === 1) {
      return { kind: "waiting" };
    }

    return { kind: "captured", binding: parts.join("+") };
  }
}

export function normalizeBinding(binding: string | undefined): string {
  if (!binding) return "";
  return binding.trim();
}

export function findConflicts(
  draft: Readonly<Record<string, string>>,
): Map<string, string[]> {
  const groups = new Map<string, string[]>();
  for (const [id, binding] of Object.entries(draft)) {
    const normalized = normalizeBinding(binding);
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
