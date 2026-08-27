import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

// The Latin faces in the UI and mono stacks carry no Arabic glyphs. Every font stack the app or
// the preview resolves must therefore name an Arabic-capable family before the
// generic keyword, or per-glyph fallback lands on the platform default.

// The app sheet is imported first and the legacy layer second (global.css), so
// a name declared by both resolves to the legacy declaration. These stacks are
// what the root actually carries with no attribute set, not what one file says.
const APP_SHEETS = [
  "src/styles/generated/theme.css",
  "src/styles/generated/legacy-aliases.css",
].map((file) => readFileSync(resolve(process.cwd(), file), "utf8"));

const PREVIEW_CSS = readFileSync(
  resolve(process.cwd(), "src-tauri/assets/preview-base.css"),
  "utf8",
);

function rootDeclarations(sheets: string[]): Map<string, string> {
  const resolved = new Map<string, string>();
  for (const sheet of sheets) {
    const stripped = sheet.replace(/\/\*[\s\S]*?\*\//g, "");
    for (const [, selector, body] of stripped.matchAll(/([^{}]+)\{([^}]*)\}/g)) {
      if (selector.includes("@")) continue;
      const selects = selector.split(",").some((part) => part.trim() === ":root");
      if (!selects) continue;
      for (const line of body.split("\n")) {
        const m = /^\s*(--[a-z0-9-]+)\s*:\s*(.+);\s*$/.exec(line);
        if (m) resolved.set(m[1], m[2].trim());
      }
    }
  }
  return resolved;
}

const APP_ROOT = rootDeclarations(APP_SHEETS);

const ARABIC_FAMILIES = ['"SF Arabic"', '"Geeza Pro"', '"Noto Naskh Arabic"'];

const STACKS: { name: string; css: string | null; token: string; generic: string }[] = [
  { name: "app ui", css: null, token: "--writ-font-ui", generic: "system-ui" },
  { name: "app sans", css: null, token: "--writ-font-sans", generic: "system-ui" },
  { name: "app mono", css: null, token: "--writ-font-mono", generic: "monospace" },
  {
    name: "preview sans",
    css: PREVIEW_CSS,
    token: "--writ-preview-font-sans",
    generic: "system-ui",
  },
  {
    name: "preview mono",
    css: PREVIEW_CSS,
    token: "--writ-preview-font-mono",
    generic: "ui-monospace",
  },
];

function stackValue(css: string | null, token: string): string {
  if (css === null) {
    const resolved = APP_ROOT.get(token);
    if (!resolved) throw new Error(`token ${token} is not declared on the app root`);
    return resolved;
  }
  const m = new RegExp(`${token}\\s*:\\s*([^;]+);`).exec(css);
  if (!m) throw new Error(`token ${token} not found`);
  return m[1];
}

const BIDI_BLOCK_ELEMENTS = [
  "p",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "li",
  "blockquote",
  "th",
  "td",
  "figcaption",
  "dt",
  "dd",
];

function plaintextSelectors(css: string): string[] {
  const stripped = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const blocks = Array.from(stripped.matchAll(/([^{}]+)\{([^}]*)\}/g));
  return blocks
    .filter(([, , body]) => /unicode-bidi\s*:\s*plaintext/.test(body))
    .flatMap(([, selector]) => selector.split(",").map((s) => s.replace(/\s+/g, " ").trim()))
    .filter((s) => s.length > 0);
}

describe("Arabic font fallbacks", () => {
  for (const { name, css, token, generic } of STACKS) {
    it(`${name} stack names Arabic families before ${generic}`, () => {
      const value = stackValue(css, token);
      for (const family of ARABIC_FAMILIES) {
        expect(value, `${token} is missing ${family}`).toContain(family);
        // lastIndexOf: the generic family closes the stack, and "monospace"
        // also occurs inside the ui-monospace keyword that opens it.
        expect(
          value.indexOf(family),
          `${family} must precede ${generic} in ${token}`,
        ).toBeLessThan(value.lastIndexOf(generic));
      }
    });
  }
});

describe("preview automatic direction", () => {
  const selectors = plaintextSelectors(PREVIEW_CSS);

  it("resolves direction per text block", () => {
    for (const element of BIDI_BLOCK_ELEMENTS) {
      expect(selectors, `${element} should resolve its own direction`).toContain(element);
    }
  });

  it("leaves code blocks left-to-right", () => {
    expect(selectors).not.toContain("pre");
    expect(selectors).not.toContain("code");
    expect(selectors).not.toContain("pre code");
  });
});
