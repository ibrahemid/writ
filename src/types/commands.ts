export type CommandScope = "app" | "editor";

export interface Command {
  id: string;
  label: string;
  description?: string;
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
   * Returns `false` when the command declined to act (no view, read-only, or a
   * no-op), so the dispatcher lets the keystroke fall through to CodeMirror and
   * the browser. Any other return value counts as handled — `void` for the
   * common case, or a `Promise` for the fire-and-forget async handlers, which a
   * plain `boolean | void` would reject.
   */
  execute: () => boolean | void | Promise<unknown>;
}
