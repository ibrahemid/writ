import type { Platform } from "./platform";

/** The fields of a `KeyboardEvent` a chord is read from. */
export interface ChordEvent {
  key: string;
  code?: string;
  metaKey?: boolean;
  ctrlKey?: boolean;
  shiftKey?: boolean;
  altKey?: boolean;
  getModifierState?: (modifier: string) => boolean;
}

const ASCII_LETTER_OR_DIGIT = /^[A-Za-z0-9]$/;
const FUNCTION_KEY = /^F([1-9]|1[0-9]|2[0-4])$/;

/**
 * Modifiers in the order every stored chord spells them. On macOS Command is
 * `CmdOrCtrl` and Control is `Ctrl`, so a Control chord stays the editor's
 * (Ctrl+N moves down a line); elsewhere either key is `CmdOrCtrl`.
 */
function modifierParts(event: ChordEvent, platform: Platform): string[] {
  const parts: string[] = [];
  if (platform === "mac") {
    if (event.metaKey) parts.push("CmdOrCtrl");
    if (event.ctrlKey) parts.push("Ctrl");
  } else if (event.metaKey || event.ctrlKey) {
    parts.push("CmdOrCtrl");
  }
  if (event.shiftKey && event.key !== "Shift") parts.push("Shift");
  if (event.altKey) parts.push("Alt");
  return parts;
}

function keyName(key: string): string {
  if (key === " ") return "Space";
  return key.length === 1 ? key.toUpperCase() : key;
}

/**
 * The Latin letter or digit printed at the key's position, for a key whose
 * own character is not one: a non-Latin layout (Arabic, Russian, Greek), an
 * Option-composed character on macOS, a dead key, or a shifted digit. `null`
 * under AltGr, where Ctrl+Alt is the layout typing a character rather than a
 * chord.
 */
function positionName(event: ChordEvent): string | null {
  if (ASCII_LETTER_OR_DIGIT.test(event.key)) return null;
  if (event.getModifierState?.("AltGraph")) return null;
  const code = event.code ?? "";
  const letter = /^Key([A-Z])$/.exec(code);
  if (letter) return letter[1];
  const digit = /^Digit([0-9])$/.exec(code);
  return digit ? digit[1] : null;
}

/** The chord spelled from the character the key produced. */
export function chordFromKey(event: ChordEvent, platform: Platform): string {
  return [...modifierParts(event, platform), keyName(event.key)].join("+");
}

/**
 * The chord spelled from the key's position, or `null` when the position
 * names nothing the character did not. Matching tries this only after the
 * character's chord misses, so punctuation, Dvorak and AZERTY letters keep
 * the meaning printed on the key.
 */
export function chordFromPosition(event: ChordEvent, platform: Platform): string | null {
  const name = positionName(event);
  if (name === null) return null;
  return [...modifierParts(event, platform), name].join("+");
}

/**
 * The chord a recorder stores: the character for a Latin letter, digit or
 * unshifted punctuation, the position for anything else, so a chord recorded
 * on one layout is the same keys on another.
 */
export function chordForRecording(event: ChordEvent, platform: Platform): string {
  const printable = event.key.length === 1 && event.key >= " " && event.key <= "~";
  const shiftedDigit = event.shiftKey === true && /^Digit[0-9]$/.test(event.code ?? "");
  if (printable && !shiftedDigit) return chordFromKey(event, platform);
  return chordFromPosition(event, platform) ?? chordFromKey(event, platform);
}

export function isFunctionKey(key: string): boolean {
  return FUNCTION_KEY.test(key);
}

/**
 * A chord's parts, modifiers first. A trailing `++` is the plus key, which a
 * plain split on `+` would lose.
 */
export function chordParts(binding: string): string[] {
  if (binding.endsWith("++")) {
    const head = binding.slice(0, -2);
    return head === "" ? ["+"] : [...head.split("+"), "+"];
  }
  return binding.split("+");
}

/**
 * A stored chord in the form this platform's events produce. Off macOS there
 * is one Control key, so a `Ctrl` recorded on a Mac is `CmdOrCtrl` here.
 */
export function canonicalChord(binding: string, platform: Platform): string {
  const trimmed = binding.trim();
  if (platform === "mac" || trimmed === "") return trimmed;
  const parts = chordParts(trimmed);
  const key = parts[parts.length - 1];
  const modifiers = parts.slice(0, -1);
  if (!modifiers.includes("Ctrl")) return trimmed;
  const merged = [...new Set(modifiers.map((part) => (part === "Ctrl" ? "CmdOrCtrl" : part)))];
  return [...merged, key].join("+");
}

/** Why the window's chord was not taken: Rust registers only these keys. */
export const UNUSABLE_GLOBAL_KEY =
  "Writ can't use that key. Pick a letter, number, punctuation, arrow, Space or F1–F12 key.";
