import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import Tooltip from "../../components/Tooltip/Tooltip";

const HOVER_DELAY_MS = 500;

function tip(): HTMLElement | null {
  return document.querySelector('[role="tooltip"]');
}

describe("Tooltip", () => {
  beforeEach(() => vi.useFakeTimers());

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("waits out the hover delay before it appears", () => {
    const { container } = render(() => (
      <Tooltip label="Toggle sidebar">
        <button type="button">Sidebar</button>
      </Tooltip>
    ));
    const anchor = container.firstElementChild as HTMLElement;

    fireEvent.pointerEnter(anchor);
    vi.advanceTimersByTime(HOVER_DELAY_MS - 1);
    expect(tip()).toBeNull();

    vi.advanceTimersByTime(1);
    expect(tip()?.textContent).toBe("Toggle sidebar");
  });

  it("leaving before the delay never shows it", () => {
    const { container } = render(() => (
      <Tooltip label="Toggle sidebar">
        <button type="button">Sidebar</button>
      </Tooltip>
    ));
    const anchor = container.firstElementChild as HTMLElement;

    fireEvent.pointerEnter(anchor);
    vi.advanceTimersByTime(200);
    fireEvent.pointerLeave(anchor);
    vi.advanceTimersByTime(HOVER_DELAY_MS);
    expect(tip()).toBeNull();
  });

  it("shows on focus without a delay and describes the control", () => {
    const { container } = render(() => (
      <Tooltip label="Toggle sidebar">
        <button type="button">Sidebar</button>
      </Tooltip>
    ));
    const anchor = container.firstElementChild as HTMLElement;
    const button = container.querySelector("button") as HTMLButtonElement;

    fireEvent.focusIn(anchor);
    const shown = tip();
    expect(shown?.getAttribute("role")).toBe("tooltip");
    expect(button.getAttribute("aria-describedby")).toBe(shown?.id);

    fireEvent.focusOut(anchor);
    expect(tip()).toBeNull();
    expect(button.getAttribute("aria-describedby")).toBeNull();
  });

  it("is removed on Escape", () => {
    const { container } = render(() => (
      <Tooltip label="Toggle sidebar">
        <button type="button">Sidebar</button>
      </Tooltip>
    ));
    const anchor = container.firstElementChild as HTMLElement;

    fireEvent.focusIn(anchor);
    expect(tip()).not.toBeNull();

    fireEvent.keyDown(document, { key: "Escape" });
    expect(tip()).toBeNull();
  });

  it("clamps to the viewport instead of overflowing it", () => {
    const anchorRect = { left: 940, right: 1000, top: 300, bottom: 320, width: 60, height: 20 };
    const tipRect = { width: 160, height: 26 };
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (
      this: Element,
    ) {
      const box =
        this.getAttribute("role") === "tooltip"
          ? { left: 0, right: tipRect.width, top: 0, bottom: tipRect.height, ...tipRect }
          : anchorRect;
      return { ...box, x: box.left, y: box.top, toJSON: () => box } as DOMRect;
    });

    const { container } = render(() => (
      <Tooltip label="A label wide enough to overflow">
        <button type="button">Sidebar</button>
      </Tooltip>
    ));
    fireEvent.focusIn(container.firstElementChild as HTMLElement);

    const shown = tip() as HTMLElement;
    // Centred would be 890px, which puts the right edge past window.innerWidth.
    expect(shown.style.left).toBe(`${window.innerWidth - tipRect.width - 4}px`);
    expect(shown.style.top).toBe(`${anchorRect.top - 6 - tipRect.height}px`);
  });

  it("flips below the anchor when there is no room above", () => {
    const anchorRect = { left: 20, right: 80, top: 4, bottom: 24, width: 60, height: 20 };
    const tipRect = { width: 80, height: 26 };
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (
      this: Element,
    ) {
      const box =
        this.getAttribute("role") === "tooltip"
          ? { left: 0, right: tipRect.width, top: 0, bottom: tipRect.height, ...tipRect }
          : anchorRect;
      return { ...box, x: box.left, y: box.top, toJSON: () => box } as DOMRect;
    });

    const { container } = render(() => (
      <Tooltip label="Toggle sidebar">
        <button type="button">Sidebar</button>
      </Tooltip>
    ));
    fireEvent.focusIn(container.firstElementChild as HTMLElement);

    expect((tip() as HTMLElement).style.top).toBe(`${anchorRect.bottom + 6}px`);
  });

  it("cleans up its listeners on unmount", () => {
    const remove = vi.spyOn(document, "removeEventListener");
    const { container, unmount } = render(() => (
      <Tooltip label="Toggle sidebar">
        <button type="button">Sidebar</button>
      </Tooltip>
    ));
    fireEvent.focusIn(container.firstElementChild as HTMLElement);
    unmount();

    expect(remove).toHaveBeenCalledWith("keydown", expect.any(Function), true);
  });
});
