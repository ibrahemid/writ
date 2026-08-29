import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import Tooltip from "../../components/Tooltip/Tooltip";

const HOVER_DELAY_MS = 500;

function tip(): HTMLElement | null {
  return document.querySelector('[role="tooltip"]');
}

function mount() {
  const result = render(() => (
    <Tooltip label="Toggle sidebar">
      <button type="button">Sidebar</button>
    </Tooltip>
  ));
  return {
    ...result,
    anchor: result.container.firstElementChild as HTMLElement,
    button: result.container.querySelector("button") as HTMLButtonElement,
  };
}

// jsdom answers `:focus-visible` false for every element, keyboard or not, so
// a keyboard focus has to be stated rather than performed.
function focusByKeyboard(anchor: HTMLElement) {
  const real = Element.prototype.matches;
  vi.spyOn(Element.prototype, "matches").mockImplementation(function (
    this: Element,
    selector: string,
  ) {
    return selector === ":focus-visible" ? true : real.call(this, selector);
  });
  fireEvent.focusIn(anchor);
}

function mockRects(anchorRect: Record<string, number>, tipRect: { width: number; height: number }) {
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
    const box =
      this.getAttribute("role") === "tooltip"
        ? { left: 0, right: tipRect.width, top: 0, bottom: tipRect.height, ...tipRect }
        : anchorRect;
    return { ...box, x: box.left, y: box.top, toJSON: () => box } as DOMRect;
  });
}

describe("Tooltip", () => {
  beforeEach(() => vi.useFakeTimers());

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("waits out the hover delay before it appears", () => {
    const { anchor } = mount();

    fireEvent.pointerEnter(anchor);
    vi.advanceTimersByTime(HOVER_DELAY_MS - 1);
    expect(tip()).toBeNull();

    vi.advanceTimersByTime(1);
    expect(tip()?.textContent).toBe("Toggle sidebar");
  });

  it("leaving before the delay never shows it", () => {
    const { anchor } = mount();

    fireEvent.pointerEnter(anchor);
    vi.advanceTimersByTime(200);
    fireEvent.pointerLeave(anchor);
    vi.advanceTimersByTime(HOVER_DELAY_MS);
    expect(tip()).toBeNull();
  });

  it("a pointer press that focuses the control shows nothing until the hover delay", () => {
    const { anchor, button } = mount();

    fireEvent.pointerEnter(anchor);
    fireEvent.pointerDown(button);
    fireEvent.focusIn(anchor);
    expect(tip()).toBeNull();

    vi.advanceTimersByTime(HOVER_DELAY_MS);
    expect(tip()).not.toBeNull();
  });

  it("keyboard focus shows it at once and describes the control", () => {
    const { anchor, button } = mount();

    focusByKeyboard(anchor);
    const shown = tip();
    expect(shown?.getAttribute("role")).toBe("tooltip");
    expect(shown?.textContent).toBe("Toggle sidebar");
    expect(button.getAttribute("aria-describedby")).toBe(shown?.id);
  });

  it("a pointer sweep does not dismiss a keyboard-focused tip", () => {
    const { anchor } = mount();

    focusByKeyboard(anchor);
    fireEvent.pointerEnter(anchor);
    fireEvent.pointerLeave(anchor);
    vi.advanceTimersByTime(HOVER_DELAY_MS);
    expect(tip()).not.toBeNull();
  });

  it("blur dismisses it and drops the description", () => {
    const { anchor, button } = mount();

    focusByKeyboard(anchor);
    expect(tip()).not.toBeNull();

    fireEvent.focusOut(anchor);
    expect(tip()).toBeNull();
    expect(button.getAttribute("aria-describedby")).toBeNull();
  });

  it("is removed on Escape", () => {
    const { anchor } = mount();

    focusByKeyboard(anchor);
    expect(tip()).not.toBeNull();

    fireEvent.keyDown(document, { key: "Escape" });
    expect(tip()).toBeNull();
  });

  it("clamps to the viewport instead of overflowing it", () => {
    const anchorRect = { left: 940, right: 1000, top: 300, bottom: 320, width: 60, height: 20 };
    const tipRect = { width: 160, height: 26 };
    mockRects(anchorRect, tipRect);

    const { anchor } = mount();
    focusByKeyboard(anchor);

    const shown = tip() as HTMLElement;
    // Centred would be 890px, which puts the right edge past window.innerWidth.
    expect(shown.style.left).toBe(`${window.innerWidth - tipRect.width - 4}px`);
    expect(shown.style.top).toBe(`${anchorRect.top - 6 - tipRect.height}px`);
  });

  it("flips below the anchor when there is no room above", () => {
    const anchorRect = { left: 20, right: 80, top: 4, bottom: 24, width: 60, height: 20 };
    const tipRect = { width: 80, height: 26 };
    mockRects(anchorRect, tipRect);

    const { anchor } = mount();
    focusByKeyboard(anchor);

    expect((tip() as HTMLElement).style.top).toBe(`${anchorRect.bottom + 6}px`);
  });

  it("cleans up its listeners on unmount", () => {
    const remove = vi.spyOn(document, "removeEventListener");
    const { anchor, unmount } = mount();

    focusByKeyboard(anchor);
    unmount();

    expect(remove).toHaveBeenCalledWith("keydown", expect.any(Function), true);
  });
});
