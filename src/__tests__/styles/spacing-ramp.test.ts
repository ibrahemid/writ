import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";

// The spacing ramp is a token set, so a padding of 8px written as a literal is
// a fork of --writ-space-3 that no theme can move. 6, 10 and 14px sit between
// the steps and are the half-steps --writ-space-2-5, -3-5 and -4-5, so they are
// read from the ramp too. A px inside calc() is not bare — the ramp cannot be
// substituted there without rewriting the expression — so calc() spans are cut
// before the scan.

const REPO_ROOT = process.cwd();
const COMPONENTS = resolve(REPO_ROOT, "src/components");

/** Spacing properties, including their logical and axis forms. */
export const SPACING_PROPERTY =
  /^(?:(?:padding|margin)(?:-(?:top|right|bottom|left|inline|block)(?:-(?:start|end))?)?|(?:row-|column-)?gap|inset(?:-(?:inline|block)(?:-(?:start|end))?)?|top|right|bottom|left)$/;

/** The ramp: 2, 4, 8, 12, 16, 24, 32px are --writ-space-1 … -7, and 6, 10, 14px
 * are the half-steps between the first five. */
export const ON_RAMP_LITERAL = /(?<![\w.-])(?:2|4|6|8|10|12|14|16|24|32)px(?![\w-])/;

function stylesheets(dir: string, found: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      if (entry !== "node_modules" && entry !== "Chat") stylesheets(full, found);
    } else if (entry.endsWith(".css")) {
      found.push(full);
    }
  }
  return found;
}

/** Every calc(…) span blanked out, parentheses balanced. */
export function withoutCalc(value: string): string {
  let out = "";
  for (let i = 0; i < value.length; i += 1) {
    if (!value.startsWith("calc(", i)) {
      out += value[i];
      continue;
    }
    let depth = 0;
    let j = i + 4;
    for (; j < value.length; j += 1) {
      if (value[j] === "(") depth += 1;
      else if (value[j] === ")") {
        depth -= 1;
        if (depth === 0) break;
      }
    }
    i = j;
  }
  return out;
}

/** Comment bodies blanked, so a colon inside one cannot swallow the rule below it. */
export function withoutComments(css: string): string {
  return css.replace(/\/\*[\s\S]*?\*\//g, (block) => " ".repeat(block.length));
}

/** Spacing declarations in `css` holding a bare on-ramp literal. */
export function rampOffenders(css: string): string[] {
  const offenders: string[] = [];
  for (const [, property, value] of withoutComments(css).matchAll(
    /([a-z-]+)\s*:\s*([^;{}]+)[;}]/g,
  )) {
    if (!SPACING_PROPERTY.test(property)) continue;
    if (ON_RAMP_LITERAL.test(withoutCalc(value))) {
      offenders.push(`${property}: ${value.trim()}`);
    }
  }
  return offenders;
}

describe("component spacing", () => {
  it("reads the ramp tokens rather than repeating their pixel values", () => {
    const offenders: string[] = [];
    for (const file of stylesheets(COMPONENTS)) {
      for (const declaration of rampOffenders(readFileSync(file, "utf8"))) {
        offenders.push(`${relative(REPO_ROOT, file)} -> ${declaration}`);
      }
    }
    expect(offenders, `on-ramp literals:\n${offenders.join("\n")}`).toEqual([]);
  });
});
