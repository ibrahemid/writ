import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

// The chat column is narrow by default and can be dragged narrower, so what a
// reply renders has to fit it. The diff gutter is the one place the column
// itself fixes a width: a proposal touching line 1000 of a note has to number
// it without running into the mark beside it.

const CSS = readFileSync(resolve(process.cwd(), "src/components/Chat/ChatPane.css"), "utf8");

const block = (selector: string): string => {
  const found = CSS.match(
    new RegExp(`${selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\s*\\{([^}]*)\\}`),
  );
  expect(found, `${selector} is declared`).toBeTruthy();
  return found![1]!;
};

describe("the chat pane at a narrow width", () => {
  it("numbers a line of any length without widening the row", () => {
    const gutter = block(".chat-diff-num");
    expect(gutter).toMatch(/min-width:\s*2\.5ch/);
    expect(gutter).not.toMatch(/[^-]width:\s*[\d.]+ch/);
  });

  it("scrolls a wide table inside the reply", () => {
    expect(block(".chat-reply-table")).toMatch(/overflow-x:\s*auto/);
  });
});
