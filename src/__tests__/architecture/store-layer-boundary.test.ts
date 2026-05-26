import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync, existsSync } from "node:fs";
import { join, relative, resolve } from "node:path";

const REPO_ROOT = process.cwd();
const SRC = resolve(REPO_ROOT, "src");
const STORES = resolve(SRC, "stores");
const STORES_GLOBAL = resolve(STORES, "global");
const STORES_WINDOW = resolve(STORES, "window");
const COMPONENTS = resolve(SRC, "components");
const WINDOW_PROVIDER = resolve(COMPONENTS, "WindowProvider");

// Match runtime imports only. Type-only imports (`import type {...}`) carry no
// runtime cost; the architectural boundary applies to runtime coupling.
// The `^` with the `m` flag anchors at line start, which skips matches that
// would otherwise start inside comments containing the word "import".
const IMPORT_RE = /^import\s+(?!type[\s{])(?:[\s\S]*?)\s+from\s+["']([^"']+)["']/gm;

function walk(dir: string, files: string[] = []): string[] {
  if (!existsSync(dir)) return files;
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    const st = statSync(full);
    if (st.isDirectory()) {
      if (entry === "node_modules" || entry === "__tests__" || entry === "dist") continue;
      walk(full, files);
    } else if ((entry.endsWith(".ts") || entry.endsWith(".tsx")) && !entry.endsWith(".d.ts")) {
      files.push(full);
    }
  }
  return files;
}

function resolveSpecifier(fromFile: string, spec: string): string | null {
  if (!spec.startsWith(".")) return null;
  return resolve(fromFile, "..", spec);
}

function extractImports(file: string): string[] {
  const text = readFileSync(file, "utf8");
  const out: string[] = [];
  for (const m of text.matchAll(IMPORT_RE)) out.push(m[1]);
  return out;
}

function isUnder(target: string, parent: string): boolean {
  const rel = relative(parent, target);
  return !!rel && !rel.startsWith("..") && !resolve(parent, rel).startsWith("..");
}

describe("store layer boundary", () => {
  it("stores/global/ does not import from stores/window/", () => {
    const files = walk(STORES_GLOBAL);
    const offenders: { file: string; spec: string }[] = [];
    for (const file of files) {
      for (const spec of extractImports(file)) {
        const r = resolveSpecifier(file, spec);
        if (r && isUnder(r, STORES_WINDOW)) {
          offenders.push({ file: relative(REPO_ROOT, file), spec });
        }
      }
    }
    expect(
      offenders,
      `global stores must not depend on window stores: ${offenders
        .map((o) => `${o.file} -> ${o.spec}`)
        .join("; ")}`,
    ).toEqual([]);
  });

  it("every store lives under global/ or window/ — no stray files directly under stores/", () => {
    // Post-migration invariant: the two-tier split is exhaustive. A `.ts`
    // file directly under stores/ is a store that escaped classification —
    // the exact global-vs-window ambiguity ADR-009 E3's split exists to
    // forbid. The only permitted direct children of stores/ are the
    // `global` and `window` directories.
    const strays = readdirSync(STORES)
      .filter((entry) => {
        const full = join(STORES, entry);
        return (
          statSync(full).isFile() && (entry.endsWith(".ts") || entry.endsWith(".tsx"))
        );
      })
      .map((entry) => relative(REPO_ROOT, join(STORES, entry)));
    expect(
      strays,
      `stores must live under global/ or window/, not directly under stores/: ${strays.join(", ")}`,
    ).toEqual([]);
  });

  it("components/ do not import directly from stores/window/ (must go through useWindow())", () => {
    const files = walk(COMPONENTS);
    const offenders: { file: string; spec: string }[] = [];
    for (const file of files) {
      // WindowProvider itself owns the factories — it's the only legal direct consumer.
      if (isUnder(file, WINDOW_PROVIDER)) continue;
      for (const spec of extractImports(file)) {
        const r = resolveSpecifier(file, spec);
        if (r && isUnder(r, STORES_WINDOW)) {
          offenders.push({ file: relative(REPO_ROOT, file), spec });
        }
      }
    }
    expect(
      offenders,
      `components must use useWindow() instead of importing window stores directly: ${offenders
        .map((o) => `${o.file} -> ${o.spec}`)
        .join("; ")}`,
    ).toEqual([]);
  });

  it("commands/ do not import directly from stores/window/ (must resolve via windowRegistry)", () => {
    const COMMANDS = resolve(SRC, "commands");
    const files = walk(COMMANDS);
    const offenders: { file: string; spec: string }[] = [];
    for (const file of files) {
      for (const spec of extractImports(file)) {
        const r = resolveSpecifier(file, spec);
        if (r && isUnder(r, STORES_WINDOW)) {
          offenders.push({ file: relative(REPO_ROOT, file), spec });
        }
      }
    }
    expect(
      offenders,
      `commands must resolve window state via stores/global/window-registry: ${offenders
        .map((o) => `${o.file} -> ${o.spec}`)
        .join("; ")}`,
    ).toEqual([]);
  });
});
