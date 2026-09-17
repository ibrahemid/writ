import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { resolve, join, relative } from "node:path";

// A comment between a selector and its brace is not a comment: the browser
// reads it as part of the selector list, so the rule matches nothing it was
// written for and whatever follows loses its scope. It is what a careless
// replace over a stylesheet leaves behind, and nothing else in a build says so.

const ROOT = process.cwd();

function stylesheets(dir: string, found: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      if (entry !== "node_modules") stylesheets(full, found);
    } else if (entry.endsWith(".css")) {
      found.push(full);
    }
  }
  return found;
}

/** The selector list of one rule, with the comments written above it dropped. */
function selectorLists(css: string): string[] {
  return [...css.matchAll(/([^{}]+)\{/g)].map(([, prelude]) =>
    prelude.replace(/^(?:\s|\/\*[\s\S]*?\*\/)*/, ""),
  );
}

describe("authored stylesheets", () => {
  it("never open a selector with a comment inside it", () => {
    const offenders: string[] = [];
    for (const file of stylesheets(resolve(ROOT, "src/components")).concat(
      stylesheets(resolve(ROOT, "src/styles")),
    )) {
      for (const selector of selectorLists(readFileSync(file, "utf8"))) {
        if (selector.includes("/*")) {
          offenders.push(`${relative(ROOT, file)} -> ${selector.split("\n")[0].trim()}`);
        }
      }
    }
    expect(offenders, `commented selectors:\n${offenders.join("\n")}`).toEqual([]);
  });

  it("scope the panel's scroller padding to a platform, as the sidebar's is", () => {
    const css = readFileSync(resolve(ROOT, "src/components/RightPanel/RightPanel.css"), "utf8");
    expect(css).toMatch(/:root\[data-platform="win"\] \.right-panel-scroll\s*\{[^}]*padding-top:\s*4px/);
    expect(css).toMatch(
      /:root\[data-platform="linux"\] \.right-panel-scroll\s*\{[^}]*padding-top:\s*6px/,
    );
    expect(css.match(/\.right-panel-empty\s*\{/g)).toHaveLength(1);
  });
});
