import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import TabItem from "../../components/Sidebar/TabItem";

const tabItemCss = readFileSync(
  resolve(process.cwd(), "src/components/Sidebar/TabItem.css"),
  "utf8",
);

describe("TabItem actions (#45)", () => {
  afterEach(() => cleanup());

  it("renders close as a real <button> with accessible name 'Close tab'", () => {
    const { container } = render(() => (
      <TabItem title="t" onClick={() => undefined} onClose={() => undefined} />
    ));
    const close = container.querySelector<HTMLButtonElement>(".tab-item-close");
    expect(close).not.toBeNull();
    expect(close!.tagName).toBe("BUTTON");
    expect(close!.getAttribute("type")).toBe("button");
    expect(close!.getAttribute("aria-label")).toBe("Close tab");
  });

  it("renders restore as a real <button> with accessible name 'Restore tab'", () => {
    const { container } = render(() => (
      <TabItem title="t" onClick={() => undefined} onRestore={() => undefined} />
    ));
    const buttons = container.querySelectorAll<HTMLButtonElement>(".tab-item-action");
    const restore = Array.from(buttons).find(
      (b) => b.getAttribute("aria-label") === "Restore tab",
    );
    expect(restore).toBeDefined();
    expect(restore!.tagName).toBe("BUTTON");
    expect(restore!.getAttribute("type")).toBe("button");
  });

  it("close click does not bubble to the row's onClick", () => {
    const onClick = vi.fn();
    const onClose = vi.fn();
    const { container } = render(() => (
      <TabItem title="t" onClick={onClick} onClose={onClose} />
    ));
    const close = container.querySelector<HTMLButtonElement>(".tab-item-close")!;
    fireEvent.click(close);
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onClick).not.toHaveBeenCalled();
  });

  it("restore click does not bubble to the row's onClick", () => {
    const onClick = vi.fn();
    const onRestore = vi.fn();
    const { container } = render(() => (
      <TabItem title="t" onClick={onClick} onRestore={onRestore} />
    ));
    const restore = container.querySelector<HTMLButtonElement>(".tab-item-action")!;
    fireEvent.click(restore);
    expect(onRestore).toHaveBeenCalledTimes(1);
    expect(onClick).not.toHaveBeenCalled();
  });

  it("CSS reveals actions on :focus-within in addition to :hover", () => {
    expect(tabItemCss).toMatch(/\.tab-item:focus-within\s+\.tab-item-actions/);
    expect(tabItemCss).toMatch(/\.tab-item:hover\s+\.tab-item-actions/);
  });
});
