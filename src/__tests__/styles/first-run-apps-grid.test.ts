import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const css = readFileSync(resolve(process.cwd(), "src/components/FirstRun/FirstRunSetup.css"), "utf8");

function block(selector: string): string | null {
  const at = css.indexOf(`${selector} {`);
  if (at === -1) return null;
  return css.slice(at, css.indexOf("}", at));
}

describe("the Apps step grid", () => {
  it("lays the cards out two to a row", () => {
    expect(block(".first-run-formats,\n.first-run-apps")).toContain(
      "grid-template-columns: repeat(2, minmax(0, 1fr))",
    );
  });

  it("gives a last card left alone on its row the row's full width", () => {
    expect(block(".first-run-app:last-child:nth-child(odd)")).toContain("grid-column: 1 / -1");
  });
});
