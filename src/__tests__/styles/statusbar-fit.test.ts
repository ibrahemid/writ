import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

// With the sidebar open at the default window width, the status bar's right
// group is wider than the space left beside the save chip. The bar must keep a
// gap between the two groups and shed its informational fields before it clips
// the palette cue, instead of letting "saved" run into "Ln 1, Col 1".

const CSS = readFileSync(resolve(process.cwd(), "src/components/Editor/StatusBar.css"), "utf8");

const block = (selector: string): string => {
  const m = CSS.match(new RegExp(`${selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\s*\\{([^}]*)\\}`));
  expect(m, `${selector} is declared`).toBeTruthy();
  return m![1]!;
};

describe("status bar fit", () => {
  it("keeps a gap between the save chip and the fields", () => {
    expect(block(".statusbar")).toMatch(/gap:\s*var\(--writ-space-4\)/);
  });

  it("is a size container so fields can shed as the editor narrows", () => {
    expect(block(".statusbar")).toMatch(/container-type:\s*inline-size/);
    expect(CSS).toMatch(/@container \(max-width: [\d.]+em\)\s*\{\s*\.statusbar-field:not\(\.statusbar-field--cursor\)\s*\{\s*display:\s*none/);
    expect(CSS).toMatch(/@container \(max-width: [\d.]+em\)[^@]*\.statusbar-right > \.statusbar-label\s*\{\s*display:\s*none/);
  });

  it("never drops the cursor position", () => {
    expect(CSS).not.toMatch(/\.statusbar-field--cursor\s*\{[^}]*display:\s*none/);
  });

  // The save label carries the file name, so the left block is as wide as the
  // longest name a note can have. It gives way first and truncates; the right
  // cluster is controls, and a control clipped off the edge has no other way in.
  it("shrinks the left block before the controls on the right", () => {
    expect(block(".statusbar-left")).toMatch(/flex-shrink:\s*1/);
    expect(block(".statusbar-left")).not.toMatch(/flex-shrink:\s*0/);
    expect(block(".statusbar-live")).toMatch(/min-width:\s*0/);
  });

  it("ellipsises the label rather than letting it push the bar wide", () => {
    const label = CSS.match(/^\.statusbar-label\s*\{([^}]*)\}/m)?.[1] ?? "";
    expect(label).toMatch(/overflow:\s*hidden/);
    expect(label).toMatch(/text-overflow:\s*ellipsis/);
  });
});
