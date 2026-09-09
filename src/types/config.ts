import type { AccentId, ProseFace } from "../styles/generated/tokens";

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

export interface UpdaterConfig {
  auto_check: boolean;
}

/** A rewrite preset id, which is also the keychain account its key is stored
 * under. The rewrite path speaks `chat/completions` only. */
export type AiPreset = "ollama" | "groq" | "gemini" | "deepseek" | "openrouter" | "custom";

/** Which wire format the chat endpoint speaks. */
export type ChatProvider = "anthropic" | "openai_compatible";

/** The chat pane's own switch, endpoint and model (`[ai.chat]`). */
export interface AiChatConfig {
  enabled: boolean;
  provider: ChatProvider;
  base_url: string;
  model: string;
}

export interface AiConfig {
  enabled: boolean;
  preset: AiPreset;
  base_url: string;
  model: string;
  consented_hosts: string[];
  chat: AiChatConfig;
}

/** What one program may do with the notes folder (ADR-031 section 3). */
export interface ClientApproval {
  name: string;
  first_seen: string;
  read: boolean;
  write: boolean;
}

export interface McpConfig {
  enabled: boolean;
  approved_clients: ClientApproval[];
}

export type SpellingDialect = "american" | "british" | "canadian" | "australian";

/** Follow the OS setting, or pin one polarity. */
export type Polarity = "system" | "light" | "dark";

export type { AccentId } from "../styles/generated/tokens";
export type ProseFaceId = ProseFace;

export interface AppearanceConfig {
  polarity: Polarity;
  accent: AccentId;
  prose_face: ProseFaceId;
  /** Interface text size in px, or null to keep the platform's own size. */
  interface_text_size: number | null;
}

export interface SpellingConfig {
  enabled: boolean;
  dialect: string;
  ignored_words: string[];
}

/** The panel beside the note. Closed until it is opened once. */
export interface PanelConfig {
  open: boolean;
  /** Width in CSS pixels, clamped to the resize range. */
  width: number;
}

/** The chat pane's column: showing or not, and how wide. */
export interface ChatPanelConfig {
  open: boolean;
  /** Width in CSS pixels, clamped to the resize range. */
  width: number;
}

/** What the first launch has already been told. */
export interface FirstRunConfig {
  /** The one line under the cursor goes on the first keystroke and stays gone. */
  hint_dismissed: boolean;
}

export interface WritConfig {
  hotkey: { toggle: string };
  sidebar: {
    toggle: string;
    default_visible: boolean;
    position: "left" | "right";
    open: boolean;
    /** Width in CSS pixels, clamped to the resize range. */
    width: number;
  };
  panel: PanelConfig;
  chat_panel: ChatPanelConfig;
  first_run: FirstRunConfig;
  editor: { font_family: string; font_size: number; word_wrap: boolean; tab_size: number; autosave_debounce_ms: number; markdown_typography: boolean; markdown_editing: boolean; status_bar: boolean };
  window: { width: number; height: number; x?: number | null; y?: number | null; maximized: boolean };
  keybindings: Record<string, string>;
  history: { max_entries: number };
  storage: { path: string };
  theme: { preset: string; overrides: Record<string, string> };
  appearance: AppearanceConfig;
  commands: CommandsConfig;
  preview: PreviewConfig;
  workspace: { root: string | null };
  inbox: InboxConfig;
  updater: UpdaterConfig;
  ai: AiConfig;
  mcp: McpConfig;
  spelling: SpellingConfig;
}
