import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

// Live menu items must read as live: the default foreground, with the subtle
// tone reserved for disabled items and the key hint. Muted on the raised
// surface sits under 4.5:1 in the dark presets and looks disabled.

const CSS = readFileSync(resolve(process.cwd(), "src/components/ContextMenu/ContextMenu.css"), "utf8");

const block = (selector: string): string => {
  const m = CSS.match(new RegExp(`${selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\s*\\{([^}]*)\\}`));
  expect(m, `${selector} is declared`).toBeTruthy();
  return m![1]!;
};

describe("context menu item tones", () => {
  it("sets live items in the default foreground", () => {
    expect(block(".context-menu-item")).toMatch(/color:\s*var\(--writ-foreground-default\)/);
  });

  it("keeps disabled items in the subtle tone so they differ from live ones", () => {
    expect(block(".context-menu-item:disabled")).toMatch(/color:\s*var\(--writ-foreground-subtle\)/);
  });
});
