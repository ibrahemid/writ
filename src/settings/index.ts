import { APPS } from "../lib/apps";
import type { AppId } from "../types/config";

export type SettingsSection =
  | "general"
  | "editor"
  | "appearance"
  | "apps"
  | "integrations"
  | "shortcuts"
  | "advanced"
  | "about";

export interface SettingEntry {
  /** Stable id; also the `data-setting-id` on the rendered row. */
  id: string;
  section: SettingsSection;
  /** The row's label on screen and its name in search: one string for both. */
  title: string;
  /** One plain sentence under the label: what the row does, beyond its name. */
  description: string;
  /** Extra terms that should surface this setting in a search. */
  keywords: string[];
  /** Apps any one of which must be on for the row to be drawn. A row whose
   * apps are all off is not found by search either. */
  requires?: readonly AppId[];
  /** The app pages under Apps that hold the row. A link to it opens the first
   * of them whose app is on. A row with none sits on the section itself. */
  pages?: readonly AppId[];
}

export class UnknownSettingError extends Error {
  constructor(id: string) {
    super(`no settings entry is named ${id}`);
    this.name = "UnknownSettingError";
  }
}

export const SECTION_LABELS: Record<SettingsSection, string> = {
  general: "General",
  editor: "Editor",
  appearance: "Appearance",
  apps: "Apps",
  integrations: "Integrations",
  shortcuts: "Shortcuts",
  advanced: "Advanced",
  about: "About",
};

export const SECTION_ORDER: SettingsSection[] = [
  "general",
  "editor",
  "appearance",
  "apps",
  "integrations",
  "shortcuts",
  "advanced",
  "about",
];

const AI_PAGES: readonly AppId[] = ["chat", "rewrite"];

function appDetail(id: AppId): string {
  return APPS.find((app) => app.id === id)?.detail ?? "";
}
const PROGRAMS_PAGE: readonly AppId[] = ["programs"];

