import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import type { UpdatePhase } from "../../types/update";

// What the banner says a failure was, and what a screen reader is handed while
// the bytes are still arriving.

const fixtures = await vi.hoisted(async () => {
  const { createSignal } = await import("solid-js");
  const [phase, setPhase] = createSignal<UpdatePhase>({ status: "idle" });
  return { phase, setPhase };
});

vi.mock("../../stores/global/update", () => ({
  updateStore: {
    phase: fixtures.phase,
    dismiss: vi.fn(),
    install: vi.fn(),
    restart: vi.fn(),
    checkForUpdate: vi.fn(),
  },
}));

import UpdateBanner from "../../components/UpdateBanner/UpdateBanner";

const UNREACHABLE = "Couldn't reach the update server. Try again later.";

afterEach(() => {
  fixtures.setPhase({ status: "idle" });
  cleanup();
});

describe("what the banner says a failure was", () => {
  it("prints the message the failure came with", () => {
    const { container } = render(() => <UpdateBanner />);
    fixtures.setPhase({ status: "failed", message: "The update did not pass its signature check." });

    const text = container.querySelector(".update-banner-error")!.textContent;
    expect(text).toContain("The update did not pass its signature check.");
    expect(text).not.toContain(UNREACHABLE);
  });

  it("falls back to the unreachable line when the failure carries no message", () => {
    const { container } = render(() => <UpdateBanner />);
    fixtures.setPhase({ status: "failed", message: "" });

    expect(container.querySelector(".update-banner-error")!.textContent).toContain(UNREACHABLE);
  });
});

describe("the download as a screen reader hears it", () => {
  it("carries the share done on a progressbar, not in the announcement", () => {
    const { container } = render(() => <UpdateBanner />);
    fixtures.setPhase({ status: "downloading", downloaded: 512_000, total: 1_024_000 });

    const track = container.querySelector('[role="progressbar"]')!;
    expect(track.getAttribute("aria-valuenow")).toBe("50");
    expect(track.getAttribute("aria-valuemax")).toBe("100");
    expect(container.querySelector(".update-banner-amount")!.getAttribute("aria-live")).toBe("off");
  });

  it("says only that it is downloading when the size is unknown", () => {
    const { container } = render(() => <UpdateBanner />);
    fixtures.setPhase({ status: "downloading", downloaded: 512_000, total: null });

    const track = container.querySelector('[role="progressbar"]')!;
    expect(track.getAttribute("aria-valuetext")).toBe("Downloading");
    expect(track.getAttribute("aria-valuenow")).toBeNull();
  });
});

describe("the banner's own controls", () => {
  it("keeps one name for the action that puts the update off", () => {
    const { container } = render(() => <UpdateBanner />);
    const labels: string[] = [];
    for (const phase of [
      { status: "available", version: "0.3.5" },
      { status: "ready" },
      { status: "failed", message: "no" },
    ] as UpdatePhase[]) {
      fixtures.setPhase(phase);
      labels.push(container.querySelector(".writ-btn-ghost")!.textContent!.trim());
    }
    expect(new Set(labels)).toEqual(new Set(["Later"]));
  });

  it("draws its actions with the app's button", () => {
    const { container } = render(() => <UpdateBanner />);
    fixtures.setPhase({ status: "available", version: "0.3.5" });

    expect(container.querySelectorAll(".writ-btn")).toHaveLength(2);
    expect(container.textContent).toContain("Update available: v0.3.5");
    expect(container.textContent).not.toContain("—");
  });
});
