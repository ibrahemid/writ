import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, it, expect } from "vitest";

// Rows and headings across the sidebar share one box, so a hover fill in one
// section lines up with the next. Pinned from the 2026-09 completeness pass:
// the watched-folder rows ran edge to edge, and the day headings under
// Recently closed sat left of the section name once it gained a caret.

function css(file: string): string {
  return readFileSync(resolve(process.cwd(), file), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
}

function rule(text: string, selector: string): string {
  const match = new RegExp(`${selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\s*\\{([^}]*)\\}`).exec(text);
  if (!match) throw new Error(`no rule for ${selector}`);
  return match[1];
}

describe("sidebar row boxes", () => {
  it("gives a watched-folder row the same margin as every other row", () => {
    const inbox = rule(css("src/components/Sidebar/InboxSection.css"), ".inbox-item");
    const tab = rule(css("src/components/Sidebar/TabItem.css"), ".tab-item");
    expect(inbox).toMatch(/margin:\s*1px 6px/);
    expect(tab).toMatch(/margin:\s*1px 6px/);
    expect(inbox).not.toMatch(/width:\s*100%/);
  });

  it("lines the day headings up with the section name past the caret", () => {
    const history = css("src/components/Sidebar/HistorySection.css");
    expect(rule(history, ".history-group-title")).toMatch(/padding:\s*12px 12px 4px 30px/);
    expect(rule(history, ':root[data-platform="win"] .history-group-title')).toMatch(
      /padding:\s*14px 16px 6px 34px/,
    );
    expect(rule(history, ':root[data-platform="linux"] .history-group-title')).toMatch(
      /padding:\s*10px 14px 4px 32px/,
    );
  });

  it("ellipsizes the search field rather than cutting a word", () => {
    const search = css("src/components/Sidebar/SearchBar.css");
    expect(rule(search, ".search-input")).toMatch(/text-overflow:\s*ellipsis/);
    expect(rule(search, ".search-input::placeholder")).toMatch(/text-overflow:\s*ellipsis/);
  });

  it("sheds the status bar chips before the layout control is clipped", () => {
    const bar = css("src/components/Editor/StatusBar.css");
    expect(bar).toMatch(
      /@container \(max-width: 32em\)\s*\{\s*\.spelling-chip,\s*\.scripts-toggle\s*\{\s*display:\s*none;/,
    );
  });
});
