import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";

const REPO_ROOT = process.cwd();
const SRC = resolve(REPO_ROOT, "src");
const THEME_CSS = resolve(SRC, "styles/theme.css");
const GLOBAL_CSS = resolve(SRC, "styles/global.css");

const MONO_ALLOWED = new Set<string>([
  resolve(SRC, "components/Sidebar/TabItem.css"),
  resolve(SRC, "components/Editor/cm-theme.ts"),
]);

const FONT_FAMILY_CSS_RE = /font-family\s*:\s*([^;}\n]+)/g;
const FONT_FAMILY_JS_RE = /fontFamily\s*:\s*([^,}\n]+)/g;

function walk(dir: string, files: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    const st = statSync(full);
    if (st.isDirectory()) {
      if (entry === "node_modules" || entry === "__tests__" || entry === "dist") continue;
      walk(full, files);
    } else if (entry.endsWith(".css") || entry.endsWith(".ts") || entry.endsWith(".tsx")) {
      if (entry.endsWith(".d.ts")) continue;
      files.push(full);
    }
  }
  return files;
}

function extractValues(file: string): string[] {
  const text = readFileSync(file, "utf8");
  const re = file.endsWith(".css") ? FONT_FAMILY_CSS_RE : FONT_FAMILY_JS_RE;
  const matches = Array.from(text.matchAll(re));
  return matches.map((m) => m[1].trim().replace(/^["']|["']$/g, ""));
}

function isAllowedValue(value: string): boolean {
  const v = value.trim();
  return (
    v === "inherit" ||
    v === "var(--writ-font-sans)" ||
    v === "var(--writ-font-mono)"
  );
}

describe("typography tokens", () => {
  it("theme.css declares both --writ-font-sans and --writ-font-mono tokens", () => {
    const theme = readFileSync(THEME_CSS, "utf8");
    expect(theme).toMatch(/--writ-font-sans\s*:/);
    expect(theme).toMatch(/--writ-font-mono\s*:/);
  });

  it("body resolves to --writ-font-sans", () => {
    const global = readFileSync(GLOBAL_CSS, "utf8");
    const bodyBlock = global.match(/html\s*,\s*body\s*\{[^}]*\}/);
    expect(bodyBlock, "expected html,body block in global.css").not.toBeNull();
    expect(bodyBlock![0]).toContain("font-family: var(--writ-font-sans)");
    expect(bodyBlock![0]).not.toContain("font-family: var(--writ-font-mono)");
  });

  it("mono token is referenced only in TabItem.css and cm-theme.ts", () => {
    const files = walk(SRC);
    const offenders: string[] = [];
    for (const file of files) {
      const values = extractValues(file);
      const usesMono = values.some((v) => v.includes("--writ-font-mono"));
      if (usesMono && !MONO_ALLOWED.has(file)) {
        offenders.push(relative(REPO_ROOT, file));
      }
    }
    expect(offenders, `unexpected mono references: ${offenders.join(", ")}`).toEqual([]);
  });

  it("no file declares a literal font-family value (only var() or inherit)", () => {
    const files = walk(SRC);
    const offenders: { file: string; value: string }[] = [];
    for (const file of files) {
      const values = extractValues(file);
      for (const value of values) {
        if (!isAllowedValue(value)) {
          offenders.push({ file: relative(REPO_ROOT, file), value });
        }
      }
    }
    expect(
      offenders,
      `hardcoded font-family values found: ${offenders
        .map((o) => `${o.file} -> ${o.value}`)
        .join("; ")}`,
    ).toEqual([]);
  });
});
