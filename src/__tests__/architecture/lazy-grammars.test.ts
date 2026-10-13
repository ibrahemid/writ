import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";

// A grammar loads the first time a file needs it (src/editor/languages.ts), so
// the main chunk carries none and a plain-text file never waits for one. A
// static import of a grammar package anywhere in the app pulls it back into
// the main chunk; only a dynamic `import()` keeps it in its own.

const REPO_ROOT = process.cwd();
const SRC = resolve(REPO_ROOT, "src");

const STATIC_GRAMMAR_IMPORT =
  /(?:^|\n)\s*(?:import|export)\s[^;]*?from\s+["'](@codemirror\/lang-[^"']+|@codemirror\/legacy-modes[^"']*)["']|(?:^|\n)\s*import\s+["'](@codemirror\/lang-[^"']+)["']/g;

function walk(dir: string, files: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      if (entry === "node_modules" || entry === "__tests__") continue;
      walk(full, files);
    } else if (/\.tsx?$/.test(entry) && !entry.endsWith(".d.ts") && !/\.test\.tsx?$/.test(entry)) {
      files.push(full);
    }
  }
  return files;
}

function staticGrammarImports(text: string): string[] {
  return [...text.matchAll(STATIC_GRAMMAR_IMPORT)].map((match) => match[1] ?? match[2]);
}

describe("grammars load on demand", () => {
  it("no app file imports a grammar package statically", () => {
    const offenders: string[] = [];
    for (const file of walk(SRC)) {
      const found = staticGrammarImports(readFileSync(file, "utf8"));
      if (found.length > 0) offenders.push(`${relative(REPO_ROOT, file)} -> ${found.join(", ")}`);
    }
    expect(offenders, `static grammar imports:\n${offenders.join("\n")}`).toEqual([]);
  });

  it("tells a static import from a dynamic one", () => {
    expect(staticGrammarImports('import { python } from "@codemirror/lang-python";')).toEqual([
      "@codemirror/lang-python",
    ]);
    expect(staticGrammarImports('import "@codemirror/lang-css";')).toEqual(["@codemirror/lang-css"]);
    expect(
      staticGrammarImports('const m = await import("@codemirror/lang-markdown");'),
    ).toEqual([]);
  });
});
