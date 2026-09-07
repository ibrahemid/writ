export type SettingsSection =
  | "notes"
  | "editor"
  | "files"
  | "preview"
  | "ai"
  | "appearance"
  | "updates"
  | "shortcuts"
  | "advanced";

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
  notes: "Notes",
  editor: "Editor",
  files: "Files",
  preview: "Preview",
  ai: "AI rewriting",
  appearance: "Appearance",
  updates: "Updates",
  shortcuts: "Shortcuts",
  advanced: "Advanced",
};

export const SECTION_ORDER: SettingsSection[] = [
  "notes",
  "editor",
  "files",
  "preview",
  "ai",
  "appearance",
  "updates",
  "shortcuts",
  "advanced",
];

export const SETTINGS_INDEX: SettingEntry[] = [
  { id: "notes.folder", section: "notes", title: "Notes folder", keywords: ["notes", "folder", "where are my notes", "location", "path", "finder", "backup", "sync", "icloud", "dropbox"] },
  { id: "editor.font_size", section: "editor", title: "Font size", keywords: ["font", "size", "text", "zoom"] },
  { id: "editor.tab_size", section: "editor", title: "Tab size", keywords: ["tab", "indent", "spaces", "width"] },
  { id: "editor.word_wrap", section: "editor", title: "Word wrap", keywords: ["wrap", "word", "line", "soft wrap"] },
  { id: "editor.markdown_typography", section: "editor", title: "Style headings and bold text as you type", keywords: ["markdown", "headings", "bold", "italic", "styling", "formatting", "live"] },
  { id: "editor.markdown_editing", section: "editor", title: "Markdown shortcuts", keywords: ["markdown", "editing", "bold", "italic", "strikethrough", "link", "shortcuts", "checkbox", "list"] },
  { id: "editor.spelling", section: "editor", title: "Spell check", keywords: ["spell", "spelling", "check", "dictionary", "typos", "grammar"] },
  { id: "editor.spelling_dialect", section: "editor", title: "Spelling", keywords: ["spelling", "english", "us", "uk", "american", "british", "canadian", "australian"] },
  { id: "editor.status_bar", section: "editor", title: "Status bar", keywords: ["status bar", "line", "column", "encoding", "word count"] },
  { id: "files.default_app", section: "files", title: "Open these file types with Writ", keywords: ["default", "default app", "open with", "file association", "txt", "text", "log", "markdown", "md", "json", "yaml", "toml", "config", "data", "csv", "code", "rust", "typescript", "python"] },
  { id: "preview.run_scripts", section: "preview", title: "Allow HTML files to run their scripts", keywords: ["scripts", "javascript", "html", "run", "safety"] },
  { id: "preview.layout_md", section: "preview", title: "When opening a Markdown file, show:", keywords: ["layout", "markdown", "md", "text", "preview", "split"] },
  { id: "preview.layout_html", section: "preview", title: "When opening an HTML file, show:", keywords: ["layout", "html", "text", "preview", "split"] },
  { id: "ai.enabled", section: "ai", title: "Rewrite selected text", keywords: ["ai", "rewrite", "proofread", "rephrase", "polish", "model", "llm", "ollama", "enable"] },
  { id: "ai.preset", section: "ai", title: "Provider", keywords: ["ai", "provider", "preset", "ollama", "groq", "gemini", "deepseek", "openrouter", "custom"] },
  { id: "ai.base_url", section: "ai", title: "Base URL", keywords: ["ai", "base url", "endpoint", "host", "server"] },
  { id: "ai.model", section: "ai", title: "Model", keywords: ["ai", "model", "id"] },
  { id: "ai.api_key", section: "ai", title: "API key", keywords: ["ai", "api key", "token", "secret", "credential"] },
  { id: "appearance.polarity", section: "appearance", title: "Light and dark", keywords: ["appearance", "light", "dark", "system", "theme", "polarity", "follow system"] },
  { id: "appearance.accent", section: "appearance", title: "Accent color", keywords: ["accent", "color", "pine", "highlight"] },
  { id: "appearance.prose_face", section: "appearance", title: "Prose typeface", keywords: ["font", "typeface", "prose", "writing", "ia writer", "quattro"] },
  { id: "appearance.interface_text_size", section: "appearance", title: "Interface text size", keywords: ["interface", "text", "size", "ui", "font", "scale", "bigger", "smaller", "sidebar", "tabs"] },
  { id: "appearance.theme", section: "appearance", title: "Theme", keywords: ["theme", "color", "appearance", "preset", "dark", "light"] },
  { id: "appearance.custom_colors", section: "appearance", title: "Custom colors", keywords: ["theme", "colors", "custom", "palette"] },
  { id: "updates.auto_check", section: "updates", title: "Check for updates automatically", keywords: ["update", "auto", "check", "version"] },
  { id: "updates.check_now", section: "updates", title: "Check for updates now", keywords: ["update", "check", "now", "version"] },
  { id: "shortcuts.edit", section: "shortcuts", title: "Keyboard shortcuts", keywords: ["shortcut", "keyboard", "keybinding", "hotkey", "rebind"] },
  { id: "files.cli", section: "advanced", title: "Terminal command", keywords: ["cli", "writ command", "terminal", "command line", "install"] },
  { id: "files.inbox_folder", section: "advanced", title: "Folder to watch for new files", keywords: ["watch", "watched folder", "new files", "auto-open", "drop"] },
  { id: "files.inbox_focus", section: "advanced", title: "Bring Writ to the front when a new file arrives", keywords: ["watch", "focus", "window", "front", "new file"] },
  { id: "preview.live_threshold", section: "advanced", title: "Stop live preview above", keywords: ["preview", "live", "size", "mb", "large files"] },
  { id: "preview.refuse_threshold", section: "advanced", title: "Do not preview files above", keywords: ["preview", "limit", "size", "mb", "large files"] },
  { id: "storage.location", section: "advanced", title: "Writ's data folder", keywords: ["database", "sqlite", "db", "writ.db", "index", "logs", "settings file", "app data"] },
];

/** The one row offering Writ as the handler for the claimable file types. */
export const DEFAULT_APP_SETTING_ID = "files.default_app";

/**
 * How well one row answers a query, highest first:
 *
 *   6 the title is the query
 *   5 a keyword is the query — the row claimed that exact word
 *   4 the title starts with it
 *   3 the title contains it
 *   2 a keyword contains it
 *   1 the section label contains it
 *
 * A keyword equal to the query also contains it, so the exact tier moves rows
 * within the results and never adds or drops one. It is what puts `Notes
 * folder`, which claims `folder`, above `Folder to watch for new files`, which
 * only happens to start with the word.
 */
function scoreEntry(entry: SettingEntry, queryLower: string): number {
  const title = entry.title.toLowerCase();
  if (title === queryLower) return 6;
  if (entry.keywords.some((k) => k.toLowerCase() === queryLower)) return 5;
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
