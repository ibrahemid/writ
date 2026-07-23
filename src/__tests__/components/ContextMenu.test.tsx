import { describe, it, expect, afterEach, vi } from "vitest";
import { render, fireEvent, cleanup } from "@solidjs/testing-library";
import ContextMenu, {
  showAnchoredMenu,
  showContextMenu,
  hideContextMenu,
} from "../../components/ContextMenu/ContextMenu";

afterEach(() => {
  hideContextMenu();
  cleanup();
  vi.useRealTimers();
});

// A trigger button plus the singleton menu, mirroring how the status-bar chip
// opens an anchored menu on a left click.
function Harness() {
  let btn!: HTMLButtonElement;
  return (
    <>
      <button
        ref={btn}
        onClick={() =>
          showAnchoredMenu(
            btn.getBoundingClientRect(),
            [{ label: "Item A", action: () => {} }],
            btn,
          )
        }
      >
        open
      </button>
      <ContextMenu />
    </>
  );
}

describe("ContextMenu anchored open", () => {
  it("stays open after the click that opened it", () => {
    const { getByText, container } = render(() => <Harness />);
    fireEvent.click(getByText("open"));
    // The bug: the same delegated click would close the menu instantly.
    expect(container.querySelector(".context-menu")).not.toBeNull();
  });

  it("closes on a genuine outside click once the listener is armed", () => {
    vi.useFakeTimers();
    const { getByText, container } = render(() => <Harness />);
    fireEvent.click(getByText("open"));
    expect(container.querySelector(".context-menu")).not.toBeNull();
    // Arm the deferred outside-click listener.
    vi.advanceTimersByTime(1);
    fireEvent.click(document.body);
    expect(container.querySelector(".context-menu")).toBeNull();
  });

  it("closes on Escape", () => {
    const { getByText, container } = render(() => <Harness />);
    fireEvent.click(getByText("open"));
    const menu = container.querySelector(".context-menu")!;
    fireEvent.keyDown(menu, { key: "Escape" });
    expect(container.querySelector(".context-menu")).toBeNull();
  });
});

describe("ContextMenu cursor open (right-click call site)", () => {
  it("opens at cursor coordinates and still dismisses on an outside click", () => {
    vi.useFakeTimers();
    const { container } = render(() => <ContextMenu />);
    showContextMenu(10, 20, [{ label: "Rename", action: () => {} }]);
    const el = container.querySelector<HTMLElement>(".context-menu");
    expect(el).not.toBeNull();
    expect(el!.style.left).toBe("10px");
    expect(el!.style.top).toBe("20px");
    vi.advanceTimersByTime(1);
    fireEvent.click(document.body);
    expect(container.querySelector(".context-menu")).toBeNull();
  });
});
