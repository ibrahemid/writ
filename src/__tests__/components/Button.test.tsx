import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import Button from "../../components/Button/Button";

function buttonIn(container: HTMLElement): HTMLButtonElement {
  const el = container.querySelector("button");
  if (!el) throw new Error("no button rendered");
  return el;
}

describe("Button", () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("defaults to a secondary button element", () => {
    const { container } = render(() => <Button>Save</Button>);
    const button = buttonIn(container);
    expect(button.type).toBe("button");
    expect(button.className.split(" ")).toContain("writ-btn-secondary");
  });

  it("primary carries the accent classes", () => {
    const { container } = render(() => <Button variant="primary">New note</Button>);
    expect(buttonIn(container).className.split(" ")).toContain("writ-btn-primary");
  });

  it("ghost carries the ghost class", () => {
    const { container } = render(() => <Button variant="ghost">Close</Button>);
    expect(buttonIn(container).className.split(" ")).toContain("writ-btn-ghost");
  });

  it("danger stacks onto the variant", () => {
    const { container } = render(() => (
      <Button variant="secondary" danger>
        Delete
      </Button>
    ));
    const classes = buttonIn(container).className.split(" ");
    expect(classes).toContain("writ-btn-secondary");
    expect(classes).toContain("writ-btn-danger");
  });

  it("forwards onClick", () => {
    const onClick = vi.fn();
    const { container } = render(() => <Button onClick={onClick}>Save</Button>);
    fireEvent.click(buttonIn(container));
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it("disabled does not fire onClick", () => {
    const onClick = vi.fn();
    const { container } = render(() => (
      <Button onClick={onClick} disabled>
        Save
      </Button>
    ));
    const button = buttonIn(container);
    expect(button.disabled).toBe(true);
    fireEvent.click(button);
    expect(onClick).not.toHaveBeenCalled();
  });

  it("renders the icon slot before the label", () => {
    const { container } = render(() => (
      <Button icon="note-pencil" variant="primary">
        New note
      </Button>
    ));
    const use = container.querySelector("use");
    expect(use?.getAttribute("href")).toBe("#ph-note-pencil");
    expect(buttonIn(container).firstElementChild?.tagName.toLowerCase()).toBe("svg");
  });

  it("an icon-only button keeps its accessible name", () => {
    const { container } = render(() => <Button icon="x" aria-label="Close tab" />);
    expect(buttonIn(container).getAttribute("aria-label")).toBe("Close tab");
  });

  it("reports an icon-only button that has no name", () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    render(() => <Button icon="x" />);
    expect(error).toHaveBeenCalledWith("[writ] an icon-only button was rendered without a name");
  });
});
