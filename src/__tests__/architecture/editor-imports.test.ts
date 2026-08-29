import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";

// CodeMirror is the editor's own dependency. Chrome around the editor —
// the toolbar, the tab strip, the palette — reaches it through a command or a
// store, never by importing the library, so the editor stays swappable and the
// chrome stays testable without a running view.

const REPO_ROOT = process.cwd();
const COMPONENTS = resolve(REPO_ROOT, "src/components");
const EDITOR_DIR = resolve(COMPONENTS, "Editor");

const EDITOR_LIBRARY = /from\s+["'](@codemirror\/[^"']+|@lezer\/[^"']+)["']/g;

function walk(dir: string, files: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      if (entry === "node_modules" || entry === "__tests__") continue;
      walk(full, files);
    } else if (/\.tsx?$/.test(entry) && !entry.endsWith(".d.ts")) {
      files.push(full);
    }
  }
  return files;
}

describe("only the editor component imports the editor library", () => {
  it("no other component reaches for @codemirror or @lezer", () => {
    const offenders: string[] = [];
    for (const file of walk(COMPONENTS)) {
      if (file.startsWith(EDITOR_DIR + "/")) continue;
      const matches = readFileSync(file, "utf8").match(EDITOR_LIBRARY);
      if (matches) offenders.push(`${relative(REPO_ROOT, file)} -> ${matches.join(", ")}`);
    }
    expect(offenders, `editor library imported outside Editor/:\n${offenders.join("\n")}`).toEqual(
      [],
    );
  });

  it("the editor component is where it does live", () => {
    const inEditor = walk(EDITOR_DIR).filter(
      (file) => readFileSync(file, "utf8").match(EDITOR_LIBRARY) !== null,
    );
    expect(inEditor.length).toBeGreaterThan(0);
  });
});
