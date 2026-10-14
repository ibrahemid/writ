import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve, sep } from "node:path";

import { SECTION_LABELS, SETTINGS_INDEX } from "../../settings";

const REPO_ROOT = process.cwd();

function repoPath(path: string): string {
  return relative(REPO_ROOT, path).split(sep).join("/");
}

const ROOTS = ["src", "site/src", "docs", "README.md", "crates", "src-tauri/src"];
const EXTENSIONS = /\.(ts|tsx|astro|md|rs)$/;
const SKIPPED_DIRS = new Set(["node_modules", "target", "dist", "generated", "__tests__", "tests"]);
const SKIPPED_PATHS = ["docs/adr"];

// The shipped release records, mirrored from CHANGELOG.md, as in
// copy-spelling.test.ts: each names the panel as it was in that version.
const ALLOWED = ["site/src/data/changelog.ts"];

const SEP = String.raw`\s*(?:,|&rarr;|→)\s*`;
// The OS's own Settings app is skipped.
const PATH = new RegExp(
  String.raw`(?<!(?:System|Windows) )\bSettings(?:\s*\([^)]*\))?${SEP}([A-Z][a-z]+)\b(?:${SEP}([^\n]{0,120}))?`,
  "g",
);

const SECTIONS = new Set<string>(Object.values(SECTION_LABELS));

const ROW_SECTIONS = new Map<string, Set<string>>();
for (const entry of SETTINGS_INDEX) {
  const sections = ROW_SECTIONS.get(entry.title) ?? new Set<string>();
  sections.add(SECTION_LABELS[entry.section]);
  ROW_SECTIONS.set(entry.title, sections);
}
const TITLES_LONGEST_FIRST = [...ROW_SECTIONS.keys()].sort((a, b) => b.length - a.length);

function stripComments(source: string): string {
  return source
    .replace(/\/\*[\s\S]*?\*\//g, " ")
    .replace(/<!--[\s\S]*?-->/g, " ")
    .replace(/(^|[^:\\])\/\/[^\n]*/g, "$1");
}

function walk(path: string, files: string[] = []): string[] {
  if (SKIPPED_PATHS.includes(repoPath(path))) return files;
  if (!statSync(path).isDirectory()) {
    if (EXTENSIONS.test(path) && !path.endsWith(".d.ts")) files.push(path);
    return files;
  }
  for (const entry of readdirSync(path)) {
    if (SKIPPED_DIRS.has(entry)) continue;
    walk(join(path, entry), files);
  }
  return files;
}

function rowNamedAt(rest: string): string | undefined {
  return TITLES_LONGEST_FIRST.find(
    (title) => rest.startsWith(title) && !/[A-Za-z]/.test(rest.charAt(title.length)),
  );
}

/** Every "Settings, Section, Row" a reader is sent to that the panel does not have. */
function stalePaths(rel: string, source: string): string[] {
  const text = /\.(md|astro)$/.test(rel) ? source.replace(/<!--[\s\S]*?-->/g, " ") : stripComments(source);
  const found: string[] = [];
  for (const match of text.matchAll(PATH)) {
    const [whole, section, rest] = match;
    const line = text.slice(0, match.index).split("\n").length;
    if (!SECTIONS.has(section)) {
      found.push(`${rel}:${line} names no section "${section}": ${whole.slice(0, 80)}`);
      continue;
    }
    const row = rest === undefined ? undefined : rowNamedAt(rest);
    if (row && !ROW_SECTIONS.get(row)?.has(section)) {
      const actual = [...(ROW_SECTIONS.get(row) ?? [])].join(" or ");
      found.push(`${rel}:${line} puts "${row}" under ${section}, it is under ${actual}`);
    }
  }
  return found;
}

describe("Settings paths in shipped text", () => {
  it("the allowlist holds only the shipped release records", () => {
    expect(ALLOWED).toEqual(["site/src/data/changelog.ts"]);
  });

  it("flags a removed section and a row under the wrong section", () => {
    expect(stalePaths("a.md", "Open Settings, Files, Folder.")).toHaveLength(1);
    expect(stalePaths("a.astro", "<code>Settings &rarr; Updates</code>")).toHaveLength(1);
    expect(stalePaths("a.astro", 'Open Settings (<kbd>⌘</kbd><kbd>,</kbd>), Files.')).toHaveLength(1);
    expect(stalePaths("a.md", "Install it from Settings, Advanced, Terminal command.")).toEqual([
      'a.md:1 puts "Terminal command" under Advanced, it is under Integrations',
    ]);
    expect(stalePaths("a.md", "Install it from Settings, Integrations, Terminal command.")).toEqual([]);
    expect(stalePaths("a.md", "Pick it in Settings, Integrations, Folder to watch for new files.")).toEqual([]);
    expect(stalePaths("a.md", "Turn it on in System Settings, Login Items.")).toEqual([]);
    expect(stalePaths("a.ts", "// Settings, Files moved\nconst x = 1;")).toEqual([]);
  });

  it("every Settings path names a section the panel has, and a row where it lives", () => {
    const offenders = ROOTS.flatMap((root) => walk(resolve(REPO_ROOT, root)))
      .map((file) => [repoPath(file), file] as const)
      .filter(([rel]) => !ALLOWED.includes(rel))
      .flatMap(([rel, file]) => stalePaths(rel, readFileSync(file, "utf8")));
    expect(offenders, `stale Settings paths:\n${offenders.join("\n")}`).toEqual([]);
  });
});
