import { describe, it, expect, afterEach, vi } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import PreviewStatusChip, {
  RENDERING_SHOWN_AFTER_MS,
  type PreviewState,
} from "../../components/Preview/PreviewStatusChip";

const menu = vi.hoisted(() => ({ showAnchoredMenu: vi.fn() }));
vi.mock("../../components/ContextMenu/ContextMenu", () => ({
  showAnchoredMenu: menu.showAnchoredMenu,
  isMenuOpenFor: () => false,
}));

// The chip is now a transient status indicator: silent in the steady OK
// state, visible only for rendering / size-gate / error / warnings.

describe("PreviewStatusChip — transient status indicator", () => {
  afterEach(() => {
    vi.useRealTimers();
    menu.showAnchoredMenu.mockClear();
    cleanup();
  });

  it("renders nothing in the OK state with no warnings", () => {
    const { container } = render(() => (
      <PreviewStatusChip state="ok" warnings={[]} message="" />
    ));
    expect(container.querySelector(".preview-chip")).toBeNull();
  });

  // A render that lands quickly says nothing: the label flashing on every
  // typing pause in a split was noise.
  it("shows the rendering label only once a render passes the delay", async () => {
    vi.useFakeTimers();
    const { container } = render(() => (
      <PreviewStatusChip state="rendering" warnings={[]} message="" />
    ));
    expect(container.querySelector(".preview-chip")).toBeNull();

    await vi.advanceTimersByTimeAsync(RENDERING_SHOWN_AFTER_MS - 1);
    expect(container.querySelector(".preview-chip")).toBeNull();

    await vi.advanceTimersByTimeAsync(1);
    expect(container.querySelector(".preview-chip")!.textContent).toContain("Rendering");
  });

  it("announces errors and nothing else", async () => {
    vi.useFakeTimers();
    const [state, setState] = createSignal<PreviewState>("rendering");
    const { container } = render(() => (
      <PreviewStatusChip state={state()} warnings={[]} message="Parse error on line 3" />
    ));
    const live = container.querySelector<HTMLElement>('[aria-live="polite"]')!;
    await vi.advanceTimersByTimeAsync(RENDERING_SHOWN_AFTER_MS);
    expect(live.textContent).toBe("");
    expect(container.querySelector(".preview-chip")!.getAttribute("aria-live")).toBeNull();

    setState("error");
    expect(live.textContent).toBe("Could not render: Parse error on line 3");
  });

  it("opens the whole message and the warnings from the chip", () => {
    const { container } = render(() => (
      <PreviewStatusChip
        state="error"
        warnings={["line 4: unknown node"]}
        message="Mermaid could not parse the diagram on line 2"
      />
    ));
    const chip = container.querySelector<HTMLButtonElement>("button.preview-chip")!;
    expect(chip.classList.contains("is-error")).toBe(true);
    expect(chip.textContent).toContain("Could not render");
    expect(chip.textContent).toContain("1 warning");
    expect(chip.querySelector("[title]")).toBeNull();

    chip.click();
    const items = menu.showAnchoredMenu.mock.calls[0][1] as { label: string; note?: boolean }[];
    expect(items.map((item) => item.label)).toEqual([
      "Mermaid could not parse the diagram on line 2",
      "line 4: unknown node",
    ]);
    expect(items.every((item) => item.note)).toBe(true);
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

  // The pane's overlay states the cap with the file's size and carries the
  // way out, so the chip says nothing a second time.
  it("leaves a document past the cap to the pane's overlay", () => {
    const { container } = render(() => (
      <PreviewStatusChip state="too_large" warnings={[]} message="" />
    ));
    expect(container.querySelector(".preview-chip")).toBeNull();
  });

  it("is the only place the size gate is stated", () => {
    const pane = readFileSync(
      resolve(process.cwd(), "src/components/Preview/PreviewPane.tsx"),
      "utf8",
    );
    const overlay = pane.match(/<div class="preview-pane-overlay">([\s\S]*?)<\/div>/);
    expect(overlay, "the pane draws the size-gate overlay").toBeTruthy();
    expect(overlay![1]).toContain("render_refuse_threshold_mb");
    expect(overlay![1]).toContain("Switch to Source");
  });
});
