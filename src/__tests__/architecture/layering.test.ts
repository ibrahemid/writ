import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";

const REPO_ROOT = process.cwd();
const SRC = resolve(REPO_ROOT, "src");
const SERVICES_DIR = resolve(SRC, "services");
const COMPONENTS_DIR = resolve(SRC, "components");

const IMPORT_RE = /import\s+(?:[\s\S]*?)\s+from\s+["']([^"']+)["']/g;

function walk(dir: string, files: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    const st = statSync(full);
    if (st.isDirectory()) {
      if (entry === "node_modules" || entry === "__tests__" || entry === "dist") continue;
      walk(full, files);
    } else if (entry.endsWith(".ts") || entry.endsWith(".tsx")) {
      if (entry.endsWith(".d.ts")) continue;
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
  for (const m of text.matchAll(IMPORT_RE)) {
    out.push(m[1]);
  }
  return out;
}

describe("frontend layering", () => {
  it("no file under src/services/ imports from src/stores/", () => {
    const files = walk(SERVICES_DIR);
    const offenders: { file: string; spec: string }[] = [];
    for (const file of files) {
      for (const spec of extractImports(file)) {
        const resolved = resolveSpecifier(file, spec);
        if (resolved && resolved.startsWith(SRC + "/stores/")) {
          offenders.push({ file: relative(REPO_ROOT, file), spec });
        }
      }
    }
    expect(
      offenders,
      `services must not import from stores: ${offenders
        .map((o) => `${o.file} -> ${o.spec}`)
        .join("; ")}`,
    ).toEqual([]);
  });

  it("no file under src/components/ imports from src/services/tauri", () => {
    const files = walk(COMPONENTS_DIR);
    const tauriPath = resolve(SRC, "services/tauri");
    const offenders: { file: string; spec: string }[] = [];
    for (const file of files) {
      for (const spec of extractImports(file)) {
        const resolved = resolveSpecifier(file, spec);
        if (!resolved) continue;
        const normalized = resolved.replace(/\.(ts|tsx)$/, "");
        if (normalized === tauriPath) {
          offenders.push({ file: relative(REPO_ROOT, file), spec });
        }
      }
    }
    expect(
      offenders,
      `components must go through stores: ${offenders
        .map((o) => `${o.file} -> ${o.spec}`)
        .join("; ")}`,
    ).toEqual([]);
  });
});
