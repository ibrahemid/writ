import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import type { UpdatePhase } from "../../types/update";

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

describe("UpdateBanner persistent live region (#50)", () => {
  afterEach(() => {
    fixtures.setPhase({ status: "idle" });
    cleanup();
  });

  it("renders the live region wrapper at mount, even when phase is idle", () => {
    const { container } = render(() => <UpdateBanner />);
    const live = container.querySelector<HTMLElement>(".update-banner-live");
    expect(live).not.toBeNull();
    expect(live!.getAttribute("role")).toBe("status");
    expect(live!.getAttribute("aria-live")).toBe("polite");
    expect(container.querySelector(".update-banner")).toBeNull();
  });

  it("checking phase mounts the banner inside the existing live region", () => {
    const { container } = render(() => <UpdateBanner />);
    const liveBefore = container.querySelector<HTMLElement>(".update-banner-live")!;
    expect(liveBefore.querySelector(".update-banner")).toBeNull();

    fixtures.setPhase({ status: "checking" });

    const liveAfter = container.querySelector<HTMLElement>(".update-banner-live")!;
    expect(liveAfter).toBe(liveBefore);
    expect(liveAfter.querySelector(".update-banner")).not.toBeNull();
  });

  it("failed phase renders error copy inside the same live region", () => {
    const { container } = render(() => <UpdateBanner />);
    fixtures.setPhase({ status: "failed", message: "network down" });
    const live = container.querySelector<HTMLElement>(".update-banner-live")!;
    expect(live).not.toBeNull();
    const err = live.querySelector(".update-banner-error");
    expect(err).not.toBeNull();
  });
});
