// Pure data: the editor command table without any run functions or imports, so
// build-time consumers (the marketing site's keycap sheet) can read the labels
// and chords without pulling the CodeMirror graph in. editor-command-table.ts
// maps these onto their CM commands; this stays the single source of truth for
// ids, labels, and keybindings.

export interface EditorCommandKey {
  id: string;
  label: string;
  description?: string;
  keybinding: string;
  aliases?: string[];
}

// Chord strings must match `normalizeKey` output exactly: modifier order is
// CmdOrCtrl, Shift, Alt, then the key, and arrows keep their DOM names. The
// `CmdOrCtrl+Shift+/` alias covers layouts where `/` needs Shift; on US layouts
// Shift+/ yields `?`, so the alias is inert there rather than wrong.
export const EDITOR_COMMAND_KEYS: readonly EditorCommandKey[] = [
  { id: "editor.duplicateLine", label: "Duplicate Line", keybinding: "CmdOrCtrl+D" },
  { id: "editor.deleteLine", label: "Delete Line", keybinding: "CmdOrCtrl+E", aliases: ["CmdOrCtrl+Shift+K"] },
  { id: "editor.moveLineUp", label: "Move Line Up", keybinding: "Shift+Alt+ArrowUp" },
  { id: "editor.moveLineDown", label: "Move Line Down", keybinding: "Shift+Alt+ArrowDown" },
  { id: "editor.toggleComment", label: "Toggle Comment", keybinding: "CmdOrCtrl+/", aliases: ["CmdOrCtrl+Shift+/"] },
  { id: "editor.selectLine", label: "Select Line", keybinding: "CmdOrCtrl+L" },
  { id: "editor.insertLineBelow", label: "Insert Line Below", keybinding: "CmdOrCtrl+Enter" },
  { id: "editor.insertLineAbove", label: "Insert Line Above", keybinding: "CmdOrCtrl+Shift+Enter" },
  { id: "editor.joinLines", label: "Join Lines", keybinding: "CmdOrCtrl+Shift+J" },
  { id: "editor.selectNextOccurrence", label: "Select Next Occurrence", keybinding: "CmdOrCtrl+Shift+D" },
];
