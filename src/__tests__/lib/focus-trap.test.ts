import { describe, it, expect, afterEach } from "vitest";
import { installFocusTrap } from "../../lib/focus-trap";

function btn(label: string): HTMLButtonElement {
  const b = document.createElement("button");
  b.textContent = label;
  return b;
}

function press(target: EventTarget, key: string, shiftKey = false): KeyboardEvent {
  const e = new KeyboardEvent("keydown", { key, shiftKey, bubbles: true, cancelable: true });
  target.dispatchEvent(e);
  return e;
}

describe("installFocusTrap", () => {
  afterEach(() => {
    while (document.body.firstChild) document.body.removeChild(document.body.firstChild);
  });

  it("focuses the first focusable on install and marks siblings inert", () => {
    const sibling = document.createElement("div");
    sibling.appendChild(btn("sibling"));
    document.body.appendChild(sibling);

    const container = document.createElement("div");
    const a = btn("a"); const b = btn("b");
    container.append(a, b);
    document.body.appendChild(container);

    const teardown = installFocusTrap(container);
    expect(document.activeElement).toBe(a);
    expect(sibling.hasAttribute("inert")).toBe(true);

    teardown();
    expect(sibling.hasAttribute("inert")).toBe(false);
  });

  it("Tab wraps from last to first", () => {
    const container = document.createElement("div");
    const a = btn("a"); const b = btn("b");
    container.append(a, b);
    document.body.appendChild(container);
    installFocusTrap(container);

    b.focus();
    press(b, "Tab");
    expect(document.activeElement).toBe(a);
  });

  it("Shift+Tab wraps from first to last", () => {
    const container = document.createElement("div");
    const a = btn("a"); const b = btn("b");
    container.append(a, b);
    document.body.appendChild(container);
    installFocusTrap(container);

    a.focus();
    press(a, "Tab", true);
    expect(document.activeElement).toBe(b);
  });

  it("Escape calls onEscape", () => {
    const container = document.createElement("div");
    const a = btn("a");
    container.appendChild(a);
    document.body.appendChild(container);

    let called = false;
    installFocusTrap(container, { onEscape: () => { called = true; } });
    press(a, "Escape");
    expect(called).toBe(true);
  });

  it("does not preventDefault on arrow keys", () => {
    const container = document.createElement("div");
    const a = btn("a");
    container.appendChild(a);
    document.body.appendChild(container);
    installFocusTrap(container);

    const e = press(a, "ArrowDown");
    expect(e.defaultPrevented).toBe(false);
  });

  it("restores previous focus on teardown", () => {
    const before = btn("before");
    document.body.appendChild(before);
    before.focus();
    expect(document.activeElement).toBe(before);

    const container = document.createElement("div");
    container.appendChild(btn("inside"));
    document.body.appendChild(container);

    const teardown = installFocusTrap(container);
    teardown();
    expect(document.activeElement).toBe(before);
  });

  it("falls back when previouslyFocused is null/body", () => {
    if (document.activeElement instanceof HTMLElement) document.activeElement.blur();

    const fallback = btn("fallback");
    document.body.appendChild(fallback);

    const container = document.createElement("div");
    container.appendChild(btn("inside"));
    document.body.appendChild(container);

    let fallbackCalled = false;
    const teardown = installFocusTrap(container, {
      fallbackRestore: () => {
        fallbackCalled = true;
        return fallback;
      },
    });
    teardown();
    expect(fallbackCalled).toBe(true);
    expect(document.activeElement).toBe(fallback);
  });

  it("does not mark ancestors of the container as inert", () => {
    const app = document.createElement("div");
    app.id = "app";
    const overlay = document.createElement("div");
    overlay.className = "overlay";
    const container = document.createElement("div");
    container.className = "modal";
    container.appendChild(btn("inside"));
    overlay.appendChild(container);
    app.appendChild(overlay);
    document.body.appendChild(app);

    const peerOutside = document.createElement("div");
    peerOutside.id = "peer-outside";
    peerOutside.appendChild(btn("peer"));
    document.body.appendChild(peerOutside);

    const teardown = installFocusTrap(container);
    expect(app.hasAttribute("inert")).toBe(false);
    expect(overlay.hasAttribute("inert")).toBe(false);
    expect(container.hasAttribute("inert")).toBe(false);
    expect(peerOutside.hasAttribute("inert")).toBe(true);

    teardown();
    expect(peerOutside.hasAttribute("inert")).toBe(false);
  });

  it("inerts siblings at every ancestor level along the path", () => {
    const app = document.createElement("div");
    app.id = "app";
    const sidebar = document.createElement("div");
    sidebar.id = "sidebar";
    sidebar.appendChild(btn("sidebar-btn"));
    const main = document.createElement("div");
    main.id = "main";
    const overlay = document.createElement("div");
    overlay.className = "overlay";
    const container = document.createElement("div");
    container.className = "modal";
    container.appendChild(btn("inside"));
    overlay.appendChild(container);
    main.appendChild(overlay);
    app.append(sidebar, main);
    document.body.appendChild(app);

    installFocusTrap(container);
    expect(sidebar.hasAttribute("inert")).toBe(true);
    expect(app.hasAttribute("inert")).toBe(false);
    expect(main.hasAttribute("inert")).toBe(false);
    expect(overlay.hasAttribute("inert")).toBe(false);
  });

  it("does not steal focus from an element already inside container", () => {
    const container = document.createElement("div");
    const a = btn("a"); const b = btn("b");
    container.append(a, b);
    document.body.appendChild(container);

    b.focus();
    expect(document.activeElement).toBe(b);
    installFocusTrap(container);
    expect(document.activeElement).toBe(b);
  });

  it("isActive=false suspends trapping", () => {
    const container = document.createElement("div");
    const a = btn("a"); const b = btn("b");
    container.append(a, b);
    document.body.appendChild(container);

    let active = false;
    let escapeCount = 0;
    installFocusTrap(container, {
      isActive: () => active,
      onEscape: () => escapeCount++,
    });

    press(a, "Escape");
    expect(escapeCount).toBe(0);

    active = true;
    press(a, "Escape");
    expect(escapeCount).toBe(1);
  });
});
