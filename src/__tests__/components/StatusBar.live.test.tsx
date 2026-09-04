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
  return { status, setStatus };
});

vi.mock("../../stores/global/save-status", () => ({
  saveStatusStore: { forNote: (): NoteSaveStatus => fixtures.status() },
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

// StatusBar and its preview controls reach into per-window state via
// useWindow(); this test renders StatusBar without a WindowProvider and is
// scoped to the save-status live region, so stub the window context and the
// window-bound widgets out.
vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    editor: {
      largeFileMode: () => null,
      cursorLine: () => 1,
      cursorCol: () => 1,
      language: () => null,
      currentText: () => "",
    },
    tabs: { activeTabId: () => "note-1" },
  }),
}));
vi.mock("../../components/Editor/TokenEstimate", () => ({
  default: () => null,
}));
vi.mock("../../components/Preview/PreviewLayoutToggle", () => ({
  default: () => null,
}));
vi.mock("../../components/Preview/PreviewScriptsToggle", () => ({
  default: () => null,
}));

import StatusBar from "../../components/Editor/StatusBar";

describe("StatusBar persistent live region (#50)", () => {
  afterEach(() => {
    fixtures.setStatus({ state: "clean", fileName: "one.md" });
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

  it("names the file it is reporting on, in every state", () => {
    const { container } = render(() => <StatusBar />);
    const label = () => container.querySelector(".statusbar-label")!.textContent;

    fixtures.setStatus({ state: "dirty", fileName: "notes.md" });
    expect(label()).toBe("Unsaved changes in notes.md");

    fixtures.setStatus({ state: "saving", fileName: "notes.md" });
    expect(label()).toBe("Saving notes.md");

    fixtures.setStatus({ state: "saved", fileName: "notes.md" });
    expect(label()).toBe("Saved notes.md");

    fixtures.setStatus({
      state: "failed",
      fileName: "notes.md",
      reason: { code: "ERR_PERMISSION_DENIED", message: "no permission", retryable: true },
    });
    expect(label()).toBe("Couldn't save notes.md");
  });

  it("failure marks the pill is-failed and carries the reason without lengthening the label", () => {
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
    expect(pill.getAttribute("title")).toBe("you do not have permission to change this file.");
    expect(pill.querySelector(".statusbar-label")!.textContent).toBe("Couldn't save one.md");
  });

  it("leaves the announced text alone while the note stays dirty", () => {
    const { container } = render(() => <StatusBar />);
    fixtures.setStatus({ state: "dirty", fileName: "one.md" });
    const first = container.querySelector(".statusbar-label")!.firstChild;

    // A second keystroke leaves the note dirty. The same text node has to
    // survive unwritten, or a screen reader announces the state again.
    fixtures.setStatus({ state: "dirty", fileName: "one.md" });

    expect(container.querySelector(".statusbar-label")!.firstChild).toBe(first);
    expect(container.querySelector(".statusbar-label")!.textContent).toBe(
      "Unsaved changes in one.md",
    );
  });
});
