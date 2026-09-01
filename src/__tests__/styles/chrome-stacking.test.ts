import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

// The find panel must outrank the word count (--writ-z-chrome) so its close
// button stays clickable, and the compose label must never wrap at a narrow
// window width.

const FIND_CSS = readFileSync(
  resolve(process.cwd(), "src/components/Find/FindOverlay.css"),
  "utf8",
);
const TOOLBAR_CSS = readFileSync(
  resolve(process.cwd(), "src/components/Toolbar/Toolbar.css"),
  "utf8",
);

const block = (css: string, selector: string): string => {
  const m = css.match(new RegExp(`${selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\s*\\{([^}]*)\\}`));
  expect(m, `${selector} is declared`).toBeTruthy();
  return m![1]!;
};

describe("find panel stacking", () => {
  it("reads the popover z-index token, not a raw number", () => {
    expect(block(FIND_CSS, ".find-overlay")).toMatch(/z-index:\s*var\(--writ-z-popover\)/);
  });

  it("keeps the tickmap on the same popover layer", () => {
    expect(block(FIND_CSS, ".find-tickmap")).toMatch(/z-index:\s*var\(--writ-z-popover\)/);
  });

  it("never hardcodes a z-index number", () => {
    expect(FIND_CSS).not.toMatch(/z-index:\s*\d/);
  });
});

describe("toolbar compose label", () => {
  it("never wraps to a second line", () => {
    const compose = block(TOOLBAR_CSS, ".writ-toolbar-compose");
    expect(compose).toMatch(/white-space:\s*nowrap/);
    expect(compose).toMatch(/flex:\s*none/);
  });

  it("lets the search field shrink before the compose label does", () => {
    const searchBlock = block(TOOLBAR_CSS, ".writ-toolbar .search-bar");
    expect(searchBlock).toMatch(/flex:\s*0 1 180px/);
    expect(searchBlock).toMatch(/min-width:\s*120px/);
  });
});
