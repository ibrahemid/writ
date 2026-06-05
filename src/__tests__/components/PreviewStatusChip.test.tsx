import { describe, it, expect, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import PreviewStatusChip from "../../components/Preview/PreviewStatusChip";

// The chip is now a transient status indicator: silent in the steady OK
// state, visible only for rendering / size-gate / error / warnings.

describe("PreviewStatusChip — transient status indicator", () => {
  afterEach(() => cleanup());

  it("renders nothing in the OK state with no warnings", () => {
    const { container } = render(() => (
      <PreviewStatusChip state="ok" warnings={[]} message="" />
    ));
    expect(container.querySelector(".preview-chip")).toBeNull();
  });

  it("shows a rendering label while rendering", () => {
    const { container } = render(() => (
      <PreviewStatusChip state="rendering" warnings={[]} message="" />
    ));
    const chip = container.querySelector(".preview-chip");
    expect(chip).not.toBeNull();
    expect(chip!.textContent).toContain("rendering");
  });

  it("shows the error state and message", () => {
    const { container } = render(() => (
      <PreviewStatusChip state="error" warnings={[]} message="boom" />
    ));
    const chip = container.querySelector(".preview-chip")!;
    expect(chip.classList.contains("is-error")).toBe(true);
    expect(chip.textContent).toContain("render error");
    expect(chip.textContent).toContain("boom");
  });

  it("surfaces parser warnings even in the OK state", () => {
    const { container } = render(() => (
      <PreviewStatusChip state="ok" warnings={["a", "b"]} message="" />
    ));
    const chip = container.querySelector(".preview-chip")!;
    expect(chip).not.toBeNull();
    expect(chip.querySelector(".preview-chip-warn")!.textContent).toContain("2 warnings");
    // No state label in OK; just the warnings.
    expect(chip.querySelector(".preview-chip-mode")).toBeNull();
  });

  it("does not render the removed scripts flag", () => {
    const { container } = render(() => (
      <PreviewStatusChip state="rendering" warnings={[]} message="" />
    ));
    expect(container.querySelector(".preview-chip-flag")).toBeNull();
  });
});
