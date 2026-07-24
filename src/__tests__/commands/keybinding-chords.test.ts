import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";

const REPO_ROOT = process.cwd();
const SRC = resolve(REPO_ROOT, "src");

const KEYBINDING_RE = /keybinding:\s*"([^"]+)"/g;
const ALIASES_RE = /keybindingAliases:\s*\[([^\]]*)\]/g;
const ALIAS_LITERAL_RE = /"([^"]+)"/g;

function walk(dir: string, files: string[] = []): string[] {
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

// Every default chord declared as a literal anywhere in src/. Aliases count:
// they are dispatched exactly like a primary binding.
function declaredChords(): Map<string, string[]> {
  const owners = new Map<string, string[]>();
  for (const file of walk(SRC)) {
    const text = readFileSync(file, "utf8");
    const chords: string[] = [];
    for (const m of text.matchAll(KEYBINDING_RE)) chords.push(m[1]);
    for (const m of text.matchAll(ALIASES_RE)) {
      for (const a of m[1].matchAll(ALIAS_LITERAL_RE)) chords.push(a[1]);
    }
    for (const chord of chords) {
      const list = owners.get(chord) ?? [];
      list.push(relative(REPO_ROOT, file));
      owners.set(chord, list);
    }
  }
  return owners;
}

describe("default keybindings", () => {
  it("no chord is claimed twice across the shipped command tables", () => {
    const duplicates = [...declaredChords()].filter(([, owners]) => owners.length > 1);
    expect(
      duplicates.map(([chord, owners]) => `${chord} in ${owners.join(", ")}`),
      "a chord claimed by two commands is dispatched to whichever registered last",
    ).toEqual([]);
  });

  it("CmdOrCtrl+Shift+F is claimed by search.openEverywhere alone", () => {
    const owners = declaredChords().get("CmdOrCtrl+Shift+F");
    expect(owners).toEqual(["src/App.tsx"]);
    const app = readFileSync(resolve(SRC, "App.tsx"), "utf8");
    expect(app).toMatch(/id:\s*"search\.openEverywhere"[\s\S]{0,200}"CmdOrCtrl\+Shift\+F"/);
  });
});
