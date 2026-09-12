import { describe, it, expect, afterEach } from "vitest";
import { moveTreeFocus } from "../../lib/tree-focus";

function tree(rows: number): { root: HTMLElement; items: HTMLElement[] } {
  const root = document.createElement("div");
  root.setAttribute("role", "tree");
  const items: HTMLElement[] = [];
  for (let i = 0; i < rows; i++) {
    const item = document.createElement("div");
    item.setAttribute("role", "treeitem");
    item.tabIndex = 0;
    root.appendChild(item);
    items.push(item);
  }
  document.body.appendChild(root);
  return { root, items };
}

afterEach(() => {
  document.body.innerHTML = "";
});

describe("moveTreeFocus", () => {
  it("moves focus to the next and previous row", () => {
    const { root, items } = tree(3);
    items[0].focus();
    moveTreeFocus(root, items[0], 1);
    expect(document.activeElement).toBe(items[1]);
    moveTreeFocus(root, items[1], -1);
    expect(document.activeElement).toBe(items[0]);
  });

  it("stays put at either end", () => {
    const { root, items } = tree(2);
    items[1].focus();
    moveTreeFocus(root, items[1], 1);
    expect(document.activeElement).toBe(items[1]);
    items[0].focus();
    moveTreeFocus(root, items[0], -1);
    expect(document.activeElement).toBe(items[0]);
  });

  it("does nothing for a row the tree does not hold", () => {
    const { root, items } = tree(2);
    const stranger = document.createElement("div");
    document.body.appendChild(stranger);
    items[0].focus();
    moveTreeFocus(root, stranger, 1);
    expect(document.activeElement).toBe(items[0]);
  });
});
