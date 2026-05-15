import { describe, it, expect, afterEach } from "vitest";
import { getFocusableWithin, getFirstFocusable } from "../../lib/focusable";

function el(tag: string, attrs: Record<string, string> = {}, text?: string): HTMLElement {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) node.setAttribute(k, v);
  if (text !== undefined) node.textContent = text;
  return node;
}

function setup(...children: HTMLElement[]): HTMLElement {
  const root = document.createElement("div");
  for (const c of children) root.appendChild(c);
  document.body.appendChild(root);
  return root;
}

describe("getFocusableWithin", () => {
  afterEach(() => {
    while (document.body.firstChild) document.body.removeChild(document.body.firstChild);
  });

  it("returns focusable buttons and inputs in document order", () => {
    const root = setup(
      el("button", {}, "a"),
      el("input", { type: "text" }),
      el("button", {}, "b"),
    );
    const items = getFocusableWithin(root);
    expect(items).toHaveLength(3);
    expect(items[0].textContent).toBe("a");
  });

  it("excludes disabled and tabindex=-1 elements", () => {
    const root = setup(
      el("button", {}, "a"),
      el("button", { disabled: "" }, "b"),
      el("button", { tabindex: "-1" }, "c"),
    );
    const items = getFocusableWithin(root);
    expect(items).toHaveLength(1);
    expect(items[0].textContent).toBe("a");
  });

  it("skips elements inside an [inert] subtree", () => {
    const inertBlock = el("div", { inert: "" });
    inertBlock.appendChild(el("button", {}, "hidden"));
    const root = setup(inertBlock, el("button", {}, "visible"));
    const items = getFocusableWithin(root);
    expect(items).toHaveLength(1);
    expect(items[0].textContent).toBe("visible");
  });

  it("getFirstFocusable returns null on empty", () => {
    const root = setup(el("div"));
    expect(getFirstFocusable(root)).toBeNull();
  });
});
