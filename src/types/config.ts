export interface CommandUsage {
  count: number;
  last_used_ms: number;
}

export interface CommandsConfig {
  usage: Record<string, CommandUsage>;
}

export interface WritConfig {
  hotkey: { toggle: string };
  sidebar: {
    toggle: string;
    default_visible: boolean;
    position: "left" | "right";
    open: boolean;
  };
  editor: { font_family: string; font_size: number; word_wrap: boolean; tab_size: number; autosave_debounce_ms: number };
  window: { width: number; height: number };
  keybindings: Record<string, string>;
  history: { max_entries: number };
  storage: { path: string };
  theme: { preset: string; overrides: Record<string, string> };
  commands: CommandsConfig;
}