export const SETTINGS_INDEX: SettingEntry[] = [
  { id: "notes.folder", section: "general", title: "Folder", description: "New files are saved here. Move… puts them in a folder you pick.", keywords: ["files", "folder", "where are my files", "location", "path", "finder", "backup", "sync", "icloud", "dropbox"] },
  { id: "files.default_extension", section: "general", title: "Default format", description: "For files you create. A file you name keeps the extension you type.", keywords: ["format", "default", "extension", "txt", "text", "plain text", "md", "markdown", "new file"] },
  { id: "hotkey.toggle", section: "general", title: "Show and hide Writ", description: "Works from any app, even while Writ is hidden.", keywords: ["shortcut", "global", "hotkey", "summon", "show", "hide", "window", "chord", "keyboard", "toggle", "from any app", "taken"] },
  { id: "general.launch_at_login", section: "general", title: "Open at login", description: "Starts hidden.", keywords: ["login", "startup", "start", "boot", "autostart", "launch", "restart", "hidden"] },
  { id: "editor.font_size", section: "editor", title: "Font size", description: "Text size in the editor and the preview.", keywords: ["font", "size", "text", "zoom"] },
  { id: "editor.tab_size", section: "editor", title: "Tab size", description: "Spaces per indent, and the width of a tab character.", keywords: ["tab", "indent", "spaces", "width"] },
  { id: "editor.word_wrap", section: "editor", title: "Word wrap", description: "Long lines wrap at the edge of the pane instead of scrolling sideways.", keywords: ["wrap", "word", "line", "soft wrap"] },
  { id: "editor.reading_width", section: "editor", title: "Reading width", description: "Prose lines stop at a set width, centered in the pane. Code files keep the full width.", keywords: ["reading", "width", "measure", "line length", "column", "narrow", "center"] },
  { id: "editor.markdown_editing", section: "editor", title: "Markdown shortcuts", description: "In Markdown files, typing *, _, ~ or ` over a selection wraps it.", keywords: ["markdown", "editing", "bold", "italic", "strikethrough", "link", "shortcuts", "checkbox", "list"] },
  { id: "preview.layout_md", section: "editor", title: "Markdown opens in", description: "Inline formats Markdown as you type. Source shows the raw text.", keywords: ["layout", "markdown", "md", "text", "preview", "inline", "source"] },
  { id: "editor.spelling", section: "editor", title: "Spell check", description: "Runs on this computer. Code, links, acronyms and camelCase words are skipped.", keywords: ["spell", "spelling", "check", "dictionary", "typos", "grammar"] },
  { id: "editor.spelling_dialect", section: "editor", title: "Dictionary", description: "Which English spelling counts as correct.", keywords: ["spelling", "english", "us", "uk", "american", "british", "canadian", "australian"] },
  { id: "editor.status_bar", section: "editor", title: "Status bar", description: "The line under the editor with the cursor position and the encoding.", keywords: ["status bar", "line", "column", "encoding", "word count"] },
  { id: "editor.status_bar_counts", section: "editor", title: "Word, character and token counts", description: "Adds them to the status bar. The token count is an estimate.", keywords: ["word count", "character count", "token", "counts", "status bar"] },
  { id: "sidebar.folder", section: "editor", title: "Show files", description: "Lists the folder's files in the sidebar.", keywords: ["sidebar", "folder", "files", "tree", "show", "hide"] },
  { id: "sidebar.recent", section: "editor", title: "Show recently closed", description: "Lists the files you closed last in the sidebar.", keywords: ["sidebar", "recent", "recently closed", "closed", "show", "hide"] },
  { id: "appearance.polarity", section: "appearance", title: "Light and dark", description: "System follows your computer's light or dark setting.", keywords: ["appearance", "light", "dark", "system", "theme", "polarity", "follow system"] },
  { id: "appearance.accent", section: "appearance", title: "Accent color", description: "The highlight color of switches, links and selected items.", keywords: ["accent", "color", "pine", "highlight"] },
  { id: "appearance.prose_face", section: "appearance", title: "Prose typeface", description: "Used for the text you write. Menus and Settings keep the system font.", keywords: ["font", "typeface", "prose", "writing", "ia writer", "quattro"] },
  { id: "appearance.interface_text_size", section: "appearance", title: "Interface text size", description: "Text size of the sidebar, menus and Settings. The editor has its own size.", keywords: ["interface", "text", "size", "ui", "font", "scale", "bigger", "smaller", "sidebar", "tabs"] },
  { id: "appearance.theme", section: "appearance", title: "Theme", description: "The colors of the whole window.", keywords: ["theme", "color", "appearance", "preset", "dark", "light"] },
  { id: "appearance.custom_colors", section: "appearance", title: "Custom colors", description: "Change single colors of the current theme.", keywords: ["theme", "colors", "custom", "palette"] },
  { id: "ai.chat.enabled", section: "apps", title: "Chat", description: appDetail("chat"), keywords: ["app", "ai", "chat", "ask", "conversation", "attach", "enable"] },
  { id: "ai.rewrite.enabled", section: "apps", title: "Rewrite", description: appDetail("rewrite"), keywords: ["app", "ai", "rewrite", "proofread", "rephrase", "polish", "selection", "enable"] },
  { id: "mcp.enabled", section: "apps", title: "Connected programs", description: appDetail("programs"), keywords: ["app", "mcp", "programs", "clients", "connect", "claude", "cursor", "editor", "assistant", "tools", "server", "enable"] },
  { id: "apps.connections", section: "apps", title: "Connections", description: appDetail("connections"), keywords: ["app", "connections", "links", "backlinks", "outline", "properties", "panel"] },
  { id: "apps.graph", section: "apps", title: "Graph", description: appDetail("graph"), keywords: ["app", "graph", "map", "links", "nearby"] },
  { id: "apps.tags", section: "apps", title: "Tags", description: appDetail("tags"), keywords: ["app", "tags", "hashtag", "filter", "sidebar"] },
  { id: "apps.offers", section: "apps", title: "Offer apps that are off", description: "Once per app, when an open file has links or tags or a program asks for your files.", keywords: ["apps", "offer", "suggest", "notice", "tips", "links", "tags"] },
  { id: "ai.provider", section: "apps", title: "Provider", description: "The service that runs the model.", keywords: ["ai", "provider", "ollama", "lm studio", "anthropic", "openai", "gemini", "openrouter", "groq", "deepseek", "mistral", "xai", "together", "fireworks", "custom"], requires: AI_PAGES, pages: AI_PAGES },
  { id: "ai.base_url", section: "apps", title: "Base URL", description: "Most servers expect /v1 at the end.", keywords: ["ai", "base url", "endpoint", "host", "server"], requires: AI_PAGES, pages: AI_PAGES },
  { id: "ai.api_key", section: "apps", title: "API key", description: "Kept in your system keychain when it is available.", keywords: ["ai", "api key", "token", "secret", "credential"], requires: AI_PAGES, pages: AI_PAGES },
  { id: "ai.model", section: "apps", title: "Model", description: "Used by Chat and Rewrite.", keywords: ["ai", "model", "id"], requires: AI_PAGES, pages: AI_PAGES },
  { id: "ai.connection", section: "apps", title: "Connection", description: "Asks the server for its model list.", keywords: ["ai", "connection", "check", "reachable", "status"], requires: AI_PAGES, pages: AI_PAGES },
  { id: "ai.chat.model", section: "apps", title: "Use a different model for chat", description: "Off means chat uses the model above.", keywords: ["ai", "chat", "model", "id", "different"], requires: ["chat"], pages: ["chat"] },
  { id: "mcp.waiting", section: "apps", title: "Waiting for you", description: "Approve a program here before it can read or write your files.", keywords: ["mcp", "waiting", "approve", "allow", "programs", "claude", "cursor", "permission"], requires: PROGRAMS_PAGE, pages: PROGRAMS_PAGE },
  { id: "mcp.connect", section: "apps", title: "Connect a program", description: "Add puts Writ in the program's MCP settings. Its first call then waits for your approval.", keywords: ["mcp", "connect", "add", "install", "claude code", "codex", "gemini cli", "cursor", "vs code", "claude desktop", "test", "setup"], requires: PROGRAMS_PAGE, pages: PROGRAMS_PAGE },
  { id: "mcp.command", section: "apps", title: "Command to give a program", description: "Paste it into a program's MCP settings to connect it to Writ.", keywords: ["mcp", "command", "copy", "paste", "configure", "setup", "stdio"], requires: PROGRAMS_PAGE, pages: PROGRAMS_PAGE },
  { id: "mcp.tools", section: "apps", title: "What a program can do", description: "Reading, writing and seeing the open file are approved separately for each program.", keywords: ["mcp", "tools", "read", "write", "rename", "create", "delete", "permission", "open file", "selection"], requires: PROGRAMS_PAGE, pages: PROGRAMS_PAGE },
  { id: "mcp.clients", section: "apps", title: "Programs", description: "Each program you approved, and what it may do.", keywords: ["mcp", "programs", "approved", "allowed", "clients", "permission", "read", "write", "open file", "selection", "ask first", "review", "apply", "discard", "revoke", "forget"], requires: PROGRAMS_PAGE, pages: PROGRAMS_PAGE },
  { id: "mcp.activity", section: "apps", title: "Program activity", description: "Every call a program made, newest first.", keywords: ["activity", "log", "record", "calls", "what happened", "audit", "programs", "mcp"], requires: PROGRAMS_PAGE, pages: PROGRAMS_PAGE },
  { id: "files.cli", section: "integrations", title: "Terminal command", description: "Opens files and folders in Writ from a terminal.", keywords: ["cli", "writ command", "terminal", "command line", "install"] },
  { id: "files.default_app", section: "integrations", title: "Open these file types with Writ", description: "A double-click on a file of a type you choose opens it in Writ.", keywords: ["default", "default app", "open with", "file association", "txt", "text", "log", "markdown", "md", "json", "yaml", "toml", "config", "data", "csv", "code", "rust", "typescript", "python"] },
  { id: "files.inbox_folder", section: "integrations", title: "Folder to watch for new files", description: "A file that appears in this folder opens in a new tab.", keywords: ["watch", "watched folder", "new files", "auto-open", "drop"] },
  { id: "files.inbox_focus", section: "integrations", title: "Bring Writ to the front when a new file arrives", description: "Off opens the file without taking you away from the app you are in.", keywords: ["watch", "focus", "window", "front", "new file"] },
  { id: "sidebar.inbox", section: "integrations", title: "Show watched folder", description: "Lists the watched folder's files in the sidebar.", keywords: ["sidebar", "watch", "watched folder", "new files", "show", "hide"] },
  { id: "shortcuts.edit", section: "shortcuts", title: "Keyboard shortcuts", description: "Click Record, then press the keys.", keywords: ["shortcut", "keyboard", "keybinding", "hotkey", "rebind"] },
  { id: "preview.layout_html", section: "advanced", title: "HTML opens in", description: "Text and preview puts the rendered page beside the code.", keywords: ["layout", "html", "text", "preview", "split"] },
  { id: "preview.live_threshold", section: "advanced", title: "Stop live preview above", description: "Above this size the preview updates only when you refresh it.", keywords: ["preview", "live", "size", "mb", "large files"] },
  { id: "preview.refuse_threshold", section: "advanced", title: "Do not preview files above", description: "Above this size a file is not previewed. It still opens as text.", keywords: ["preview", "limit", "size", "mb", "large files"] },
  { id: "storage.location", section: "advanced", title: "Writ's data folder", description: "Settings, the search index and versions are kept here. Your files stay in your folder.", keywords: ["database", "sqlite", "db", "writ.db", "index", "logs", "settings file", "app data"] },
  { id: "notes.versions", section: "advanced", title: "Versions", description: "Files over 2 MB are not versioned. File, Revert To… lists the versions of the open file.", keywords: ["versions", "version", "restore", "revert", "older", "previous", "keep", "retention", "days"] },
  { id: "about.version", section: "about", title: "Version", description: "What's new lists the changes in this version.", keywords: ["version", "release", "what's new", "changelog"] },
  { id: "updates.check_now", section: "about", title: "Updates", description: "Nothing installs until you choose Install.", keywords: ["update", "check", "now", "version", "up to date"] },
  { id: "updates.auto_check", section: "about", title: "Check for updates automatically", description: "Looks at most once a day while Writ is open.", keywords: ["update", "auto", "check", "version"] },
  { id: "about.docs", section: "about", title: "Writ help", description: "Opens the guide on the Writ site.", keywords: ["help", "docs", "documentation", "manual", "guide"] },
  { id: "about.licences", section: "about", title: "Third-party licenses", description: "Opens them in a read-only tab.", keywords: ["licences", "licenses", "notices", "open source", "copyright", "credits"] },
];

