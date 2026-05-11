export type CommandScope = "app" | "editor";

export interface Command {
  id: string;
  label: string;
  description?: string;
  keybinding?: string;
  keybindingAliases?: string[];
  scope: CommandScope;
  execute: () => void;
}
