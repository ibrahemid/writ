import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import type { NoteSaveStatus } from "../../stores/global/save-status";

const fixtures = await vi.hoisted(async () => {
  const { createSignal } = await import("solid-js");
  const [status, setStatus] = createSignal<{
    state: "clean" | "dirty" | "saving" | "saved" | "failed";
    fileName: string;
    reason?: { code: string | null; message: string; retryable: boolean };
  }>({ state: "clean", fileName: "one.md" });
  const [updated, setUpdated] = createSignal(false);
  const [by, setBy] = createSignal<string | null>(null);
  return { status, setStatus, updated, setUpdated, by, setBy };
});

vi.mock("../../stores/global/save-status", () => ({
  saveStatusStore: { forNote: (): NoteSaveStatus => fixtures.status() },
}));

// StatusBar and its preview controls reach into per-window state via
// useWindow(); this test renders StatusBar without a WindowProvider and is
// scoped to the save-status live region, so stub the window context and the
// window-bound widgets out.
vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    editor: {
      largeFileMode: () => null,
      currentBufferId: () => "note-1",
      tierOffer: () => null,
      longestLine: () => 0,
      overridesOf: () => ({}),
      cursorLine: () => 1,
      cursorCol: () => 1,
      language: () => null,
      isUpdatedFromDisk: () => fixtures.updated(),
      updatedFromDiskBy: () => (fixtures.updated() ? fixtures.by() : null),
    },
    counts: { counts: () => null, watch: () => () => {} },
    tabs: { activeTabId: () => "note-1" },
  }),
}));
vi.mock("../../components/Editor/TokenEstimate", () => ({
  default: () => null,
}));
vi.mock("../../components/Preview/PreviewLayoutToggle", () => ({
  default: () => null,
}));

import StatusBar from "../../components/Editor/StatusBar";

describe("StatusBar persistent live region (#50)", () => {
  afterEach(() => {
    fixtures.setStatus({ state: "clean", fileName: "one.md" });
    fixtures.setUpdated(false);
    fixtures.setBy(null);
    cleanup();
  });

  it("renders the live region wrapper at mount, with a clean note saying nothing", () => {
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

    fixtures.setStatus({ state: "saved", fileName: "one.md" });

    const liveAfter = container.querySelector<HTMLElement>(".statusbar-live")!;
    expect(liveAfter).toBe(liveBefore);
    expect(liveAfter.querySelector(".statusbar-save")).not.toBeNull();
  });

  it("names the file it is reporting on when a save is asked for or fails", () => {
    const { container } = render(() => <StatusBar />);
    const label = () => container.querySelector(".statusbar-label")!.textContent;

    fixtures.setStatus({ state: "saved", fileName: "notes.md" });
    expect(label()).toBe("Saved notes.md");

    fixtures.setStatus({
      state: "failed",
      fileName: "notes.md",
      reason: { code: "ERR_PERMISSION_DENIED", message: "no permission", retryable: true },
    });
    expect(label()).toBe("Could not save notes.md");
  });

  // Autosave runs on every pause. The tab's mark already says a note is
  // unsaved, so the corner stays quiet and the live region announces nothing.
  it("says nothing while autosave works", () => {
    const { container } = render(() => <StatusBar />);

    fixtures.setStatus({ state: "dirty", fileName: "one.md" });
    expect(container.querySelector(".statusbar-save")).toBeNull();

    fixtures.setStatus({ state: "saving", fileName: "one.md" });
    expect(container.querySelector(".statusbar-save")).toBeNull();
    expect(container.querySelector(".statusbar-live")!.textContent).toBe("");
  });

  it("marks a failure is-failed and leaves the reason to the bar above", () => {
    const { container } = render(() => <StatusBar />);
    fixtures.setStatus({
      state: "failed",
      fileName: "one.md",
      reason: {
        code: "ERR_PERMISSION_DENIED",
        message: "you do not have permission to change this file.",
        retryable: true,
      },
    });
    const pill = container.querySelector<HTMLElement>(".statusbar-save")!;
    expect(pill.classList.contains("is-failed")).toBe(true);
    expect(pill.hasAttribute("title")).toBe(false);
    expect(pill.querySelector(".statusbar-label")!.textContent).toBe("Could not save one.md");
  });

  it("says one thing at a time when a note is both unsaved and just reloaded", () => {
    // Two states in one region read as neither, and the reload is the newer
    // fact: it is what just happened, and the text on screen came from it.
    fixtures.setStatus({ state: "dirty", fileName: "one.md" });
    fixtures.setUpdated(true);

    const { container } = render(() => <StatusBar />);

    const live = container.querySelector<HTMLElement>(".statusbar-live")!;
    expect(live.textContent).toBe("Updated from disk");
    expect(container.querySelector(".statusbar-save")).toBeNull();
  });

  it("names the program when a connected program wrote the file", () => {
    fixtures.setUpdated(true);
    fixtures.setBy("Claude Code");

    const { container } = render(() => <StatusBar />);

    expect(container.querySelector(".statusbar-live")!.textContent).toBe("Changed by Claude Code");
  });

  it("hands the region back to the save state once the marker has gone", () => {
    fixtures.setStatus({ state: "failed", fileName: "one.md" });
    const { container } = render(() => <StatusBar />);

    expect(container.querySelector(".statusbar-save")?.textContent).toBe("Could not save one.md");
    expect(container.querySelector(".statusbar-updated")).toBeNull();
  });
});
