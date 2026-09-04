export type CommandScope = "app" | "editor";

export interface Command {
  id: string;
  label: string;
  description?: string;
  /**
   * Extra search terms for the palette, for words a user would reasonably type
   * that appear in neither the label nor the description. Ranked above the
   * description and below the id. Mirrors the `keywords` field the settings
   * index already uses.
   */
  keywords?: string[];
  keybinding?: string;
  keybindingAliases?: string[];
  scope: CommandScope;
  /**
   * App-scoped commands fire even while the user is typing in the editor or a
   * text input only when this is true. Editor-scoped commands are always
   * delivered to a focused editor regardless of this flag.
   */
  global?: boolean;
  /**
   * Whether the command can act right now. A command that answers `false` is
   * kept out of the palette, so a surface never offers an action it would
   * only stop: `note.delete` on a file the user opened from somebody else's
   * folder is not a Delete Writ may perform. Omitted means always available,
   * which is every command that does not depend on what is open.
   */
  isAvailable?: () => boolean;
  /**
   * Returns `false` when the command declined to act (no view, read-only, or a
   * no-op), so the dispatcher lets the keystroke fall through to CodeMirror and
   * the browser. Any other return value counts as handled — `void` for the
   * common case, or a `Promise` for the fire-and-forget async handlers, which a
   * plain `boolean | void` would reject.
   */
  execute: () => boolean | void | Promise<unknown>;
}
