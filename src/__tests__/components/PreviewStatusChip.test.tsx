import { describe, it, expect, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
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
    expect(chip!.textContent).toContain("Rendering");
  });

  it("shows the error state and message", () => {
    const { container } = render(() => (
      <PreviewStatusChip state="error" warnings={[]} message="boom" />
    ));
    const chip = container.querySelector(".preview-chip")!;
    expect(chip.classList.contains("is-error")).toBe(true);
    expect(chip.textContent).toContain("Could not render");
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

  // The key is the one thing the state does not already say, and it is the key
  // the app binds rather than one written into the label.
  it("states the key that renders a document held back for its size", () => {
    const { container } = render(() => (
      <PreviewStatusChip state="manual" warnings={[]} message="" />
    ));
    const chip = container.querySelector(".preview-chip")!;
    expect(chip.textContent).toContain("Too large to render live");
    expect(chip.textContent).not.toContain("—");
  });

  it("says a document past the cap cannot be rendered at all", () => {
    const { container } = render(() => (
      <PreviewStatusChip state="too_large" warnings={[]} message="" />
    ));
    expect(container.querySelector(".preview-chip")!.textContent).toContain(
      "Too large to render",
    );
  });

  // The chip is where a state is stated. The overlay is on screen at the same
  // moment, so it carries the way out and does not say the state a second time.
  it("is the only place the size gate is stated", () => {
    const pane = readFileSync(
      resolve(process.cwd(), "src/components/Preview/PreviewPane.tsx"),
      "utf8",
    );
    const overlay = pane.match(/<div class="preview-pane-overlay">([\s\S]*?)<\/div>/);
    expect(overlay, "the pane draws the size-gate overlay").toBeTruthy();
    expect(overlay![1]).not.toContain("Too large to render");
    expect(overlay![1]).toContain("Use the source view.");
  });
});
