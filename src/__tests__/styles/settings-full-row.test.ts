import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const css = readFileSync(resolve(process.cwd(), "src/components/SettingsModal/SettingsModal.css"), "utf8");

function block(selector: string): string | null {
  const at = css.indexOf(`${selector} {`);
  if (at === -1) return null;
  return css.slice(at, css.indexOf("}", at));
}

// The Shortcuts list fills its row; its sentence ("Click Record, then press the
// keys.") has to stay on screen while the label is hidden.
describe("a full-width settings row", () => {
  it("hides only the label text, never the description", () => {
    expect(block('.settings-row[data-layout="full"] .settings-row-label-text')).toContain(
      "clip-path: inset(50%)",
    );
    expect(block('.settings-row[data-layout="full"] > .settings-row-label') ?? "").not.toContain("clip-path");
  });
});
