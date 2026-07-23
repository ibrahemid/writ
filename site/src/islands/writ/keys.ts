// Translates the app's chord strings (the `keybinding` field on
// EDITOR_COMMANDS and the markdown format commands, e.g. "CmdOrCtrl+Shift+D")
// into CodeMirror `KeyBinding.key` strings ("Mod-Shift-d"). The app runs those
// chords through its registry chord map; the demo binds them straight into a CM
// keymap, so the two must agree on the same physical keys.

const MODIFIERS: Record<string, string> = {
  CmdOrCtrl: 'Mod',
  Shift: 'Shift',
  Alt: 'Alt',
  Ctrl: 'Ctrl',
  Meta: 'Meta',
};

// A single alphabetic key is lowercased so it matches CodeMirror's normalized
// `event.key`; named keys (ArrowUp, Enter) and punctuation pass through.
function normalizeKeyPart(part: string): string {
  return part.length === 1 && /[A-Za-z]/.test(part) ? part.toLowerCase() : part;
}

export function toCmKey(chord: string): string {
  const parts = chord.split('+');
  const out: string[] = [];
  for (let i = 0; i < parts.length; i++) {
    const part = parts[i]!;
    const mod = MODIFIERS[part];
    if (mod && i < parts.length - 1) {
      out.push(mod);
    } else {
      out.push(normalizeKeyPart(part));
    }
  }
  return out.join('-');
}
