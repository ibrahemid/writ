import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { createSignal } from "solid-js";

const h = vi.hoisted(() => ({ maximized: (): boolean => false }));

vi.mock("../../stores/global/os-window", () => ({
  osWindowStore: { maximized: () => h.maximized() },
}));

import AppFrame from "../../components/AppFrame/AppFrame";

afterEach(() => {
  cleanup();
  h.maximized = () => false;
  vi.clearAllMocks();
});

describe("the window frame follows the maximized signal", () => {
  it("carries no maximized class while the window floats", () => {
    const { container } = render(() => <AppFrame>{null}</AppFrame>);
    expect(container.querySelector(".app-container")!.classList.contains("is-maximized")).toBe(
      false,
    );
  });

  // The GNOME inset that makes room for the client-side-decoration shadow and
  // the rounded corners both hang off this one class, so it has to track the
  // signal rather than being read once at mount.
  it("takes the class on and off as the signal changes", () => {
    const [maximized, setMaximized] = createSignal(false);
    h.maximized = maximized;
    const { container } = render(() => <AppFrame>{null}</AppFrame>);
    const frame = container.querySelector(".app-container")!;

    setMaximized(true);
    expect(frame.classList.contains("is-maximized")).toBe(true);

    setMaximized(false);
    expect(frame.classList.contains("is-maximized")).toBe(false);
  });

  it("renders what it wraps", () => {
    const { container } = render(() => (
      <AppFrame>
        <p class="child" />
      </AppFrame>
    ));
    expect(container.querySelector(".app-container > .child")).not.toBeNull();
  });
});
