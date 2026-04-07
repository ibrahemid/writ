export type CommandScope = "app" | "editor";

export interface Command {
  id: string;
  label: string;
  keybinding?: string;
  scope: CommandScope;
  execute: () => void;
}
