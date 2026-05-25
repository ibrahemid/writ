import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import TabItem from "../../components/Sidebar/TabItem";

describe("TabItem keyboard operability (#44)", () => {
  afterEach(() => cleanup());

  it("exposes role=button and tabIndex=0 so the row is focusable", () => {
    const { container } = render(() => (
      <TabItem title="example.txt" onClick={() => undefined} />
    ));
    const row = container.querySelector<HTMLDivElement>(".tab-item");
    expect(row).not.toBeNull();
    expect(row!.getAttribute("role")).toBe("button");
    expect(row!.tabIndex).toBe(0);
  });

  it("Enter activates the row", () => {
    const onClick = vi.fn();
    const { container } = render(() => (
      <TabItem title="example.txt" onClick={onClick} />
    ));
    const row = container.querySelector<HTMLDivElement>(".tab-item")!;
    fireEvent.keyDown(row, { key: "Enter" });
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it("Space activates the row and prevents default page scroll", () => {
    const onClick = vi.fn();
    const { container } = render(() => (
      <TabItem title="example.txt" onClick={onClick} />
    ));
    const row = container.querySelector<HTMLDivElement>(".tab-item")!;
    const event = new KeyboardEvent("keydown", { key: " ", bubbles: true, cancelable: true });
    row.dispatchEvent(event);
    expect(onClick).toHaveBeenCalledTimes(1);
    expect(event.defaultPrevented).toBe(true);
  });

  it("ignores unrelated keys", () => {
    const onClick = vi.fn();
    const { container } = render(() => (
      <TabItem title="example.txt" onClick={onClick} />
    ));
    const row = container.querySelector<HTMLDivElement>(".tab-item")!;
    fireEvent.keyDown(row, { key: "a" });
    fireEvent.keyDown(row, { key: "Tab" });
    expect(onClick).not.toHaveBeenCalled();
  });
});
