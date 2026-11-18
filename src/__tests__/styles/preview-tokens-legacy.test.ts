import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { LEGACY_CSS_PREFIX } from "../../lib/legacy-names";

// Previewed documents can read the preview properties by the names an earlier
// build used, so the preview layer declares each old name as an alias of the
// current one (ADR-059 decision 5).

const CSS = readFileSync(
  resolve(process.cwd(), "src-tauri/assets/generated/preview-tokens.css"),
  "utf8",
);
const CURRENT = "--jotra-preview-";
const LEGACY = `${LEGACY_CSS_PREFIX}preview-`;

function declaredNames(prefix: string): Set<string> {
  const names = new Set<string>();
  for (const match of CSS.matchAll(/^\s*(--[A-Za-z0-9-]+)\s*:/gm)) {
    if (match[1].startsWith(prefix)) names.add(match[1]);
  }
  return names;
}

function aliases(): Map<string, string> {
  const out = new Map<string, string>();
  const pattern = /^\s*(--[A-Za-z0-9-]+)\s*:\s*var\((--[A-Za-z0-9-]+)\)\s*;/gm;
  for (const match of CSS.matchAll(pattern)) {
    if (match[1].startsWith(LEGACY)) out.set(match[1], match[2]);
  }
  return out;
}

describe("the preview layer's aliases for the old property names", () => {
  it("cover every current preview property", () => {
    const current = declaredNames(CURRENT);
    expect(current.size).toBeGreaterThan(0);
    const aliased = aliases();
    for (const name of current) {
      const old = LEGACY + name.slice(CURRENT.length);
      expect(aliased.get(old), `${old} is not declared`).toBe(name);
    }
  });

  it("name nothing the layer does not declare", () => {
    const current = declaredNames(CURRENT);
    for (const [old, target] of aliases()) {
      expect(current.has(target), `${old} points at an undeclared ${target}`).toBe(true);
      expect(target.slice(CURRENT.length)).toBe(old.slice(LEGACY.length));
    }
  });

  it("are declared only as aliases", () => {
    expect(declaredNames(LEGACY).size).toBe(aliases().size);
  });

  it("come after the dark block, in their own root block", () => {
    const dark = CSS.indexOf('[data-jotra-theme="dark"] {');
    const firstAlias = CSS.indexOf(LEGACY);
    expect(dark).toBeGreaterThan(-1);
    expect(firstAlias).toBeGreaterThan(dark);
    expect(CSS.lastIndexOf(":root {")).toBeGreaterThan(dark);
  });
});
