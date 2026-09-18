import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

// The ring follows the host: focus.css moves --writ-focus-outline and
// --writ-focus-offset per shell, and the chrome either spends those tokens or
// says nothing and lets the global :focus-visible rule draw. A control that is
// deliberately silent says so on the element, through data-writ-focus-silent,
// so one decision has one mechanism.

const read = (path: string) => readFileSync(resolve(process.cwd(), path), "utf8");

/** The declaration body of one rule, matched on its own selector line. */
function ruleBody(css: string, selector: string): string | null {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return css.match(new RegExp(`^${escaped}\\s*\\{([^}]*)\\}`, "m"))?.[1] ?? null;
}

/** The body of the one rule whose selector list is exactly `selectors`. */
function groupedRuleBody(css: string, selectors: readonly string[]): string | null {
  for (const [, list, body] of css.matchAll(/([^{}]+)\{([^}]*)\}/g)) {
    const named = list
      .replace(/\/\*[\s\S]*?\*\//g, "")
      .split(",")
      .map((one) => one.trim())
      .filter(Boolean);
    if (named.length === selectors.length && named.every((one, i) => one === selectors[i])) {
      return body;
    }
  }
  return null;
}

describe("the focus ring in the chrome", () => {
  // `.winctrl` is the one exception focus.css names: Fluent's second ring
  // cannot be a token, because it is drawn inside a control that runs to the
  // window edge.
  it("is silenced on the element wherever a control is deliberately quiet", () => {
    expect(read("src/components/ContextMenu/ContextMenu.tsx")).toMatch(/data-writ-focus-silent/);
    expect(read("src/components/Graph/FolderGraphView.tsx")).toMatch(/data-writ-focus-silent/);
    expect(read("src/components/Sidebar/SearchBar.tsx")).toMatch(/data-writ-focus-silent/);
  });

  // A ring drawn outside a full-height separator brackets the pane beside it,
  // and one outside a heading that fills the sidebar is clipped by it. Both
  // edges of the window say the same thing.
  it("draws inside the controls that cannot hold an outward ring", () => {
    const SIDEBAR = read("src/components/Sidebar/Sidebar.css");
    const PANEL = read("src/components/RightPanel/RightPanel.css");
    for (const [css, selector, offset] of [
      [SIDEBAR, ".sidebar-resizer:focus-visible", "-2px"],
      [SIDEBAR, ".sidebar-section-toggle:focus-visible", "-2px"],
      [SIDEBAR, ".sidebar-section-action:focus-visible", "-1px"],
      [PANEL, ".right-panel-resizer:focus-visible", "-2px"],
      [PANEL, ".right-panel-row:focus-visible", "var\\(--writ-focus-offset-inset[,)]"],
    ] as const) {
      const body = ruleBody(css, selector);
      expect(body, `${selector} is declared`).toBeTruthy();
      expect(body!).toMatch(/outline:\s*var\(--writ-focus-outline[,)]/);
      expect(body!).toMatch(new RegExp(`outline-offset:\\s*${offset}`));
    }
  });

  // A row lives in a scroller that clips: an outward ring is cut flat on the
  // first and last visible rows and laps the neighbour's fill in between. One
  // token says how far inward, and it is the same on all three shells.
  it("draws a row's ring inside the row, from one token", () => {
    const FOCUS = read("src/styles/focus.css");
    expect(ruleBody(FOCUS, ":root")).toMatch(/--writ-focus-offset-inset:\s*-2px/);
    for (const root of [':root[data-platform="win"]', ':root[data-platform="linux"]']) {
      expect(ruleBody(FOCUS, root), root).not.toMatch(/--writ-focus-offset-inset/);
    }

    const TABBAR = read("src/components/Editor/TabBar.css");
    for (const [css, selectors] of [
      [read("src/components/Sidebar/Sidebar.css"), [".sidebar-row:focus-visible"]],
      [TABBAR, [".tab-label:focus-visible", ".tab-close:focus-visible", ".tab-add:focus-visible"]],
    ] as const) {
      const body = groupedRuleBody(css, selectors);
      expect(body, `${selectors.join(", ")} is declared`).toBeTruthy();
      expect(body!).toMatch(/outline:\s*var\(--writ-focus-outline[,)]/);
      expect(body!).toMatch(/outline-offset:\s*var\(--writ-focus-offset-inset[,)]/);
    }

    // The rows are not left on the outward offset by the shared list either.
    for (const name of [".tab-label", ".tab-close", ".tab-add", ".tab-item"]) {
      expect(FOCUS, `${name} still takes the outward offset`).not.toMatch(
        new RegExp(`\\${name}:focus-visible`),
      );
    }
  });

  it("keeps the background that stands in for the ring on a menu item", () => {
    expect(read("src/components/ContextMenu/ContextMenu.css")).toMatch(
      /\.context-menu-item:focus-visible\s*\{[^}]*background:\s*var\(--writ-bg-hover\)/,
    );
  });
});

describe("the search field's focus state", () => {
  const SEARCH = read("src/components/Sidebar/SearchBar.css");

  it("keeps the macOS halo", () => {
    expect(SEARCH).toMatch(
      /^\.search-field:focus-within\s*\{[^}]*box-shadow:[^}]*var\(--writ-accent\)/m,
    );
  });

  it("is a bottom accent edge on Windows and no glow", () => {
    const win = /:root\[data-platform="win"\] \.search-field:focus-within\s*\{([^}]*)\}/.exec(
      SEARCH,
    );
    expect(win, "the Windows half is declared").toBeTruthy();
    expect(win![1]).toMatch(/border-bottom:\s*2px solid var\(--writ-accent\)/);
    expect(win![1]).toMatch(/box-shadow:\s*none/);
  });

  it("is the platform ring on GNOME", () => {
    const linux = /:root\[data-platform="linux"\] \.search-field:focus-within\s*\{([^}]*)\}/.exec(
      SEARCH,
    );
    expect(linux, "the GNOME half is declared").toBeTruthy();
    expect(linux![1]).toMatch(/outline:\s*var\(--writ-focus-outline[,)]/);
    expect(linux![1]).toMatch(/outline-offset:\s*var\(--writ-focus-offset[,)]/);
  });
});
