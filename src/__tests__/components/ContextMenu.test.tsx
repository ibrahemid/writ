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

describe("ContextMenu separator items stay clickable", () => {
  // `separator: true` draws a divider above an item; it never meant the item
  // itself is a divider. Reading it that way made every group-opening item dead
  // on arrival: Spelling settings, Close All Tabs, Clear All History.
  it("runs the action of an item that opens a group", () => {
    const action = vi.fn();
    const { container, getByText } = render(() => <ContextMenu />);
    showContextMenu(0, 0, [
      { label: "Turn off spelling", action: () => {} },
      { label: "Spelling settings", action, separator: true },
    ]);
    fireEvent.click(getByText("Spelling settings"));
    expect(action).toHaveBeenCalledTimes(1);
    expect(container.querySelector(".context-menu")).toBeNull();
  });

  it("still draws the divider and still skips disabled items", () => {
    const action = vi.fn();
    const { container, getByText } = render(() => <ContextMenu />);
    showContextMenu(0, 0, [
      { label: "Live", action: () => {} },
      { label: "Not reachable", action, disabled: true, separator: true },
    ]);
    expect(container.querySelector(".context-menu-separator")).not.toBeNull();
    fireEvent.click(getByText("Not reachable"));
    expect(action).not.toHaveBeenCalled();
  });

  it("gives keyboard focus to a separator item", () => {
    const action = vi.fn();
    render(() => <ContextMenu />);
    showContextMenu(0, 0, [
      { label: "First", action: () => {} },
      { label: "Second", action, separator: true },
    ]);
    const menu = document.querySelector(".context-menu")!;
    fireEvent.keyDown(menu, { key: "ArrowDown" });
    fireEvent.keyDown(menu, { key: "Enter" });
    expect(action).toHaveBeenCalledTimes(1);
  });
});

describe("ContextMenu viewport clamping", () => {
  it("flips back over the cursor near the right and bottom edges", () => {
    const { container } = render(() => <ContextMenu />);
    // jsdom reports a zero-sized box, so stub a realistic one.
    const rect = { width: 200, height: 150 } as DOMRect;
    const original = Element.prototype.getBoundingClientRect;
    Element.prototype.getBoundingClientRect = function (this: Element) {
      return this.classList?.contains("context-menu") ? rect : original.call(this);
    };

    try {
      showContextMenu(window.innerWidth - 10, window.innerHeight - 10, [
        { label: "Copy", action: () => {} },
      ]);
      const el = container.querySelector<HTMLElement>(".context-menu")!;
      const left = Number.parseFloat(el.style.left);
      const top = Number.parseFloat(el.style.top);
      expect(left + rect.width).toBeLessThanOrEqual(window.innerWidth);
      expect(top + rect.height).toBeLessThanOrEqual(window.innerHeight);
    } finally {
      Element.prototype.getBoundingClientRect = original;
    }
  });

  it("leaves a menu that fits exactly where it was asked for", () => {
    const { container } = render(() => <ContextMenu />);
    showContextMenu(10, 20, [{ label: "Copy", action: () => {} }]);
    const el = container.querySelector<HTMLElement>(".context-menu")!;
    expect(el.style.left).toBe("10px");
    expect(el.style.top).toBe("20px");
  });

  it("renders a shortcut hint when one is given", () => {
    const { getByText } = render(() => <ContextMenu />);
    showContextMenu(0, 0, [{ label: "Copy", action: () => {}, kbd: "⌘C" }]);
    expect(getByText("⌘C")).not.toBeNull();
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
