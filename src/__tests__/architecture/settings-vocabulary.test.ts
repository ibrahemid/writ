import { describe, it, expect } from "vitest";

import allowlist from "./banned-words.allowlist.json";
import {
  SECTION_LABELS,
  SECTION_ORDER,
  SETTINGS_INDEX,
  rankSettings,
} from "../../settings";

/**
 * The panel's words, pinned: label, section and the terms each row claims. A
 * rename or a moved keyword is a product decision, so it changes this table in
 * the same diff rather than passing quietly.
 */
const EXPECTED_ROWS: ReadonlyArray<[string, string, string, string[]]> = [
  ["notes.folder", "files", "Folder", ["files", "folder", "where are my files", "location", "path", "finder", "backup", "sync", "icloud", "dropbox"]],
  ["files.default_extension", "files", "Default format", ["format", "default", "extension", "txt", "text", "plain text", "md", "markdown", "new file"]],
  ["files.default_app", "files", "Open these file types with Writ", ["default", "default app", "open with", "file association", "txt", "text", "log", "markdown", "md", "json", "yaml", "toml", "config", "data", "csv", "code", "rust", "typescript", "python"]],
  ["files.cli", "advanced", "Terminal command", ["cli", "writ command", "terminal", "command line", "install"]],
  ["notes.versions", "advanced", "Versions", ["versions", "version", "restore", "revert", "older", "previous", "keep", "retention", "days"]],
  ["editor.font_size", "editor", "Font size", ["font", "size", "text", "zoom"]],
  ["editor.tab_size", "editor", "Tab size", ["tab", "indent", "spaces", "width"]],
  ["editor.word_wrap", "editor", "Word wrap", ["wrap", "word", "line", "soft wrap"]],
  ["editor.markdown_editing", "editor", "Markdown shortcuts", ["markdown", "editing", "bold", "italic", "strikethrough", "link", "shortcuts", "checkbox", "list"]],
  ["editor.spelling", "editor", "Spell check", ["spell", "spelling", "check", "dictionary", "typos", "grammar"]],
  ["editor.spelling_dialect", "editor", "Spelling", ["spelling", "english", "us", "uk", "american", "british", "canadian", "australian"]],
  ["editor.status_bar", "editor", "Status bar", ["status bar", "line", "column", "encoding", "word count"]],
  ["editor.status_bar_counts", "editor", "Word, character and token counts", ["word count", "character count", "token", "counts", "status bar"]],
  ["preview.run_scripts", "advanced", "Allow HTML files to run their scripts", ["scripts", "javascript", "html", "run", "safety"]],
  ["preview.layout_md", "preview", "When opening a Markdown file, show:", ["layout", "markdown", "md", "text", "preview", "inline"]],
  ["preview.layout_html", "advanced", "When opening an HTML file, show:", ["layout", "html", "text", "preview", "split"]],
  ["ai.provider", "apps", "Provider", ["ai", "provider", "ollama", "lm studio", "anthropic", "openai", "gemini", "openrouter", "groq", "deepseek", "mistral", "xai", "together", "fireworks", "custom"]],
  ["ai.base_url", "apps", "Base URL", ["ai", "base url", "endpoint", "host", "server"]],
  ["ai.api_key", "apps", "API key", ["ai", "api key", "token", "secret", "credential"]],
  ["ai.model", "apps", "Model", ["ai", "model", "id"]],
  ["ai.connection", "apps", "Connection", ["ai", "connection", "check", "reachable", "status"]],
  ["ai.chat.enabled", "apps", "Chat", ["app", "ai", "chat", "ask", "conversation", "attach", "enable"]],
  ["ai.rewrite.enabled", "apps", "Rewrite", ["app", "ai", "rewrite", "proofread", "rephrase", "polish", "selection", "enable"]],
  ["ai.chat.model", "apps", "Use a different model for chat", ["ai", "chat", "model", "id", "different"]],
  ["mcp.enabled", "apps", "Connected programs", ["app", "mcp", "programs", "clients", "connect", "claude", "editor", "assistant", "tools", "server", "enable"]],
  ["mcp.command", "apps", "Command to give a program", ["mcp", "command", "copy", "paste", "configure", "setup", "stdio"]],
  ["mcp.tools", "apps", "What a program can do", ["mcp", "tools", "read", "write", "rename", "create", "delete", "permission"]],
  ["mcp.clients", "apps", "Programs you approved", ["mcp", "programs", "approved", "clients", "permission", "read", "write", "revoke", "forget"]],
  ["mcp.activity", "apps", "Recent activity", ["activity", "log", "record", "calls", "what happened", "audit"]],
  ["apps.connections", "apps", "Connections", ["app", "connections", "links", "backlinks", "outline", "properties", "panel"]],
  ["apps.graph", "apps", "Graph", ["app", "graph", "map", "links", "nearby"]],
  ["apps.tags", "apps", "Tags", ["app", "tags", "hashtag", "filter", "sidebar"]],
  ["apps.screen", "apps", "Setup screen", ["apps", "setup", "choose", "screen", "start"]],
  ["appearance.polarity", "appearance", "Light and dark", ["appearance", "light", "dark", "system", "theme", "polarity", "follow system"]],
  ["appearance.accent", "appearance", "Accent color", ["accent", "color", "pine", "highlight"]],
  ["appearance.prose_face", "appearance", "Prose typeface", ["font", "typeface", "prose", "writing", "ia writer", "quattro"]],
  ["appearance.interface_text_size", "appearance", "Interface text size", ["interface", "text", "size", "ui", "font", "scale", "bigger", "smaller", "sidebar", "tabs"]],
  ["appearance.theme", "appearance", "Theme", ["theme", "color", "appearance", "preset", "dark", "light"]],
  ["appearance.custom_colors", "appearance", "Custom colors", ["theme", "colors", "custom", "palette"]],
  ["sidebar.folder", "sidebar", "Show files", ["sidebar", "folder", "files", "tree", "show", "hide"]],
  ["sidebar.inbox", "advanced", "Show watched folder", ["sidebar", "watch", "watched folder", "new files", "show", "hide"]],
  ["sidebar.recent", "sidebar", "Show recently closed", ["sidebar", "recent", "recently closed", "closed", "show", "hide"]],
  ["updates.auto_check", "updates", "Check for updates automatically", ["update", "auto", "check", "version"]],
  ["updates.check_now", "updates", "Check for updates now", ["update", "check", "now", "version"]],
  ["shortcuts.edit", "shortcuts", "Keyboard shortcuts", ["shortcut", "keyboard", "keybinding", "hotkey", "rebind"]],
  ["files.inbox_folder", "advanced", "Folder to watch for new files", ["watch", "watched folder", "new files", "auto-open", "drop"]],
  ["files.inbox_focus", "advanced", "Bring Writ to the front when a new file arrives", ["watch", "focus", "window", "front", "new file"]],
  ["preview.live_threshold", "advanced", "Stop live preview above", ["preview", "live", "size", "mb", "large files"]],
  ["preview.refuse_threshold", "advanced", "Do not preview files above", ["preview", "limit", "size", "mb", "large files"]],
  ["storage.location", "advanced", "Writ's data folder", ["database", "sqlite", "db", "writ.db", "index", "logs", "settings file", "app data"]],
];

