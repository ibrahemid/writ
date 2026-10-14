import { describe, it, expect } from "vitest";
import { existsSync, readFileSync, statSync } from "node:fs";
import { relative, resolve } from "node:path";

// The capture window is its own realm (ADR-055): the main window's singletons
// must never load there, or a second copy of a store written for one window
// would run beside the first.

const REPO_ROOT = process.cwd();
const SRC = resolve(REPO_ROOT, "src");
const ENTRY = resolve(SRC, "capture/main.tsx");

// Runtime imports only; `import type` carries nothing into the bundle.
const IMPORT_RE = /^import\s+(?!type[\s{])(?:[\s\S]*?\s+from\s+)?["']([^"']+)["']/gm;
const EXTENSIONS = ["", ".ts", ".tsx", "/index.ts", "/index.tsx"];

function resolveImport(fromFile: string, spec: string): string | null {
  if (!spec.startsWith(".")) return null;
  const base = resolve(fromFile, "..", spec);
  for (const extension of EXTENSIONS) {
    const candidate = base + extension;
    if (existsSync(candidate) && statSync(candidate).isFile()) return candidate;
  }
  return null;
}

function reachable(entry: string): Set<string> {
  const seen = new Set<string>();
  const queue = [entry];
  while (queue.length > 0) {
    const file = queue.pop()!;
    if (seen.has(file)) continue;
    seen.add(file);
    if (!/\.tsx?$/.test(file)) continue;
    for (const match of readFileSync(file, "utf8").matchAll(IMPORT_RE)) {
      const target = resolveImport(file, match[1]!);
      if (target) queue.push(target);
    }
  }
  return seen;
}

const MAIN_WINDOW_ONLY = [
  "stores/global/",
  "stores/window/",
  "commands/",
  "editor/",
  "App.tsx",
  "main.tsx",
];

describe("the capture window's entry", () => {
  const files = [...reachable(ENTRY)].map((file) => relative(SRC, file));

  it("reaches the note, its store and the services", () => {
    expect(files).toContain("components/CaptureNote/CaptureNote.tsx");
    expect(files).toContain("stores/capture/capture-note.ts");
    expect(files).toContain("services/tauri.ts");
  });

  it("loads none of the main window's modules", () => {
    const offenders = files.filter(
      (file) =>
        MAIN_WINDOW_ONLY.some((prefix) => file.startsWith(prefix)) ||
        (file.startsWith("components/") && !file.startsWith("components/CaptureNote/")),
    );
    expect(offenders, `the capture entry imports:\n${offenders.join("\n")}`).toEqual([]);
  });

  it("loads no module that keeps single-window state", () => {
    const offenders = files.filter(
      (file) =>
        /\.tsx?$/.test(file) &&
        readFileSync(resolve(SRC, file), "utf8").includes("Writ is single-window"),
    );
    expect(offenders).toEqual([]);
  });
});
