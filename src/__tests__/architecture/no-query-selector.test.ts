import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";

// Components and stores reach the DOM through refs and store-managed state.
// A lookup by selector reads a tree Solid owns, so it fails here.

const REPO_ROOT = process.cwd();
const SCANNED_DIRS = [resolve(REPO_ROOT, "src/components"), resolve(REPO_ROOT, "src/stores")];

const DOCUMENT_LOOKUP = /document\.(?:querySelectorAll|querySelector|getElementById)\s*\(/g;

function walk(dir: string, files: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      if (entry === "node_modules" || entry === "__tests__") continue;
      walk(full, files);
    } else if (/\.(ts|tsx)$/.test(entry) && !entry.endsWith(".d.ts")) {
      files.push(full);
    }
  }
  return files;
}

function withoutComments(text: string): string {
  return text
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .split("\n")
    .map((line) => line.replace(/\/\/.*$/, ""))
    .join("\n");
}

/** Every document lookup in `text`, comments excluded. */
export function documentLookups(text: string): string[] {
  return withoutComments(text).match(DOCUMENT_LOOKUP) ?? [];
}

describe("no document lookup in components or stores", () => {
  it("every element comes from a ref or the store", () => {
    const offenders: string[] = [];
    for (const dir of SCANNED_DIRS) {
      for (const file of walk(dir)) {
        const matches = documentLookups(readFileSync(file, "utf8"));
        if (matches.length > 0) {
          offenders.push(`${relative(REPO_ROOT, file)} -> ${matches.join(", ")}`);
        }
      }
    }
    expect(offenders, `document lookups found:\n${offenders.join("\n")}`).toEqual([]);
  });

  it("catches a lookup written into a component", () => {
    const source = [
      "export function Panel() {",
      '  const node = document.querySelector(".writ-panel");',
      '  const list = document.querySelectorAll(".writ-row");',
      '  const byId = document.getElementById("writ-root");',
      "  return [node, list, byId];",
      "}",
    ].join("\n");
    expect(documentLookups(source)).toHaveLength(3);
  });

  it("reads the directories the rule names", () => {
    for (const dir of SCANNED_DIRS) {
      expect(statSync(dir).isDirectory(), `${dir} is gone`).toBe(true);
      expect(walk(dir).length, `${dir} has no files to scan`).toBeGreaterThan(0);
    }
  });
});
