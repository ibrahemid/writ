import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import type { UpdatePhase } from "../../types/update";

// What the banner says a failure was, and what a screen reader is handed while
// the bytes are still arriving.

const fixtures = await vi.hoisted(async () => {
  const { createSignal } = await import("solid-js");
  const [phase, setPhase] = createSignal<UpdatePhase>({ status: "idle" });
  const { vi: hoistedVi } = await import("vitest");
  return { phase, setPhase, openReleaseNotes: hoistedVi.fn() };
});

vi.mock("../../stores/global/update", () => ({
  updateStore: {
    phase: fixtures.phase,
    dismiss: vi.fn(),
    install: vi.fn(),
    restart: vi.fn(),
    checkForUpdate: vi.fn(),
    openReleaseNotes: fixtures.openReleaseNotes,
  },
}));

import UpdateBanner from "../../components/UpdateBanner/UpdateBanner";

const UNREACHABLE = "Couldn't reach the update server.";

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

describe("a failure reads as one", () => {
  it("carries the error rail, not the accent", () => {
    const { container } = render(() => <UpdateBanner />);
    fixtures.setPhase({ status: "failed", message: "network down" });
    expect(container.querySelector(".update-banner")!.classList.contains("is-failed")).toBe(true);
    fixtures.setPhase({ status: "available", version: "0.7.0" });
    expect(container.querySelector(".update-banner")!.classList.contains("is-failed")).toBe(false);
  });
});

describe("what changed in the version on offer", () => {
  it("opens the changelog at that version", () => {
    const { getByRole } = render(() => <UpdateBanner />);
    fixtures.setPhase({ status: "available", version: "0.7.0" });
    getByRole("button", { name: "What's new" }).click();
    expect(fixtures.openReleaseNotes).toHaveBeenCalledWith("0.7.0");
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
      { status: "managed", version: "0.3.5" },
      { status: "ready" },
      { status: "failed", message: "no" },
    ] as UpdatePhase[]) {
      fixtures.setPhase(phase);
      const ghosts = [...container.querySelectorAll(".writ-btn-ghost")].map((b) =>
        b.textContent!.trim(),
      );
      labels.push(ghosts.filter((label) => label !== "What's new").join("|"));
    }
    expect(new Set(labels)).toEqual(new Set(["Later"]));
  });

  it("draws its actions with the app's button", () => {
    const { container } = render(() => <UpdateBanner />);
    fixtures.setPhase({ status: "available", version: "0.3.5" });

    expect(container.querySelectorAll(".writ-btn")).toHaveLength(3);
    expect(container.textContent).toContain("Update available: v0.3.5");
    expect(container.textContent).not.toContain("—");
  });
});

describe("an install a package manager owns", () => {
  it("names the package manager and offers no install", () => {
    const { container } = render(() => <UpdateBanner />);
    fixtures.setPhase({ status: "managed", version: "0.7.0" });

    expect(container.textContent).toContain("v0.7.0 is available. Update with your package manager.");
    expect([...container.querySelectorAll(".writ-btn")].map((b) => b.textContent!.trim())).toEqual([
      "What's new",
      "Later",
    ]);
    expect(container.querySelector(".writ-btn-primary")).toBeNull();
  });
});
