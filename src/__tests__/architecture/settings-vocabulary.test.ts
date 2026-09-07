import { describe, it, expect } from "vitest";

import allowlist from "./banned-words.allowlist.json";
import {
  SECTION_LABELS,
  SECTION_ORDER,
  SETTINGS_INDEX,
  rankSettings,
} from "../../settings";

/**
 * The panel's words, pinned. A rename is a product decision, so it changes this
 * table in the same diff rather than passing quietly.
 */
const EXPECTED_ROWS: ReadonlyArray<[string, string, string]> = [
  ["notes.folder", "notes", "Notes folder"],
  ["editor.font_size", "editor", "Font size"],
  ["editor.tab_size", "editor", "Tab size"],
  ["editor.word_wrap", "editor", "Word wrap"],
  ["editor.markdown_typography", "editor", "Style headings and bold text as you type"],
  ["editor.markdown_editing", "editor", "Markdown shortcuts"],
  ["editor.spelling", "editor", "Spell check"],
  ["editor.spelling_dialect", "editor", "Spelling"],
  ["editor.status_bar", "editor", "Status bar"],
  ["files.default_app", "files", "Open these file types with Writ"],
  ["preview.run_scripts", "preview", "Allow HTML files to run their scripts"],
  ["preview.layout_md", "preview", "When opening a Markdown file, show:"],
  ["preview.layout_html", "preview", "When opening an HTML file, show:"],
  ["ai.enabled", "ai", "Rewrite selected text"],
  ["ai.preset", "ai", "Provider"],
  ["ai.base_url", "ai", "Base URL"],
  ["ai.model", "ai", "Model"],
  ["ai.api_key", "ai", "API key"],
  ["appearance.polarity", "appearance", "Light and dark"],
  ["appearance.accent", "appearance", "Accent color"],
  ["appearance.prose_face", "appearance", "Prose typeface"],
  ["appearance.interface_text_size", "appearance", "Interface text size"],
  ["appearance.theme", "appearance", "Theme"],
  ["appearance.custom_colors", "appearance", "Custom colors"],
  ["updates.auto_check", "updates", "Check for updates automatically"],
  ["updates.check_now", "updates", "Check for updates now"],
  ["shortcuts.edit", "shortcuts", "Keyboard shortcuts"],
  ["files.cli", "advanced", "Terminal command"],
  ["files.inbox_folder", "advanced", "Folder to watch for new files"],
  ["files.inbox_focus", "advanced", "Bring Writ to the front when a new file arrives"],
  ["preview.live_threshold", "advanced", "Stop live preview above"],
  ["preview.refuse_threshold", "advanced", "Do not preview files above"],
  ["storage.location", "advanced", "Writ's data folder"],
];

/**
 * Records the allowlist held at 8077719, the number it may only fall below. A
 * literal, not a `git show`: CI checks out a shallow tree with no `origin/main`.
 */
const ALLOWLIST_RECORDS_BEFORE = 35;

describe("settings vocabulary", () => {
  it("settings_rows_carry_the_pinned_label_and_section", () => {
    expect(SETTINGS_INDEX.map((e) => [e.id, e.section, e.title])).toEqual(
      EXPECTED_ROWS.map((row) => [...row]),
    );
  });

  it("settings_sections_are_named_and_ordered_as_pinned", () => {
    expect(SECTION_ORDER).toEqual([
      "notes",
      "editor",
      "files",
      "preview",
      "ai",
      "appearance",
      "updates",
      "shortcuts",
      "advanced",
    ]);
    expect(SECTION_ORDER.map((s) => SECTION_LABELS[s])).toEqual([
      "Notes",
      "Editor",
      "Files",
      "Preview",
      "AI rewriting",
      "Appearance",
      "Updates",
      "Shortcuts",
      "Advanced",
    ]);
  });

  // The audience's own words, so the panel answers the question as asked.
  it("settings_search_answers_the_audiences_words", () => {
    for (const term of ["notes", "folder", "where are my notes", "backup", "sync"]) {
      expect(rankSettings(term).length, term).toBeGreaterThan(0);
    }
  });

  it("where_are_my_notes_leads_with_the_notes_folder_row", () => {
    expect(rankSettings("where are my notes")[0]?.id).toBe("notes.folder");
  });

  // A `.db` path is never the answer to "where are my notes": the data folder
  // is a separate row, in Advanced, and its keywords must not compete.
  it("the_data_folder_row_does_not_outrank_the_notes_folder", () => {
    for (const term of ["notes", "where are my notes", "backup", "sync"]) {
      expect(rankSettings(term)[0]?.id, term).toBe("notes.folder");
    }
    const dataFolder = SETTINGS_INDEX.find((e) => e.id === "storage.location")!;
    expect(dataFolder.section).toBe("advanced");
    expect(dataFolder.title).not.toMatch(/\.db/);
  });

  it("banned_words_allowlist_is_shorter_than_before", () => {
    expect(allowlist.length).toBeLessThan(ALLOWLIST_RECORDS_BEFORE);
  });
});