/**
 * Records the allowlist held at 8077719, the number it may only fall below. A
 * literal, not a `git show`: CI checks out a shallow tree with no `origin/main`.
 */
const ALLOWLIST_RECORDS_BEFORE = 35;

describe("settings vocabulary", () => {
  it("settings_rows_carry_the_pinned_label_section_and_keywords", () => {
    expect(SETTINGS_INDEX.map((e) => [e.id, e.section, e.title, e.keywords])).toEqual(
      EXPECTED_ROWS.map((row) => [...row]),
    );
  });

  // One connection serves rewriting and chat, so its rows are named for the
  // connection, not after one of them (ADR-040 section 1), and are drawn while
  // either is on (ADR-042 section 4).
  it("names the connection rows after what they hold, not after one feature", () => {
    expect(SECTION_LABELS.apps).toBe("Apps");
    const connection = SETTINGS_INDEX.filter((e) => e.id.startsWith("ai.") && e.requires);
    expect(connection.map((e) => e.id)).toEqual(expect.arrayContaining(["ai.provider", "ai.model"]));
    for (const entry of connection) {
      expect(entry.section, entry.id).toBe("apps");
      expect(entry.keywords, entry.id).toContain("ai");
    }
    for (const entry of connection.filter((e) => e.id !== "ai.chat.model")) {
      expect(entry.requires, entry.id).toEqual(["chat", "rewrite"]);
    }
  });

  // The provider row is the one a person reaches for by naming their service.
  it("the provider row claims every provider name", () => {
    for (const name of ["ollama", "lm studio", "anthropic", "openai", "openrouter", "fireworks"]) {
      expect(rankSettings(name)[0]?.id, name).toBe("ai.provider");
    }
  });

  it("settings_sections_are_named_and_ordered_as_pinned", () => {
    expect(SECTION_ORDER).toEqual([
      "files",
      "apps",
      "editor",
      "preview",
      "appearance",
      "sidebar",
      "updates",
      "shortcuts",
      "advanced",
    ]);
    expect(SECTION_ORDER.map((s) => SECTION_LABELS[s])).toEqual([
      "Files",
      "Apps",
      "Editor",
      "Preview",
      "Appearance",
      "Sidebar",
      "Updates",
      "Shortcuts",
      "Advanced",
    ]);
  });

  // The audience's own words, so the panel answers the question as asked.
  it("settings_search_answers_the_audiences_words", () => {
    for (const term of ["files", "folder", "where are my files", "backup", "sync"]) {
      expect(rankSettings(term).length, term).toBeGreaterThan(0);
    }
  });

  it("where_are_my_files_leads_with_the_folder_row", () => {
    expect(rankSettings("where are my files")[0]?.id).toBe("notes.folder");
  });

  // `folder` is the term the panel and the command palette used to disagree on:
  // the palette lists rank order, so the watched-folder row led there.
  it("folder_leads_with_the_folder_row_not_the_watched_folder", () => {
    const ranked = rankSettings("folder").map((e) => e.id);
    expect(ranked[0]).toBe("notes.folder");
    expect(ranked).toContain("files.inbox_folder");
    expect(ranked.indexOf("notes.folder")).toBeLessThan(ranked.indexOf("files.inbox_folder"));
  });

  // The exact-keyword tier reorders results; it must never change which rows
  // match, since a keyword equal to the query also contains it.
  it("claiming_a_term_exactly_reorders_results_without_changing_the_set", () => {
    for (const term of ["files", "folder", "backup", "sync", "theme", "font", "markdown"]) {
      const byScore = new Set(rankSettings(term).map((e) => e.id));
      const byContains = SETTINGS_INDEX.filter(
        (e) =>
          e.title.toLowerCase().includes(term) ||
          e.keywords.some((k) => k.toLowerCase().includes(term)) ||
          SECTION_LABELS[e.section].toLowerCase().includes(term),
      ).map((e) => e.id);
      expect([...byScore].sort(), term).toEqual([...byContains].sort());
    }
  });

  // A `.db` path is never the answer to "where are my files": the data folder
  // is a separate row, in Advanced, and its keywords must not compete.
  it("the_data_folder_row_does_not_outrank_the_folder_row", () => {
    for (const term of ["files", "folder", "where are my files", "backup", "sync"]) {
      expect(rankSettings(term)[0]?.id, term).toBe("notes.folder");
    }
    const dataFolder = SETTINGS_INDEX.find((e) => e.id === "storage.location")!;
    expect(dataFolder.section).toBe("advanced");
    expect(dataFolder.title).toBe("Writ's data folder");
    expect(dataFolder.title).not.toMatch(/\.db/);
    for (const term of ["files", "folder", "backup", "sync"]) {
      expect(dataFolder.keywords, term).not.toContain(term);
    }
  });

  // The watched-folder section is "inbox" in config and nowhere in the panel's
  // words, so the id alone must not answer a search for it.
  it("inbox_surfaces_no_sidebar_row", () => {
    expect(rankSettings("inbox").filter((e) => e.section === "sidebar")).toEqual([]);
    expect(rankSettings("watched folder").map((e) => e.id)).toContain("sidebar.inbox");
  });

  it("banned_words_allowlist_is_shorter_than_before", () => {
    expect(allowlist.length).toBeLessThan(ALLOWLIST_RECORDS_BEFORE);
  });
});
