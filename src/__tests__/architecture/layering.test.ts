import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";

const REPO_ROOT = process.cwd();
const SRC = resolve(REPO_ROOT, "src");
const SERVICES_DIR = resolve(SRC, "services");
const COMPONENTS_DIR = resolve(SRC, "components");
const EDITOR_DIR = resolve(SRC, "editor");
const STORES_DIR = resolve(SRC, "stores");

const IMPORT_RE = /import\s+(?:[\s\S]*?)\s+from\s+["']([^"']+)["']/g;

// The text of every `onEvent("<kind>", …)` callback in `text`, taken from the
// call to the balanced close of its argument list.
function handlerBodies(text: string, kind: string): string[] {
  const bodies: string[] = [];
  const needle = `onEvent("${kind}"`;
  let at = text.indexOf(needle);
  while (at !== -1) {
    let depth = 0;
    let i = text.indexOf("(", at);
    const start = i;
    for (; i < text.length; i += 1) {
      if (text[i] === "(") depth += 1;
      else if (text[i] === ")") {
        depth -= 1;
        if (depth === 0) break;
      }
    }
    bodies.push(text.slice(start, i + 1));
    at = text.indexOf(needle, i);
  }
  return bodies;
}

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

  // Components call stores; stores call services. A component may not import
  // any services/* module directly. Modules that are a deliberate exception go
  // in the allowlist (as a path relative to src/services/, without extension).
  const COMPONENT_SERVICES_ALLOWLIST: string[] = [];

  it("no file under src/components/ imports from src/services/ outside the allowlist", () => {
    const files = walk(COMPONENTS_DIR);
    const offenders: { file: string; spec: string }[] = [];
    for (const file of files) {
      for (const spec of extractImports(file)) {
        const resolved = resolveSpecifier(file, spec);
        if (!resolved) continue;
        if (resolved !== SERVICES_DIR && !resolved.startsWith(SERVICES_DIR + "/")) continue;
        const rel = relative(SERVICES_DIR, resolved).replace(/\.(ts|tsx)$/, "");
        if (COMPONENT_SERVICES_ALLOWLIST.includes(rel)) continue;
        offenders.push({ file: relative(REPO_ROOT, file), spec });
      }
    }
    expect(
      offenders,
      `components must go through stores (allowlist: [${COMPONENT_SERVICES_ALLOWLIST.join(", ")}]): ${offenders
        .map((o) => `${o.file} -> ${o.spec}`)
        .join("; ")}`,
    ).toEqual([]);
  });

  // The editor layer is CodeMirror wiring and pure decisions over a document.
  // It takes what it needs to reach the app through injected interfaces —
  // LinkDeps, WikilinkDeps, NoteLinkActions — so the same code answers to a
  // test with a plain object. A module reaching a store or a component from
  // here is the layer skipping the wiring it is supposed to be given.
  //
  // A service is a different question: clipboard-commands.ts calls one
  // directly, so services are allowed here and stores are not.
  it("no file under src/editor/ imports from src/stores/ or src/components/", () => {
    const offenders: { file: string; spec: string }[] = [];
    for (const file of walk(EDITOR_DIR)) {
      for (const spec of extractImports(file)) {
        const resolved = resolveSpecifier(file, spec);
        if (!resolved) continue;
        const reaches = (dir: string) => resolved === dir || resolved.startsWith(dir + "/");
        if (!reaches(STORES_DIR) && !reaches(COMPONENTS_DIR)) continue;
        offenders.push({ file: relative(REPO_ROOT, file), spec });
      }
    }
    expect(
      offenders,
      `the editor layer takes what it needs through injected dependencies: ${offenders
        .map((o) => `${o.file} -> ${o.spec}`)
        .join("; ")}`,
    ).toEqual([]);
  });

  // A notes:changed handler patches the notes tree and the index and nothing
  // else. Reloading the document registry from a watcher event recreates a
  // loaded writ-preview:// iframe, and removing a loaded one hard-freezes the
  // macOS webview (PR #127).
  it("no notes:changed handler reaches the buffer registry", () => {
    const offenders: string[] = [];
    for (const file of walk(SRC)) {
      const text = readFileSync(file, "utf8");
      if (!text.includes("notes:changed")) continue;
      for (const block of handlerBodies(text, "notes:changed")) {
        if (/bufferRegistry|handleExternalEdit/.test(block)) {
          offenders.push(relative(REPO_ROOT, file));
        }
      }
    }
    expect(offenders).toEqual([]);
  });
});
