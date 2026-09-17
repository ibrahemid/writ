import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { resolve, join } from "node:path";

// The ring follows the host: focus.css moves --writ-focus-outline and
// --writ-focus-offset per shell, and the chrome either spends those tokens or
// says nothing and lets the global :focus-visible rule draw. A control that is
// deliberately silent says so on the element, through data-writ-focus-silent,
// so one decision has one mechanism.

const read = (path: string) => readFileSync(resolve(process.cwd(), path), "utf8");

const AREAS = [
  "src/components/Sidebar",
  "src/components/TitleBar",
  "src/components/Toolbar",
  "src/components/Resizer",
  "src/components/ContextMenu",
  "src/components/RightPanel",
  "src/components/Graph",
];

function stylesheets(): string[] {
  const files: string[] = [];
  for (const area of AREAS) {
    for (const name of readdirSync(resolve(process.cwd(), area))) {
      if (name.endsWith(".css")) files.push(join(area, name));
    }
  }
  files.push("src/components/Editor/TabBar.css");
  return files;
}

describe("the focus ring in the chrome", () => {
  // `.winctrl` is the one exception focus.css names: Fluent's second ring
  // cannot be a token, because it is drawn inside a control that runs to the
  // window edge.
  it("is never hand-written from the accent", () => {
    for (const file of stylesheets()) {
      expect(read(file), `${file} draws its own ring`).not.toMatch(
        /outline:\s*\d+px solid var\(--writ-accent\)/,
      );
    }
  });

  it("is never cancelled in CSS", () => {
    for (const file of stylesheets()) {
      expect(read(file), `${file} cancels the ring in CSS`).not.toMatch(/outline:\s*none/);
    }
  });

  it("is silenced on the element wherever a control is deliberately quiet", () => {
    expect(read("src/components/ContextMenu/ContextMenu.tsx")).toMatch(/data-writ-focus-silent/);
    expect(read("src/components/Graph/FolderGraphView.tsx")).toMatch(/data-writ-focus-silent/);
    expect(read("src/components/Sidebar/SearchBar.tsx")).toMatch(/data-writ-focus-silent/);
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
