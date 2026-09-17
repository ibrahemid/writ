import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

// Every list in the sidebar draws the same row: one pitch, one margin, one
// radius. The metrics live on a single base class so a platform block moves
// the tree, tags, tabs, the inbox and search results together rather than
// leaving four of them on the macOS box while the fifth follows the host.

const read = (path: string) => readFileSync(resolve(process.cwd(), path), "utf8");

const SIDEBAR = read("src/components/Sidebar/Sidebar.css");

const ROWS: [string, string][] = [
  ["src/components/Sidebar/TabItem.tsx", "tab-item"],
  ["src/components/Sidebar/FileTree.tsx", "file-tree-item"],
  ["src/components/Sidebar/TagsSection.tsx", "tags-row"],
  ["src/components/Sidebar/InboxSection.tsx", "inbox-item"],
  ["src/components/Sidebar/SearchResults.tsx", "search-result"],
];

function rule(css: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = css.match(new RegExp(`^${escaped}\\s*\\{([^}]*)\\}`, "m"));
  expect(match, `${selector} is declared`).toBeTruthy();
  return match![1];
}

describe("the shared sidebar row", () => {
  it("carries the pitch, the margin and the radius on one class", () => {
    const base = rule(SIDEBAR, ".sidebar-row");
    expect(base).toMatch(/min-height:\s*var\(--writ-sidebar-row-fill\)/);
    expect(base).toMatch(/margin:\s*1px 6px/);
    expect(base).toMatch(/border-radius:\s*var\(--writ-r-row\)/);
    expect(base).toMatch(/position:\s*relative/);
  });

  it("moves every row with one block per platform", () => {
    expect(rule(SIDEBAR, ':root[data-platform="win"] .sidebar-row')).toMatch(
      /margin:\s*var\(--writ-space-1\) var\(--writ-space-2\)/,
    );
    const linux = rule(SIDEBAR, ':root[data-platform="linux"] .sidebar-row');
    expect(linux).toMatch(/margin:\s*0 6px var\(--writ-space-1\)/);
    expect(linux).toMatch(/column-gap:\s*10px/);
  });

  it("is worn by every list row in the sidebar", () => {
    for (const [path, name] of ROWS) {
      expect(read(path), `${name} wears .sidebar-row`).toMatch(
        new RegExp(`class="sidebar-row ${name}"`),
      );
    }
  });

  it("leaves the row gap to the row, so a two-line result keeps its own", () => {
    expect(rule(SIDEBAR, ".sidebar-row")).not.toMatch(/(^|[\s;])gap:/);
    expect(rule(read("src/components/Sidebar/SearchResults.css"), ".search-result")).toMatch(
      /row-gap:\s*1px/,
    );
  });
});

describe("the Windows selection bar", () => {
  it("marks the selected row on its leading edge, on Windows alone", () => {
    const bar = rule(SIDEBAR, ':root[data-platform="win"] .sidebar-row.is-selected::before');
    expect(bar).toMatch(/width:\s*3px/);
    expect(bar).toMatch(/height:\s*16px/);
    expect(bar).toMatch(/border-radius:\s*var\(--writ-r-indicator\)/);
    expect(bar).toMatch(/background:\s*var\(--writ-accent\)/);
    expect(SIDEBAR).not.toMatch(/^\.sidebar-row\.is-selected::before/m);
  });

  it("names the selected row once, so the bar reaches a search result too", () => {
    expect(read("src/components/Sidebar/SearchResults.tsx")).toMatch(/"is-selected":/);
    expect(read("src/components/Sidebar/SearchResults.css")).not.toMatch(/is-active/);
  });
});
