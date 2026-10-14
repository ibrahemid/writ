import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import TabItem from "../../components/Sidebar/TabItem";

// The row is a list item holding a real button that opens it, with its
// actions as buttons beside that one, never nested in a button role (NV-30).
describe("TabItem structure and keyboard (#44, NV-30)", () => {
  afterEach(() => cleanup());

  it("is a list item whose open control is a real button", () => {
    const { container } = render(() => (
      <TabItem label="example.txt" onClick={() => undefined} onClose={() => undefined} />
    ));
    const row = container.querySelector<HTMLDivElement>(".tab-item")!;
    expect(row.getAttribute("role")).toBe("listitem");
    expect(row.hasAttribute("tabindex")).toBe(false);
    const main = row.querySelector<HTMLButtonElement>(".tab-item-main")!;
    expect(main.tagName).toBe("BUTTON");
    expect(main.getAttribute("type")).toBe("button");
  });

  it("nests no control inside another control", () => {
    const { container } = render(() => (
      <TabItem
        label="example.txt"
        onClick={() => undefined}
        onClose={() => undefined}
        onRestore={() => undefined}
      />
    ));
    expect(container.querySelector('[role="button"]')).toBeNull();
    expect(container.querySelector("button button")).toBeNull();
    const buttons = Array.from(container.querySelectorAll("button"));
    expect(buttons).toHaveLength(3);
    for (const button of buttons) {
      expect(button.parentElement?.closest("button")).toBeNull();
    }
  });

  it("opens the row from its button", () => {
    const onClick = vi.fn();
    const { container } = render(() => <TabItem label="example.txt" onClick={onClick} />);
    fireEvent.click(container.querySelector(".tab-item-main")!);
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it("draws no actions slot for a row without actions", () => {
    const { container } = render(() => <TabItem label="example.txt" onClick={() => undefined} />);
    expect(container.querySelector(".tab-item-actions")).toBeNull();
  });
});
