import { describe, it, expect, vi, afterEach, beforeAll } from "vitest";
import { render, cleanup, waitFor } from "@solidjs/testing-library";
import { createSignal } from "solid-js";

beforeAll(() => {
  if (!(Element.prototype as { scrollIntoView?: () => void }).scrollIntoView) {
    Element.prototype.scrollIntoView = function scrollIntoView() {};
  }
});

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({ editor: { focusEditor: vi.fn() } }),
}));

import Palette from "../../components/Palette/Palette";
import type { ResultProvider } from "../../components/Palette/types";

const PROVIDERS: ResultProvider[] = [
  {
    id: "test",
    section: "Test",
    heading: null,
    order: 0,
    cap: 10,
    modes: ["all"],
    showKbd: false,
    query: () => [],
  },
];

function harness(initialQuery: () => string) {
  const [open, setOpen] = createSignal(false);
  const result = render(() => (
    <Palette
      open={open()}
      onClose={() => setOpen(false)}
      providers={PROVIDERS}
      placeholder="Search"
      label="Search"
      inputLabel="Search"
      initialQuery={initialQuery}
    />
  ));
  return { ...result, setOpen };
}

function input(container: HTMLElement) {
  return container.querySelector<HTMLInputElement>("input")!;
}

const flushFrame = () => new Promise((r) => requestAnimationFrame(() => r(null)));

// Each open schedules a frame that focuses (and may select) the input. Flush it
// here, or a previous test's pending frame fires inside the next one and its
// select() lands on whatever spy that test installed.
afterEach(async () => {
  await flushFrame();
  cleanup();
});

describe("palette seeding", () => {
  it("fills the box from the seed on open", async () => {
    const { container, setOpen } = harness(() => "propose");
    setOpen(true);
    await waitFor(() => expect(input(container).value).toBe("propose"));
  });

  it("opens empty when there is no seed", async () => {
    const { container, setOpen } = harness(() => "");
    setOpen(true);
    await waitFor(() => expect(container.querySelector("input")).not.toBeNull());
    expect(input(container).value).toBe("");
  });

  it("does not select text it did not seed", async () => {
    // The reported hazard: an unconditional select() swallows characters typed
    // before the focus frame runs, so "br" becomes "r".
    const { container, setOpen } = harness(() => "");
    const select = vi.spyOn(HTMLInputElement.prototype, "select");
    try {
      setOpen(true);
      await waitFor(() => expect(container.querySelector("input")).not.toBeNull());
      await flushFrame();
      expect(select).not.toHaveBeenCalled();
    } finally {
      select.mockRestore();
    }
  });

  it("selects the seed so typing replaces it", async () => {
    const { container, setOpen } = harness(() => "propose");
    const select = vi.spyOn(HTMLInputElement.prototype, "select");
    try {
      setOpen(true);
      await waitFor(() => expect(input(container).value).toBe("propose"));
      await flushFrame();
      expect(select).toHaveBeenCalled();
    } finally {
      select.mockRestore();
    }
  });

  it("clears the box on close so a reopen starts empty", async () => {
    let seed = "propose";
    const { container, setOpen } = harness(() => seed);
    setOpen(true);
    await waitFor(() => expect(input(container).value).toBe("propose"));

    setOpen(false);
    // What the search palette does on close.
    seed = "";
    setOpen(true);
    await waitFor(() => expect(container.querySelector("input")).not.toBeNull());
    expect(input(container).value).toBe("");
  });
});
