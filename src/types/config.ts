export interface CommandUsage {
  count: number;
  last_used_ms: number;
}

export interface CommandsConfig {
  usage: Record<string, CommandUsage>;
}

export type DefaultLayout = "source" | "split" | "preview";

export interface PreviewConfig {
  default_layout_html: DefaultLayout;
  default_layout_markdown: DefaultLayout;
  live_render_threshold_mb: number;
  render_confirm_threshold_mb: number;
  render_refuse_threshold_mb: number;
  debounce_ms: number;
  run_scripts: boolean;
}

export interface InboxConfig {
  path: string | null;
  focus: boolean;
}

export interface WritConfig {
  hotkey: { toggle: string };
  sidebar: {
    toggle: string;
    default_visible: boolean;
    position: "left" | "right";
    open: boolean;
  };
  editor: { font_family: string; font_size: number; word_wrap: boolean; tab_size: number; autosave_debounce_ms: number; markdown_typography: boolean };
  window: { width: number; height: number; x?: number | null; y?: number | null };
  keybindings: Record<string, string>;
  history: { max_entries: number };
  storage: { path: string };
  theme: { preset: string; overrides: Record<string, string> };
  commands: CommandsConfig;
  preview: PreviewConfig;
  workspace: { root: string | null };
  inbox: InboxConfig;
}
