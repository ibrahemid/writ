import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";

const fixtures = await vi.hoisted(async () => {
  const { createSignal } = await import("solid-js");
  const [status, setStatus] = createSignal<"idle" | "saved" | "failed">("idle");
  return { status, setStatus };
});

vi.mock("../../stores/global/save-status", () => ({
  saveStatusStore: { status: fixtures.status },
}));

vi.mock("../../commands/registry", () => ({
  useCommand: () => undefined,
}));

vi.mock("../../commands/keybindings", () => ({
  useEffectiveBinding: () => null,
}));

vi.mock("../../components/Kbd/Kbd", () => ({
  default: () => null,
}));

// The preview controls reach into per-window state via useWindow(); this
// test renders StatusBar without a WindowProvider and is scoped to the
// save-status live region, so stub them out.
vi.mock("../../components/Preview/PreviewLayoutToggle", () => ({
  default: () => null,
}));
vi.mock("../../components/Preview/PreviewScriptsToggle", () => ({
  default: () => null,
}));

import StatusBar from "../../components/Editor/StatusBar";

describe("StatusBar persistent live region (#50)", () => {
  afterEach(() => {
    fixtures.setStatus("idle");
    cleanup();
  });

  it("renders the live region wrapper at mount, even when status is idle", () => {
    const { container } = render(() => <StatusBar />);
    const live = container.querySelector<HTMLElement>(".statusbar-live");
    expect(live).not.toBeNull();
    expect(live!.getAttribute("role")).toBe("status");
    expect(live!.getAttribute("aria-live")).toBe("polite");
    expect(container.querySelector(".statusbar-save")).toBeNull();
  });

  it("first save transition mounts the pill inside the existing live region", () => {
    const { container } = render(() => <StatusBar />);
    const liveBefore = container.querySelector<HTMLElement>(".statusbar-live")!;
    expect(liveBefore.querySelector(".statusbar-save")).toBeNull();

    fixtures.setStatus("saved");

    const liveAfter = container.querySelector<HTMLElement>(".statusbar-live")!;
    expect(liveAfter).toBe(liveBefore);
    expect(liveAfter.querySelector(".statusbar-save")).not.toBeNull();
  });

  it("failure marks the pill is-failed inside the same live region", () => {
    const { container } = render(() => <StatusBar />);
    fixtures.setStatus("failed");
    const pill = container.querySelector<HTMLElement>(".statusbar-save")!;
    expect(pill).not.toBeNull();
    expect(pill.classList.contains("is-failed")).toBe(true);
  });
});
