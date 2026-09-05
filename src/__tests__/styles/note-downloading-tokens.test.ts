import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

// `legacy-aliases.test.ts` is a blocklist: it catches the pre-ADR-030 names and
// nothing else, so a property that exists nowhere at all still passes it. The
// download pane and its tab are the newest stylesheet on this branch and were
// retokenised by hand, which is exactly the case a blocklist cannot see. This
// reads the other way round: every name the sheet asks for has to be one the
// generated theme declares.

const ROOT = process.cwd();

const SHEET = "src/components/Editor/NoteDownloading.css";

const THEME_CSS = readFileSync(resolve(ROOT, "src/styles/generated/theme.css"), "utf8");
const SHEET_CSS = readFileSync(resolve(ROOT, SHEET), "utf8");

function declaredNames(css: string): Set<string> {
  const names = new Set<string>();
  for (const [, name] of css.matchAll(/^\s*(--writ-[a-z0-9-]+)\s*:/gm)) names.add(name);
  return names;
}

function readNames(css: string): string[] {
  const names = new Set<string>();
  for (const [, name] of css.matchAll(/var\(\s*(--writ-[a-z0-9-]+)/g)) names.add(name);
  return [...names].sort();
}

describe("the download pane's stylesheet", () => {
  it("reads only properties the generated theme declares", () => {
    const declared = declaredNames(THEME_CSS);
    expect(declared.size).toBeGreaterThan(0);

    const missing = readNames(SHEET_CSS).filter((name) => !declared.has(name));
    expect(missing).toEqual([]);
  });

  it("reads a property for every value it paints", () => {
    // No literal colour, radius or step: the pane is a full-height surface in
    // the editor slot, so a hard-coded value would survive a theme change.
    const withoutVars = SHEET_CSS.replace(/var\([^)]*\)/g, "");
    expect(withoutVars).not.toMatch(/#[0-9a-fA-F]{3,8}\b/);
    expect(withoutVars).not.toMatch(/\brgba?\(/);

    // The names actually in use, so a silent swap back to a literal or onto a
    // property from another layer shows up here.
    expect(readNames(SHEET_CSS)).toEqual([
      "--writ-accent",
      "--writ-bg-canvas",
      "--writ-bg-raised",
      "--writ-border",
      "--writ-fg",
      "--writ-r-control",
      "--writ-space-2",
      "--writ-space-3",
      "--writ-space-4",
      "--writ-status-error",
      "--writ-ui-sm",
    ]);
  });
});
