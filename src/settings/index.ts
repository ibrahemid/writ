export type SettingsSection =
  | "editor"
  | "files"
  | "preview"
  | "appearance"
  | "updates"
  | "shortcuts";

export interface SettingEntry {
  /** Stable id; also the `data-setting-id` on the rendered row. */
  id: string;
  section: SettingsSection;
  /** Human label shown in search results and matched first. */
  title: string;
  /** Extra terms that should surface this setting in a search. */
  keywords: string[];
}

export const SECTION_LABELS: Record<SettingsSection, string> = {
  editor: "Editor",
  files: "Files",
  preview: "Preview",
  appearance: "Appearance",
  updates: "Updates",
  shortcuts: "Shortcuts",
};

export const SECTION_ORDER: SettingsSection[] = [
  "editor",
  "files",
  "preview",
  "appearance",
  "updates",
  "shortcuts",
];

export const SETTINGS_INDEX: SettingEntry[] = [
  { id: "editor.font_size", section: "editor", title: "Font size", keywords: ["font", "size", "text", "zoom"] },
  { id: "editor.tab_size", section: "editor", title: "Tab size", keywords: ["tab", "indent", "spaces", "width"] },
  { id: "editor.word_wrap", section: "editor", title: "Word wrap", keywords: ["wrap", "word", "line", "soft wrap"] },
  { id: "files.autosave", section: "files", title: "Autosave delay", keywords: ["autosave", "save", "delay", "debounce"] },
  { id: "files.cli", section: "files", title: "Command-line tool", keywords: ["cli", "writ command", "terminal", "command line", "install"] },
  { id: "files.default_app.plain-text", section: "files", title: "Default app for plain text & logs", keywords: ["default", "open with", "file association", "txt", "text", "log"] },
  { id: "files.default_app.markdown", section: "files", title: "Default app for Markdown", keywords: ["default", "open with", "file association", "markdown", "md"] },
  { id: "files.default_app.config-data", section: "files", title: "Default app for config & data", keywords: ["default", "open with", "file association", "json", "yaml", "toml", "config", "data", "csv"] },
  { id: "files.default_app.source-code", section: "files", title: "Default app for source code", keywords: ["default", "open with", "file association", "source", "code", "rust", "typescript", "python"] },
  { id: "files.inbox_folder", section: "files", title: "Watched inbox folder", keywords: ["inbox", "watch", "folder", "auto-open"] },
  { id: "files.inbox_focus", section: "files", title: "Focus window on inbox open", keywords: ["inbox", "focus", "window"] },
  { id: "preview.live_threshold", section: "preview", title: "Live render threshold", keywords: ["preview", "live", "render", "threshold", "size", "mb"] },
  { id: "preview.refuse_threshold", section: "preview", title: "Refuse render threshold", keywords: ["preview", "render", "refuse", "threshold", "limit", "mb"] },
  { id: "preview.run_scripts", section: "preview", title: "Run scripts by default", keywords: ["scripts", "javascript", "preview", "run"] },
  { id: "preview.layout_html", section: "preview", title: "HTML default layout", keywords: ["layout", "html", "split", "source", "preview"] },
  { id: "preview.layout_md", section: "preview", title: "Markdown default layout", keywords: ["layout", "markdown", "md", "split", "source", "preview"] },
  { id: "appearance.theme", section: "appearance", title: "Theme", keywords: ["theme", "color", "appearance", "preset", "dark", "light"] },
  { id: "appearance.custom_colors", section: "appearance", title: "Custom colors", keywords: ["theme", "colors", "custom", "palette"] },
  { id: "updates.auto_check", section: "updates", title: "Check for updates automatically", keywords: ["update", "auto", "check", "version"] },
  { id: "updates.check_now", section: "updates", title: "Check for updates now", keywords: ["update", "check", "now", "version"] },
  { id: "shortcuts.edit", section: "shortcuts", title: "Keyboard shortcuts", keywords: ["shortcut", "keyboard", "keybinding", "hotkey", "rebind"] },
];

/** Prefix shared by the platform-gated "Default app for …" rows. */
export const DEFAULT_APP_SETTING_PREFIX = "files.default_app.";

/** Build the setting id for a claimable default-app type (mirrors the row's `data-setting-id`). */
export function defaultAppSettingId(typeId: string): string {
  return `${DEFAULT_APP_SETTING_PREFIX}${typeId}`;
}

function scoreEntry(entry: SettingEntry, queryLower: string): number {
  const title = entry.title.toLowerCase();
  if (title === queryLower) return 5;
  if (title.startsWith(queryLower)) return 4;
  if (title.includes(queryLower)) return 3;
  if (entry.keywords.some((k) => k.toLowerCase().includes(queryLower))) return 2;
  if (SECTION_LABELS[entry.section].toLowerCase().includes(queryLower)) return 1;
  return -1;
}

/**
 * Rank settings against a free-text query. An empty query returns no matches:
 * settings are search-only surfaces, never the full empty-state listing.
 */
export function rankSettings(
  query: string,
  entries: ReadonlyArray<SettingEntry> = SETTINGS_INDEX,
): SettingEntry[] {
  const q = query.toLowerCase().trim();
  if (!q) return [];

  return entries
    .map((entry) => ({ entry, score: scoreEntry(entry, q) }))
    .filter((row) => row.score > 0)
    .sort((a, b) => {
      if (b.score !== a.score) return b.score - a.score;
      return a.entry.title.localeCompare(b.entry.title, undefined, { sensitivity: "base" });
    })
    .map((row) => row.entry);
}

export function matchedSettingIds(query: string): Set<string> {
  return new Set(rankSettings(query).map((entry) => entry.id));
}

export function sectionHasMatch(section: SettingsSection, query: string): boolean {
  const q = query.toLowerCase().trim();
  if (!q) return true;
  return rankSettings(query).some((entry) => entry.section === section);
}
