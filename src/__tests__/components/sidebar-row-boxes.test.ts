import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, it, expect } from "vitest";

// Rows and headings across the sidebar share one box, so a hover fill in one
// section lines up with the next. Pinned from the 2026-09 completeness pass:
// the watched-folder rows ran edge to edge, and the day headings under
// Recent sat left of the section name once it gained a caret.

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
    expect(rule(css("src/components/Sidebar/Sidebar.css"), ".sidebar-row")).toMatch(
      /margin:\s*1px var\(--writ-space-2-5\)/,
    );
    expect(inbox).not.toMatch(/margin:/);
    expect(inbox).not.toMatch(/width:\s*100%/);
  });

  it("puts the section caret on the tree's caret column, 16px in, on every shell", () => {
    const sidebar = css("src/components/Sidebar/Sidebar.css");
    expect(rule(sidebar, ".sidebar-section-toggle")).toMatch(
      /padding:\s*var\(--writ-space-4\) var\(--writ-space-4\) var\(--writ-space-2\) var\(--writ-space-5\)/,
    );
    expect(sidebar).toMatch(
      /:root\[data-platform="win"\] \.sidebar-section-title,\s*:root\[data-platform="win"\] \.sidebar-section-toggle\s*\{\s*padding:\s*var\(--writ-space-4-5\) var\(--writ-space-5\) var\(--writ-space-2-5\);/,
    );
    expect(sidebar).toMatch(
      /:root\[data-platform="linux"\] \.sidebar-section-title,\s*:root\[data-platform="linux"\] \.sidebar-section-toggle\s*\{\s*padding:\s*var\(--writ-space-3-5\) var\(--writ-space-4-5\) var\(--writ-space-2\) var\(--writ-space-5\);/,
    );
  });

  it("lines the day headings up with the section name past the caret", () => {
    const history = css("src/components/Sidebar/HistorySection.css");
    expect(rule(history, ".history-group-title")).toMatch(
      /padding:\s*var\(--writ-space-4\) var\(--writ-space-4\) var\(--writ-space-2\) 34px/,
    );
    expect(rule(history, ':root[data-platform="win"] .history-group-title')).toMatch(
      /padding:\s*var\(--writ-space-4-5\) var\(--writ-space-5\) var\(--writ-space-2-5\) 34px/,
    );
    expect(rule(history, ':root[data-platform="linux"] .history-group-title')).toMatch(
      /padding:\s*var\(--writ-space-3-5\) var\(--writ-space-4-5\) var\(--writ-space-2\) 34px/,
    );
  });

  it("keeps closed-note and watched-folder rows on the tree's icon column", () => {
    const history = css("src/components/Sidebar/HistorySection.css");
    expect(rule(history, ".history-list .tab-item")).toMatch(/padding-left:\s*30px/);
    expect(rule(history, ':root[data-platform="win"] .history-list .tab-item')).toMatch(
      /padding-left:\s*20px/,
    );
    expect(rule(css("src/components/Sidebar/InboxSection.css"), ".inbox-item")).toMatch(
      /padding:\s*0 var\(--writ-space-3-5\) 0 30px/,
    );
  });

  it("ellipsizes the search field rather than cutting a word", () => {
    const search = css("src/components/Sidebar/SearchBar.css");
    expect(rule(search, ".search-input")).toMatch(/text-overflow:\s*ellipsis/);
    expect(rule(search, ".search-input::placeholder")).toMatch(/text-overflow:\s*ellipsis/);
  });
});
