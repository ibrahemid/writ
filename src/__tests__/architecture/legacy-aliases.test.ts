import { describe, it, expect } from "vitest";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";

// The pre-ADR-030 property names. `src/styles/generated/legacy-aliases.css`
// forwarded each one onto its replacement while the retokenisation ran; the
// sheet and its DTCG source are gone, so a file reading one of these names now
// resolves to nothing. Three names from the alias block are absent here because
// the generated sheet declares them itself: `--writ-overlay-scrim`,
// `--writ-selection` and `--writ-statusbar-height`.
export const RETIRED_TOKEN_NAMES: readonly string[] = [
  "--writ-accent-default",
  "--writ-accent-foreground",
  "--writ-bg-tab-pill",
  "--writ-border-default",
  "--writ-border-focus",
  "--writ-border-pill",
  "--writ-font-sans",
  "--writ-font-size",
  "--writ-font-size-sm",
  "--writ-font-size-xs",
  "--writ-foreground-default",
  "--writ-foreground-muted",
  "--writ-foreground-subtle",
  "--writ-line-height",
  "--writ-overlay-hover",
  "--writ-overlay-subtle",
  "--writ-radius-1",
  "--writ-radius-2",
  "--writ-radius-3",
  "--writ-shadow-banner",
  "--writ-shadow-dialog",
  "--writ-shadow-overlay",
  "--writ-shadow-sidebar",
  "--writ-shadow-toast",
  "--writ-shadow-xs",
  "--writ-surface-background",
  "--writ-surface-elevated",
  "--writ-surface-hover",
  "--writ-surface-input",
  "--writ-surface-raised",
  "--writ-surface-sunken",
  "--writ-tab-pill-height",
  "--writ-tabbar-height",
  "--writ-titlebar-height",
  "--writ-warning-foreground",
  "--writ-window-radius",
];

const REPO_ROOT = process.cwd();

// The shipping surfaces: the app, the site's demo window, the preview assets
// and the boot document. `src/__tests__` is excluded because two fixtures and
// the resolved-token ledger record origin/main's names on purpose.
const ROOTS = ["src", "site/src", "src-tauri/assets", "src-tauri/src"];
const SKIP_DIRS = [resolve(REPO_ROOT, "src/__tests__")];
const EXTENSIONS = /\.(css|ts|tsx|rs|html)$/;

function walk(dir: string, files: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      if (entry === "node_modules" || SKIP_DIRS.includes(full)) continue;
      walk(full, files);
    } else if (EXTENSIONS.test(entry) && !entry.endsWith(".d.ts")) {
      files.push(full);
    }
  }
  return files;
}

const SOURCES = [
  ...ROOTS.flatMap((root) => walk(resolve(REPO_ROOT, root))),
  resolve(REPO_ROOT, "index.html"),
];

// A retired name is only a match on its own boundary: `--writ-border-default`
// must not be reported for a file that reads `--writ-border-default-x`, and
// `--writ-font-size` must not be reported for `--writ-font-size-sm`.
const RETIRED = new RegExp(`(${RETIRED_TOKEN_NAMES.join("|")})(?![a-z0-9-])`, "g");

describe("the legacy alias layer is gone", () => {
  it("the alias sheet and its DTCG source no longer exist", () => {
    for (const path of ["src/styles/generated/legacy-aliases.css", "design/tokens/legacy.json"]) {
      expect(existsSync(resolve(REPO_ROOT, path)), `${path} still exists`).toBe(false);
    }
  });

  it("global.css imports no alias sheet", () => {
    const css = readFileSync(resolve(REPO_ROOT, "src/styles/global.css"), "utf8");
    expect(css).not.toContain("legacy-aliases");
  });

  it("no source file references a retired token name", () => {
    const offenders: string[] = [];
    for (const file of SOURCES) {
      const matches = readFileSync(file, "utf8").match(RETIRED);
      if (matches) {
        const names = [...new Set(matches)].sort().join(", ");
        offenders.push(`${relative(REPO_ROOT, file)} -> ${names}`);
      }
    }
    expect(offenders, `retired token names found:\n${offenders.join("\n")}`).toEqual([]);
  });

  it("the generated sheet declares no retired token name", () => {
    const css = readFileSync(resolve(REPO_ROOT, "src/styles/generated/theme.css"), "utf8");
    const declared = [...css.matchAll(/^\s*(--[a-z0-9-]+)\s*:/gm)].map((m) => m[1]);
    expect(declared.filter((name) => RETIRED_TOKEN_NAMES.includes(name))).toEqual([]);
  });
});
