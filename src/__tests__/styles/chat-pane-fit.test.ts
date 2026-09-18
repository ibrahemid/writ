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

  // A box measured in rem keeps its size while the interface text grows, so
  // the boxes that hold lines of text are measured in line heights.
  it("sizes the mention list, the diff box and the field in line heights", () => {
    expect(block(".chat-mention")).toMatch(/max-height:\s*calc\(var\(--writ-ui-md-lh\)/);
    expect(block(".chat-diff")).toMatch(/max-height:\s*calc\(var\(--writ-ui-sm-lh\)/);
    const field = block(".chat-composer-input");
    expect(field).toMatch(/min-height:\s*calc\(var\(--writ-ui-md-lh\) \* 2/);
    expect(field).toMatch(/max-height:\s*calc\(var\(--writ-ui-md-lh\) \* 10/);
    expect(field).toMatch(/overflow-y:\s*auto/);
  });
});
