import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";

// The overlay never takes focus, by design: the editor keeps it so an edit can
// abort the preview. A dialog that never takes focus is unreachable in browse
// mode, so it is a region and the result pane announces itself.

const h = await vi.hoisted(async () => {
  const { createSignal } = await import("solid-js");
  const [status, setStatus] = createSignal("done");
  return { status, setStatus };
});

vi.mock("../../stores/global/ai-rewrite", () => ({
  aiRewriteStore: {
    isOpen: () => true,
    status: h.status,
    actionLabel: () => "Rewrite",
    original: () => "before",
    result: () => "after",
    errorMessage: () => "the model refused",
    instruction: () => "",
    setInstruction: vi.fn(),
    submitInstruction: vi.fn(),
    discard: vi.fn(),
    apply: vi.fn(),
    retry: vi.fn(),
  },
}));

import AiRewriteOverlay from "../../components/AiRewrite/AiRewriteOverlay";

afterEach(() => {
  h.setStatus("done");
  cleanup();
});

describe("the rewrite overlay", () => {
  it("is a region, not a dialog it never focuses", () => {
    const { container } = render(() => <AiRewriteOverlay />);
    const overlay = container.querySelector(".ai-overlay")!;
    expect(overlay.getAttribute("role")).toBe("region");
    expect(overlay.getAttribute("aria-label")).toBe("Rewrite preview");
  });

  it("announces the result pane as it fills", () => {
    const { container } = render(() => <AiRewriteOverlay />);
    const result = container.querySelector(".ai-overlay-result")!.closest(".ai-overlay-pane")!;
    expect(result.getAttribute("aria-live")).toBe("polite");
  });

  it("writes a failure as a sentence, not a bare adjective", () => {
    h.setStatus("error");
    const { container } = render(() => <AiRewriteOverlay />);
    expect(container.querySelector(".ai-overlay-status")!.textContent).toBe("Could not rewrite");
  });
});
