import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import PreviewSplit from "../../components/Preview/PreviewSplit";

afterEach(() => cleanup());

function container(width: number, height: number): HTMLElement {
  const el = document.createElement("div");
  el.getBoundingClientRect = () =>
    ({ left: 0, top: 0, width, height, right: width, bottom: height, x: 0, y: 0 }) as DOMRect;
  return el;
}

function mount(orientation: "vertical" | "horizontal", start = 0.5) {
  const [ratio, setRatio] = createSignal(start);
  const onResize = vi.fn((next: number) => setRatio(next));
  const onCommit = vi.fn();
  const box = container(1000, 600);
  const { container: root } = render(() => (
    <PreviewSplit
      orientation={orientation}
      ratio={ratio()}
      container={() => box}
      onResize={onResize}
      onCommit={onCommit}
    />
  ));
  const handle = root.querySelector<HTMLElement>('[role="separator"]')!;
  handle.setPointerCapture = vi.fn();
  handle.releasePointerCapture = vi.fn();
  return { handle, onResize, onCommit, ratio };
}

describe("the preview split handle", () => {
  it("is a focusable separator that states its position and range", () => {
    const { handle } = mount("vertical", 0.4);
    expect(handle.tabIndex).toBe(0);
    expect(handle.getAttribute("aria-orientation")).toBe("vertical");
    expect(handle.getAttribute("aria-valuenow")).toBe("40");
    expect(handle.getAttribute("aria-valuemin")).toBe("15");
    expect(handle.getAttribute("aria-valuemax")).toBe("85");
    expect(handle.getAttribute("aria-label")).toBeTruthy();
  });

  it("follows the pointer and stops at either end", () => {
    const { handle, onResize, onCommit } = mount("vertical");
    fireEvent.pointerDown(handle, { pointerId: 1, clientX: 500 });
    fireEvent.pointerMove(handle, { pointerId: 1, clientX: 300 });
    expect(onResize).toHaveBeenLastCalledWith(0.3);

    fireEvent.pointerMove(handle, { pointerId: 1, clientX: 10 });
    expect(onResize).toHaveBeenLastCalledWith(0.15);
    fireEvent.pointerMove(handle, { pointerId: 1, clientX: 990 });
    expect(onResize).toHaveBeenLastCalledWith(0.85);

    fireEvent.pointerUp(handle, { pointerId: 1 });
    expect(onCommit).toHaveBeenCalledTimes(1);
    expect(handle.getAttribute("aria-valuenow")).toBe("85");
  });

  it("reads the pointer's height when the panes are stacked", () => {
    const { handle, onResize } = mount("horizontal");
    fireEvent.pointerDown(handle, { pointerId: 1, clientY: 300 });
    fireEvent.pointerMove(handle, { pointerId: 1, clientY: 150 });
    expect(onResize).toHaveBeenLastCalledWith(0.25);
  });

  it("moves in steps from the keyboard, within the same bounds, and saves each step", () => {
    const { handle, onResize, onCommit } = mount("vertical", 0.84);
    fireEvent.keyDown(handle, { key: "ArrowRight" });
    expect(onResize).toHaveBeenLastCalledWith(0.85);
    fireEvent.keyDown(handle, { key: "ArrowLeft" });
    expect(onResize.mock.calls[onResize.mock.calls.length - 1][0]).toBeCloseTo(0.83);
    expect(onCommit).toHaveBeenCalledTimes(2);
  });

  it("ignores pointer movement that did not start on it", () => {
    const { handle, onResize } = mount("vertical");
    fireEvent.pointerMove(handle, { pointerId: 1, clientX: 300 });
    expect(onResize).not.toHaveBeenCalled();
  });
});
