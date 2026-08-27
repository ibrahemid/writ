import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";

const REPO_ROOT = process.cwd();
const SRC = resolve(REPO_ROOT, "src");
const THEME_CSS = resolve(SRC, "styles/generated/theme.css");
const LEGACY_CSS = resolve(SRC, "styles/generated/legacy-aliases.css");
const GLOBAL_CSS = resolve(SRC, "styles/global.css");

// Mono is for code (ADR-030 decision 7): the editor's code face and the two
// markdown code surfaces. TabItem's timestamp goes with the sidebar rebuild.
const MONO_ALLOWED = new Set<string>([
  resolve(SRC, "components/Sidebar/TabItem.css"),
  resolve(SRC, "components/Editor/cm-theme.ts"),
  resolve(SRC, "components/Editor/cm-markdown-typography.css"),
]);

// The one @font-face the app ships. Its `font-family` names the face rather
// than reading a token, which is what a face declaration is.
const FONT_FACE_FILE = resolve(SRC, "styles/fonts.css");

const FONT_FAMILY_CSS_RE = /font-family\s*:\s*([^;}\n]+)/g;
const FONT_FAMILY_JS_RE = /fontFamily\s*:\s*([^,}\n]+)/g;

function walk(dir: string, files: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    const st = statSync(full);
    if (st.isDirectory()) {
      if (entry === "node_modules" || entry === "__tests__" || entry === "dist") continue;
      if (entry === "generated") continue;
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

const ALLOWED_FONT_VALUES = new Set([
  "inherit",
  "var(--writ-font-ui)",
  "var(--writ-font-prose)",
  "var(--writ-font-mono)",
  // Still declared by the legacy layer, still read by the files U3 did not
  // migrate. It goes with the last of them.
  "var(--writ-font-sans)",
]);

function isAllowedValue(value: string): boolean {
  return ALLOWED_FONT_VALUES.has(value.trim());
}

describe("typography tokens", () => {
  it("the generated sheet declares both --writ-font-ui and --writ-font-mono tokens", () => {
    const theme = readFileSync(THEME_CSS, "utf8");
    expect(theme).toMatch(/--writ-font-ui\s*:/);
    expect(theme).toMatch(/--writ-font-mono\s*:/);
  });

  it("the generated sheet declares the alternate prose face", () => {
    const theme = readFileSync(THEME_CSS, "utf8");
    expect(theme).toMatch(/--writ-font-prose-alt\s*:\s*"iA Writer Quattro S"/);
  });

  it("--writ-font-sans survives in the legacy layer for the files still reading it", () => {
    const legacy = readFileSync(LEGACY_CSS, "utf8");
    expect(legacy).toMatch(/--writ-font-sans\s*:/);
  });

  it("body resolves to --writ-font-ui", () => {
    const global = readFileSync(GLOBAL_CSS, "utf8");
    const bodyBlock = global.match(/html\s*,\s*body\s*\{[^}]*\}/);
    expect(bodyBlock, "expected html,body block in global.css").not.toBeNull();
    expect(bodyBlock![0]).toContain("font-family: var(--writ-font-ui)");
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
      if (file === FONT_FACE_FILE) continue;
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