const ENTRIES: ReadonlyMap<string, SettingEntry> = new Map(
  SETTINGS_INDEX.map((entry) => [entry.id, entry]),
);

export function settingEntry(id: string): SettingEntry | undefined {
  return ENTRIES.get(id);
}

export function settingTitle(id: string): string {
  const entry = ENTRIES.get(id);
  if (!entry) throw new UnknownSettingError(id);
  return entry.title;
}

export function settingDescription(id: string): string {
  const entry = ENTRIES.get(id);
  if (!entry) throw new UnknownSettingError(id);
  return entry.description;
}

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
 * within the results and never adds or drops one. It is what puts `Folder`,
 * which claims `folder`, above `Folder to watch for new files`, which only
 * happens to start with the word.
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

export function rankSections(query: string): SettingsSection[] {
  const q = query.toLowerCase().trim();
  if (!q) return [];
  const starts = SECTION_ORDER.filter((id) => SECTION_LABELS[id].toLowerCase().startsWith(q));
  const contains = SECTION_ORDER.filter(
    (id) => !starts.includes(id) && SECTION_LABELS[id].toLowerCase().includes(q),
  );
  return [...starts, ...contains];
}

export function sectionHasMatch(section: SettingsSection, query: string): boolean {
  const q = query.toLowerCase().trim();
  if (!q) return true;
  return rankSettings(query).some((entry) => entry.section === section);
}
