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
  execute: () => void;
}
