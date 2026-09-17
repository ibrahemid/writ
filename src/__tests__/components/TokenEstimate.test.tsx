import { describe, it, expect, afterEach, vi } from "vitest";
import { render, cleanup, waitFor } from "@solidjs/testing-library";

const h = vi.hoisted(() => ({ estimate: vi.fn().mockResolvedValue(1234) }));

vi.mock("../../services/tauri", () => ({ promptEstimateTokens: h.estimate }));
vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: { getActive: () => ({ editor: { currentText: () => "some text" } }) },
}));

import TokenEstimate from "../../components/Editor/TokenEstimate";

afterEach(cleanup);

// "tok" is an abbreviation nobody outside AI tooling reads, and the label a
// screen reader gets is a sentence like every other one in the bar.
describe("the token estimate", () => {
  it("names the unit in full and reads as a sentence", async () => {
    vi.useFakeTimers();
    const { container } = render(() => <TokenEstimate />);
    await vi.advanceTimersByTimeAsync(600);
    vi.useRealTimers();

    const field = await waitFor(() => {
      const el = container.querySelector(".statusbar-tokens");
      expect(el).not.toBeNull();
      return el!;
    });
    expect(field.textContent).toContain("tokens");
    expect(field.textContent).not.toMatch(/tok$/);
    expect(field.getAttribute("aria-label")).toBe("Estimated 1.2k tokens");
  });
});
