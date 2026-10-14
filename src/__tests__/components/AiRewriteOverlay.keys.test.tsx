import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import { createRoot, createSignal } from "solid-js";
import type { WordSpan } from "../../services/tauri";

// The rewrite preview with the editor holding focus: what each chord does in
// each state, and a finished result read with its changes marked in place.

const h = vi.hoisted(() => ({
  apply: vi.fn(),
  retry: vi.fn(),
  discard: vi.fn(),
  submitInstruction: vi.fn(),
}));

const [status, setStatus] = createRoot(() =>
  createSignal<"awaiting-instruction" | "streaming" | "done" | "error">("done"),
);
const [changes, setChanges] = createRoot(() => createSignal<WordSpan[] | null>(null));

vi.mock("../../stores/global/ai-rewrite", () => ({
  aiRewriteStore: {
    isOpen: () => true,
    status: () => status(),
    changes: () => changes(),
    actionLabel: () => "Proofread",
    original: () => "The launch slips to March.",
    result: () => "The launch moves to April.",
    errorMessage: () => "The model did not answer.",
    instruction: () => "shorter",
    setInstruction: vi.fn(),
    submitInstruction: h.submitInstruction,
    discard: h.discard,
    apply: h.apply,
    retry: h.retry,
  },
}));

import AiRewriteOverlay from "../../components/AiRewrite/AiRewriteOverlay";

beforeEach(() => {
  for (const mock of Object.values(h)) mock.mockReset();
  setStatus("done");
  setChanges(null);
});

afterEach(() => cleanup());

function pressRun(): void {
  fireEvent.keyDown(window, { key: "Enter", metaKey: true });
}

describe("the rewrite preview's chords", () => {
  it("Escape discards in every state", () => {
    for (const state of ["awaiting-instruction", "streaming", "done", "error"] as const) {
      setStatus(state);
      render(() => <AiRewriteOverlay />);
      fireEvent.keyDown(window, { key: "Escape" });
      cleanup();
    }
    expect(h.discard).toHaveBeenCalledTimes(4);
  });

  it("the run chord applies only a finished result", () => {
    render(() => <AiRewriteOverlay />);
    setStatus("streaming");
    pressRun();
    expect(h.apply).not.toHaveBeenCalled();
    setStatus("done");
    pressRun();
    expect(h.apply).toHaveBeenCalledTimes(1);
  });

  it("the run chord retries a failure, so Retry is reachable from the editor", () => {
    setStatus("error");
    render(() => <AiRewriteOverlay />);
    pressRun();
    expect(h.retry).toHaveBeenCalledTimes(1);
    expect(h.apply).not.toHaveBeenCalled();
  });

  it("Enter in the instruction field runs it", () => {
    setStatus("awaiting-instruction");
    const { getByLabelText } = render(() => <AiRewriteOverlay />);
    fireEvent.keyDown(getByLabelText("Rewrite instruction"), { key: "Enter" });
    expect(h.submitInstruction).toHaveBeenCalledTimes(1);
  });

  it("Apply and Retry carry their chord and keep their names", () => {
    const { getByRole } = render(() => <AiRewriteOverlay />);
    const apply = getByRole("button", { name: "Apply" });
    expect(apply.querySelector(".ai-overlay-chord")?.textContent).not.toBe("");
    expect(apply.getAttribute("aria-keyshortcuts")).toContain("Enter");
    cleanup();
    setStatus("error");
    const again = render(() => <AiRewriteOverlay />).getByRole("button", { name: "Retry" });
    expect(again.querySelector(".ai-overlay-chord")).not.toBeNull();
  });
});

describe("a finished rewrite", () => {
  it("reads as one text with the changed words marked", () => {
    setChanges([
      { kind: "context", text: "The launch " },
      { kind: "removed", text: "slips" },
      { kind: "added", text: "moves" },
      { kind: "context", text: " to " },
      { kind: "removed", text: "March." },
      { kind: "added", text: "April." },
    ]);
    const { container } = render(() => <AiRewriteOverlay />);
    expect([...container.querySelectorAll("del")].map((el) => el.textContent)).toEqual([
      "slips",
      "March.",
    ]);
    expect([...container.querySelectorAll("ins")].map((el) => el.textContent)).toEqual([
      "moves",
      "April.",
    ]);
    expect(container.querySelector(".ai-overlay-panes")).toBeNull();
  });

  it("shows the two texts side by side until the comparison answers", () => {
    const { container } = render(() => <AiRewriteOverlay />);
    expect(container.querySelector(".ai-overlay-panes")).not.toBeNull();
    expect(container.querySelector("del")).toBeNull();
  });
});
