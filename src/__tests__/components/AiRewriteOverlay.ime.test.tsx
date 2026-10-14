import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";

// The instruction field takes Japanese, Chinese and Korean text, where Enter
// and Escape belong to the input method until the conversion is confirmed.

const h = vi.hoisted(() => ({ submitInstruction: vi.fn(), discard: vi.fn() }));

vi.mock("../../stores/global/ai-rewrite", () => ({
  aiRewriteStore: {
    isOpen: () => true,
    status: () => "awaiting-instruction",
    actionLabel: () => "Rewrite",
    original: () => "before",
    result: () => "",
    errorMessage: () => "",
    instruction: () => "",
    setInstruction: vi.fn(),
    submitInstruction: h.submitInstruction,
    discard: h.discard,
    apply: vi.fn(),
    retry: vi.fn(),
  },
}));

import AiRewriteOverlay from "../../components/AiRewrite/AiRewriteOverlay";

afterEach(() => {
  h.submitInstruction.mockReset();
  h.discard.mockReset();
  cleanup();
});

describe("the rewrite instruction while an input method composes", () => {
  it("sends nothing on the Enter that confirms a conversion", () => {
    const { getByLabelText } = render(() => <AiRewriteOverlay />);
    const field = getByLabelText("Rewrite instruction");
    fireEvent.keyDown(field, { key: "Enter", isComposing: true });
    fireEvent.keyDown(field, { key: "Enter", keyCode: 229 });
    expect(h.submitInstruction).not.toHaveBeenCalled();

    fireEvent.keyDown(field, { key: "Enter" });
    expect(h.submitInstruction).toHaveBeenCalledTimes(1);
  });

  it("keeps the rewrite on an Escape the input method takes", () => {
    const { getByLabelText } = render(() => <AiRewriteOverlay />);
    fireEvent.keyDown(getByLabelText("Rewrite instruction"), { key: "Escape", isComposing: true });
    expect(h.discard).not.toHaveBeenCalled();
  });
});
