import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";

// Colours come from design/tokens through the generated sheet. A literal in an
// authored file is a fork of the palette, so it fails here.

const REPO_ROOT = process.cwd();
const SRC = resolve(REPO_ROOT, "src");
const SKIP_DIRS = [
  resolve(SRC, "__tests__"),
  resolve(SRC, "styles/generated"),
  resolve(SRC, "styles/themes"),
];

/** Files still carrying a literal colour. Empty: nothing may be added to it. */
export const LITERAL_COLOR_ALLOWLIST: readonly string[] = [];

const LITERAL_COLOR = /#[0-9a-fA-F]{3,8}\b|\b(?:rgba?|hsla?)\s*\(/g;

function walk(dir: string, files: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      if (entry === "node_modules" || SKIP_DIRS.includes(full)) continue;
      walk(full, files);
    } else if (/\.(css|ts|tsx)$/.test(entry) && !entry.endsWith(".d.ts")) {
      files.push(full);
    }
  }
  return files;
}

// Issue references (#124) live in comments, so comments come out before the
// scan rather than being special-cased in the pattern.
function withoutComments(text: string): string {
  return text
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .split("\n")
    .map((line) => line.replace(/\/\/.*$/, ""))
    .join("\n");
}

describe("no literal colour in authored source", () => {
  it("every colour resolves through a --writ-* token", () => {
    const offenders: string[] = [];
    for (const file of walk(SRC)) {
      const rel = relative(REPO_ROOT, file);
      if (LITERAL_COLOR_ALLOWLIST.includes(rel)) continue;
      const matches = withoutComments(readFileSync(file, "utf8")).match(LITERAL_COLOR);
      if (matches) offenders.push(`${rel} -> ${matches.join(", ")}`);
    }
    expect(offenders, `literal colours found:\n${offenders.join("\n")}`).toEqual([]);
  });

  it("allowlist entries all still exist", () => {
    for (const rel of LITERAL_COLOR_ALLOWLIST) {
      expect(() => statSync(resolve(REPO_ROOT, rel)), `${rel} is gone`).not.toThrow();
    }
  });
});
