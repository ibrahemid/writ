import { describe, it, expect, afterEach, vi } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import type { DocumentCounts } from "../../stores/window/counts-store";

const h = vi.hoisted(() => ({
  counts: vi.fn((): DocumentCounts | null => ({ words: 1000, characters: 4400, cjk: 0, arabic: 0, exact: true })),
  watch: vi.fn(() => () => {}),
}));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({ counts: { counts: h.counts, watch: h.watch } }),
}));

import TokenEstimate from "../../components/Editor/TokenEstimate";

afterEach(cleanup);

// "tok" is an abbreviation nobody outside AI tooling reads, and the label a
// screen reader gets is a sentence like every other one in the bar.
describe("the token estimate", () => {
  it("names the unit in full and reads as a sentence", () => {
    const { container } = render(() => <TokenEstimate />);

    const field = container.querySelector(".statusbar-tokens");
    expect(field).not.toBeNull();
    expect(field!.textContent).toContain("tokens");
    expect(field!.textContent).not.toMatch(/tok$/);
    expect(field!.getAttribute("aria-label")).toBe("Estimated 1.2k tokens");
  });

  it("keeps the document's counts current while it shows", () => {
    h.watch.mockClear();
    render(() => <TokenEstimate />);
    expect(h.watch).toHaveBeenCalledOnce();
  });

  it("shows nothing for a document with no words, or before the counts are in", () => {
    h.counts.mockReturnValueOnce({ words: 0, characters: 5, cjk: 0, arabic: 0, exact: true });
    const empty = render(() => <TokenEstimate />);
    expect(empty.container.querySelector(".statusbar-tokens")).toBeNull();
    cleanup();

    h.counts.mockReturnValueOnce(null);
    const pending = render(() => <TokenEstimate />);
    expect(pending.container.querySelector(".statusbar-tokens")).toBeNull();
  });
});
