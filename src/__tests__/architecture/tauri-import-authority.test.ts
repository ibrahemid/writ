import { describe, it, expect } from "vitest";
import { mkdtempSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, relative, resolve } from "node:path";

const REPO_ROOT = process.cwd();
const SRC = resolve(REPO_ROOT, "src");

// The one file allowed to import each specifier, as a path under the source
// root. CLAUDE.md states both rules; this test is what enforces them.
const API_AUTHORITY = "services/tauri.ts";
const EVENT_AUTHORITY = "services/events.ts";

const API_PACKAGE = "@tauri-apps/api";
const EVENT_MODULE = "@tauri-apps/api/event";

// Static imports, side-effect imports, dynamic imports, requires, and the two
// re-export forms. A dynamic import of Tauri from a component is the same
// violation as a static one, and CLAUDE.md names it separately because it is
// the one that reads as a loophole; a re-export is the same skip written the
// other way round, since it hands the API to every file downstream.
const SPECIFIER_RE =
  /(?:import\s+(?:[\s\S]*?)\s+from\s*|export\s+(?:[\s\S]*?)\s+from\s*|export\s*\*\s*from\s*|import\s*|import\s*\(\s*|require\s*\(\s*)["']([^"']+)["']/g;

type Offender = { file: string; spec: string; authority: string };

function walk(dir: string, files: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      if (entry === "node_modules" || entry === "__tests__" || entry === "dist") continue;
      walk(full, files);
      continue;
    }
    if (!entry.endsWith(".ts") && !entry.endsWith(".tsx")) continue;
    if (entry.endsWith(".d.ts")) continue;
    files.push(full);
  }
  return files;
}

// Every file under `root` that imports a Tauri module it is not the authority
// for. Takes the root so the same matcher runs over a fixture tree.
function tauriImportOffenders(root: string): Offender[] {
  const offenders: Offender[] = [];
  const seen = new Set<string>();
  for (const file of walk(root)) {
    const here = relative(root, file);
    const text = readFileSync(file, "utf8");
    for (const match of text.matchAll(SPECIFIER_RE)) {
      const spec = match[1];
      if (spec !== API_PACKAGE && !spec.startsWith(API_PACKAGE + "/")) continue;
      const authority = spec === EVENT_MODULE ? EVENT_AUTHORITY : API_AUTHORITY;
      if (here === authority) continue;
      // Two forms can match the same line; one file naming one module is one
      // violation either way.
      if (!seen.add(`${here}\u0000${spec}`)) continue;
      offenders.push({ file: here, spec, authority });
    }
  }
  return offenders;
}

function describeOffenders(offenders: Offender[]): string {
  return offenders.map((o) => `${o.file} -> ${o.spec} (only ${o.authority} may)`).join("; ");
}

describe("tauri import authority", () => {
  it("only src/services/tauri.ts imports @tauri-apps/api", () => {
    const offenders = tauriImportOffenders(SRC).filter((o) => o.spec !== EVENT_MODULE);
    expect(offenders, describeOffenders(offenders)).toEqual([]);
  });

  it("only src/services/events.ts imports @tauri-apps/api/event", () => {
    const offenders = tauriImportOffenders(SRC).filter((o) => o.spec === EVENT_MODULE);
    expect(offenders, describeOffenders(offenders)).toEqual([]);
  });

  // The rule is only enforced if breaking it fails. Asserted on a fixture tree,
  // so the check runs against a planted violation without a real file changing.
  it("fails on a component that reaches Tauri", () => {
    const root = mkdtempSync(join(tmpdir(), "writ-tauri-authority-"));
    try {
      mkdirSync(join(root, "services"), { recursive: true });
      mkdirSync(join(root, "components", "Widget"), { recursive: true });
      writeFileSync(
        join(root, "services", "tauri.ts"),
        `import { invoke } from "${API_PACKAGE}/core";\nexport const call = invoke;\n`,
      );
      writeFileSync(
        join(root, "services", "events.ts"),
        `import { listen } from "${EVENT_MODULE}";\nexport const on = listen;\n`,
      );
      expect(tauriImportOffenders(root)).toEqual([]);

      writeFileSync(
        join(root, "components", "Widget", "Widget.tsx"),
        `import { invoke } from "${API_PACKAGE}/core";\nexport const Widget = () => invoke("noop");\n`,
      );
      writeFileSync(
        join(root, "components", "Widget", "Late.tsx"),
        `export const late = async () => (await import("${EVENT_MODULE}")).listen;\n`,
      );
      // A component that re-exports the API hands it to every other component,
      // which is the same layer skip written the other way round.
      writeFileSync(
        join(root, "components", "Widget", "Passthrough.tsx"),
        `export { invoke } from "${API_PACKAGE}/core";\n`,
      );
      writeFileSync(
        join(root, "components", "Widget", "Star.tsx"),
        `export * from "${EVENT_MODULE}";\n`,
      );

      const offenders = tauriImportOffenders(root);
      expect(offenders.map((o) => o.file).sort()).toEqual([
        join("components", "Widget", "Late.tsx"),
        join("components", "Widget", "Passthrough.tsx"),
        join("components", "Widget", "Star.tsx"),
        join("components", "Widget", "Widget.tsx"),
      ]);
      expect(offenders.find((o) => o.file.endsWith("Late.tsx"))?.authority).toBe(EVENT_AUTHORITY);
      expect(offenders.find((o) => o.file.endsWith("Star.tsx"))?.authority).toBe(EVENT_AUTHORITY);
      expect(offenders.find((o) => o.file.endsWith("Passthrough.tsx"))?.authority).toBe(
        API_AUTHORITY,
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
});
