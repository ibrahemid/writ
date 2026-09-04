import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";

// House style is en-US in every string a user can read: "Custom colors",
// "Switch presets or override individual colors live", "every color editable in
// Settings". An en-GB spelling reached the appearance panel and the theme
// editor once, which split the settings search between two spellings of the
// same word. Comments are stripped before the match, so a file may still say
// "colour" in its own prose.
const EN_GB =
  /\b(colour(s|ed|ing)?|centre(s|d|ing)?|customis(e|es|ed|ing|ation)|favourite(s)?)\b/gi;

const REPO_ROOT = process.cwd();

// The app and the site. `.css` is excluded because a stylesheet carries no
// copy, and `__tests__` because a test names the string it pins.
const ROOTS = ["src", "site/src"];
const EXTENSIONS = /\.(ts|tsx|astro)$/;

// The shipped release records, mirrored from CHANGELOG.md. Their text is what
// each version announced and is not rewritten after the fact. Nothing else may
// be added here.
const ALLOWED = ["site/src/data/changelog.ts"];

function stripComments(source: string): string {
  return source
    .replace(/\/\*[\s\S]*?\*\//g, " ")
    .replace(/<!--[\s\S]*?-->/g, " ")
    .replace(/(^|[^:\\])\/\/[^\n]*/g, "$1");
}

function walk(dir: string, files: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      if (entry === "node_modules" || entry === "__tests__" || entry === "generated") continue;
      walk(full, files);
    } else if (EXTENSIONS.test(entry) && !entry.endsWith(".d.ts")) {
      files.push(full);
    }
  }
  return files;
}

const SOURCES = ROOTS.flatMap((root) => walk(resolve(REPO_ROOT, root)));

describe("user-visible copy is en-US", () => {
  it("the allowlist holds only the shipped release records", () => {
    expect(ALLOWED).toEqual(["site/src/data/changelog.ts"]);
  });

  it("no source file spells a user-visible string en-GB", () => {
    const offenders: string[] = [];
    for (const file of SOURCES) {
      const rel = relative(REPO_ROOT, file);
      if (ALLOWED.includes(rel)) continue;
      const matches = stripComments(readFileSync(file, "utf8")).match(EN_GB);
      if (matches) {
        offenders.push(`${rel} -> ${[...new Set(matches)].sort().join(", ")}`);
      }
    }
    expect(offenders, `en-GB spellings found:\n${offenders.join("\n")}`).toEqual([]);
  });
});
