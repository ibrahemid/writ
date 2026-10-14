import { chordParts } from "./chord";

const IS_MAC = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);

const KEY_GLYPHS: Readonly<Record<string, string>> = {
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
  Escape: "Esc",
};

const MAC_KEY_GLYPHS: Readonly<Record<string, string>> = {
  Enter: "↩",
  Backspace: "⌫",
  Delete: "⌦",
  Tab: "⇥",
};

function partToGlyph(part: string, mac: boolean): string {
  if (part === "CmdOrCtrl") return mac ? "⌘" : "Ctrl";
  if (part === "Shift") return mac ? "⇧" : "Shift";
  if (part === "Alt") return mac ? "⌥" : "Alt";
  if (part === "Ctrl") return mac ? "⌃" : "Ctrl";
  return (mac ? MAC_KEY_GLYPHS[part] : undefined) ?? KEY_GLYPHS[part] ?? part;
}

const SPOKEN_KEYS: Readonly<Record<string, string>> = {
  ArrowUp: "Up Arrow",
  ArrowDown: "Down Arrow",
  ArrowLeft: "Left Arrow",
  ArrowRight: "Right Arrow",
  "\\": "Backslash",
  "/": "Slash",
  ",": "Comma",
  ".": "Period",
  ";": "Semicolon",
  "'": "Quote",
  "`": "Backquote",
  "[": "Left Bracket",
  "]": "Right Bracket",
  "=": "Equals",
  "-": "Minus",
  "+": "Plus",
};

function partToWord(part: string, mac: boolean): string {
  if (part === "CmdOrCtrl") return mac ? "Command" : "Control";
  if (part === "Ctrl") return "Control";
  if (part === "Alt") return mac ? "Option" : "Alt";
  return SPOKEN_KEYS[part] ?? part;
}

export function formatKeybinding(binding: string | undefined, opts?: { isMac?: boolean }): string {
  if (!binding) return "";
  const mac = opts?.isMac ?? IS_MAC;

  if (binding === "Shift+Shift") return mac ? "⇧ ⇧" : "Shift Shift";

  return chordParts(binding)
    .map((part) => partToGlyph(part, mac))
    .join(mac ? "" : "+");
}

export function keybindingSegments(binding: string | undefined, opts?: { isMac?: boolean }): string[] {
  if (!binding) return [];
  const mac = opts?.isMac ?? IS_MAC;
  if (binding === "Shift+Shift") {
    return mac ? ["⇧", "⇧"] : ["Shift", "Shift"];
  }
  return chordParts(binding).map((part) => partToGlyph(part, mac));
}

/** The chord in words, for a screen reader: "Command Shift P", "Shift Shift". */
export function spokenKeybinding(binding: string | undefined, opts?: { isMac?: boolean }): string {
  if (!binding) return "";
  const mac = opts?.isMac ?? IS_MAC;
  return chordParts(binding)
    .map((part) => partToWord(part, mac))
    .join(" ");
}
