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
  { id: "editor.duplicateLine", label: "Duplicate line", keybinding: "CmdOrCtrl+D" },
  // Cmd+E is Eject in Finder and Use Selection for Find in Cocoa, so deleting
  // a line answers to Cmd+Shift+K alone.
  { id: "editor.deleteLine", label: "Delete line", keybinding: "CmdOrCtrl+Shift+K" },
  { id: "editor.moveLineUp", label: "Move line up", keybinding: "Shift+Alt+ArrowUp" },
  { id: "editor.moveLineDown", label: "Move line down", keybinding: "Shift+Alt+ArrowDown" },
  { id: "editor.toggleComment", label: "Toggle comment", keybinding: "CmdOrCtrl+/", aliases: ["CmdOrCtrl+Shift+/"] },
  { id: "editor.selectLine", label: "Select line", keybinding: "CmdOrCtrl+L" },
  { id: "editor.insertLineBelow", label: "Insert line below", keybinding: "CmdOrCtrl+Enter" },
  { id: "editor.insertLineAbove", label: "Insert line above", keybinding: "CmdOrCtrl+Shift+Enter" },
  { id: "editor.joinLines", label: "Join lines", keybinding: "CmdOrCtrl+Shift+J" },
  { id: "editor.selectNextOccurrence", label: "Select next occurrence", keybinding: "CmdOrCtrl+Shift+D" },
];

/**
 * The Markdown formatting commands, registered while a Markdown file is in
 * front with Markdown shortcuts on (EditorInstance). Settings names their
 * chords from here while no Markdown file is open to register them.
 */
export const MARKDOWN_FORMAT_KEYS = [
  { id: "editor.toggleBold", label: "Bold", keybinding: "CmdOrCtrl+B" },
  { id: "editor.toggleItalic", label: "Italic", keybinding: "CmdOrCtrl+I" },
  { id: "editor.toggleStrikethrough", label: "Strikethrough", keybinding: "CmdOrCtrl+Shift+X" },
  { id: "editor.toggleInlineCode", label: "Inline code", keybinding: "CmdOrCtrl+Shift+E" },
  { id: "editor.insertLink", label: "Insert link", keybinding: "CmdOrCtrl+K" },
  { id: "editor.toggleBulletList", label: "Bulleted list", keybinding: undefined },
  { id: "editor.toggleTaskList", label: "Task list", keybinding: undefined },
] as const;
