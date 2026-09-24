import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";
import { Show } from "solid-js";
import WindowProvider, { useWindow } from "../../components/WindowProvider/WindowProvider";
import type { WindowState } from "../../stores/window/createWindowState";
import type { BufferDocument } from "../../types/buffer";

const mocks = vi.hoisted(() => ({
  firstRunState: vi.fn(),
  finishFirstRun: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  firstRunState: mocks.firstRunState,
  finishFirstRun: mocks.finishFirstRun,
  dismissFirstRunHint: vi.fn().mockResolvedValue(undefined),
  autoRetitleNote: vi.fn().mockResolvedValue({ kind: "skipped" }),
  listActiveBuffers: vi.fn().mockResolvedValue([]),
  listHistory: vi.fn().mockResolvedValue([]),
  getBuffer: vi.fn(),
  renameNote: vi.fn(),
  renameNoteWithLinks: vi.fn(),
  previewClose: vi.fn().mockResolvedValue(undefined),
  previewListRenderers: vi.fn().mockResolvedValue([]),
  previewGetLayout: vi.fn().mockResolvedValue(null),
  previewSetLayout: vi.fn().mockResolvedValue(undefined),
  getConfig: vi.fn(),
  updateConfig: vi.fn().mockResolvedValue(undefined),
}));

// The store is the app's singleton and reads the launch once, and the Apps
// step has no way back to the format step. Each test gets a store of its own
// behind the singleton's name, so every one starts on the format step.
vi.mock("../../stores/global/first-run", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../stores/global/first-run")>();
  const { createRoot } = await import("solid-js");
  let current = createRoot(actual.createFirstRunStore);
  const firstRunStore = new Proxy({} as typeof current, {
    get: (_target, key) => current[key as keyof typeof current],
  });
  return {
    ...actual,
    firstRunStore,
    renewFirstRunStore: () => {
      current = createRoot(actual.createFirstRunStore);
    },
  };
});

import FirstRunSetup from "../../components/FirstRun/FirstRunSetup";
import * as firstRun from "../../stores/global/first-run";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { configStore } from "../../stores/global/config";

const { firstRunStore } = firstRun;
const renewFirstRunStore = (firstRun as unknown as { renewFirstRunStore: () => void })
  .renewFirstRunStore;

const DOC = {
  id: "note-first",
  title: "Untitled.md",
  filename: "Untitled.md",
  status: "active",
  source_path: "/notes/Untitled.md",
} as unknown as BufferDocument;

let win: WindowState | null = null;

function CaptureWindow() {
  win = useWindow();
  return null;
}

function mount() {
  return render(() => (
    <WindowProvider windowId={8802}>
      <CaptureWindow />
      <Show when={firstRunStore.step() !== null}>
        <FirstRunSetup />
      </Show>
    </WindowProvider>
  ));
}

function continueButton(container: Element): HTMLButtonElement {
  const button = Array.from(container.querySelectorAll("button")).find(
    (el) => el.textContent === "Continue",
  );
  if (!button) throw new Error("no Continue");
  return button;
}

/** The format step's Continue, which moves to the Apps step and writes nothing. */
async function continueToApps(container: Element): Promise<void> {
  fireEvent.click(continueButton(container));
  await waitFor(() => expect(firstRunStore.step()).toBe("apps"));
  expect(mocks.finishFirstRun).not.toHaveBeenCalled();
}

function option(container: Element, format: string): HTMLElement {
  const el = container.querySelector<HTMLElement>(`[data-format='${format}']`);
  if (!el) throw new Error(`no ${format} option`);
  return el;
}

describe("the format the first launch asks about", () => {
  beforeEach(() => {
    mocks.firstRunState.mockReset().mockResolvedValue({
      first_run: true,
      hint_dismissed: false,
      file_manager: "Finder",
    });
    mocks.finishFirstRun.mockReset().mockResolvedValue(null);
    renewFirstRunStore();
    win = null;
  });

  afterEach(() => cleanup());

  it("stays off the screen while the launch has nothing to ask", async () => {
    mocks.firstRunState.mockResolvedValue({
      first_run: false,
      hint_dismissed: false,
      file_manager: "Finder",
    });
    await firstRunStore.load();
    const { container } = mount();
    expect(firstRunStore.step()).toBeNull();
    expect(container.querySelector(".first-run-setup")).toBeNull();
  });

  it("offers the two formats with plain text already chosen", async () => {
    await firstRunStore.load();
    const { container } = mount();

    await waitFor(() => expect(container.querySelector(".first-run-setup")).not.toBeNull());
    expect(option(container, "txt").textContent).toBe(
      "Plain text (.txt)No markup. Opens in any editor.",
    );
    expect(option(container, "md").textContent).toBe(
      "Markdown (.md)Headings, lists and links, with a rendered view.",
    );
    expect(option(container, "txt").getAttribute("aria-checked")).toBe("true");
    expect(option(container, "md").getAttribute("aria-checked")).toBe("false");
  });

  it("moves between the two options with the arrow keys", async () => {
    await firstRunStore.load();
    const { container } = mount();

    fireEvent.keyDown(option(container, "txt"), { key: "ArrowRight" });
    await waitFor(() => expect(option(container, "md").getAttribute("aria-checked")).toBe("true"));
    expect(option(container, "md").getAttribute("tabindex")).toBe("0");
    expect(option(container, "txt").getAttribute("tabindex")).toBe("-1");

    fireEvent.keyDown(option(container, "md"), { key: "ArrowLeft" });
    await waitFor(() => expect(option(container, "txt").getAttribute("aria-checked")).toBe("true"));
  });

  // The screen is the only place the answer exists until Continue lands, so a
  // failed call leaves it up rather than dropping the question.
  it("keeps the screen up when the answer could not be recorded", async () => {
    await firstRunStore.load();
    mocks.finishFirstRun.mockRejectedValue(new Error("no IPC"));
    const consoleSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const { container } = mount();

    fireEvent.click(option(container, "md"));
    await continueToApps(container);
    fireEvent.click(continueButton(container));

    await waitFor(() => expect(mocks.finishFirstRun).toHaveBeenCalledWith("md", []));
    expect(container.querySelector(".first-run-setup")).not.toBeNull();
    expect(firstRunStore.step()).toBe("apps");
    expect(firstRunStore.format()).toBe("md");
    consoleSpy.mockRestore();
  });

  it("is the one thing on the window: a dialog, named by its own heading", async () => {
    await firstRunStore.load();
    const { container } = mount();

    const panel = container.querySelector<HTMLElement>(".first-run-setup-panel");
    expect(panel?.getAttribute("role")).toBe("dialog");
    expect(panel?.getAttribute("aria-modal")).toBe("true");
    const heading = panel?.getAttribute("aria-labelledby");
    expect(heading).not.toBeNull();
    expect(container.querySelector(`#${heading}`)?.textContent).toBe("Default format");
  });

  it("selects the focused option with Space and answers with Enter", async () => {
    await firstRunStore.load();
    // The answer is refused, so the screen stays up.
    mocks.finishFirstRun.mockRejectedValue(new Error("no IPC"));
    const consoleSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const { container } = mount();

    // The options rove: focus sits on the chosen one, so an arrow puts the
    // reader on Markdown and Space keeps it.
    fireEvent.keyDown(option(container, "txt"), { key: "ArrowRight" });
    await waitFor(() => expect(document.activeElement).toBe(option(container, "md")));
    fireEvent.keyDown(option(container, "md"), { key: " " });
    await waitFor(() => expect(option(container, "md").getAttribute("aria-checked")).toBe("true"));
    expect(option(container, "txt").getAttribute("aria-checked")).toBe("false");
    expect(mocks.finishFirstRun).not.toHaveBeenCalled();

    fireEvent.keyDown(option(container, "md"), { key: "Enter" });
    await waitFor(() => expect(firstRunStore.step()).toBe("apps"));
    expect(mocks.finishFirstRun).not.toHaveBeenCalled();

    fireEvent.keyDown(container.querySelector(".first-run-apps")!, { key: "Enter" });
    await waitFor(() => expect(mocks.finishFirstRun).toHaveBeenCalledWith("md", []));
    consoleSpy.mockRestore();
  });

  // A held Enter repeats inside one round trip. Two answers would leave the
  // folder with a writ-<yymmdd>-<hhmm>-2 nobody asked for.
  it("takes one answer however many times Continue is pressed", async () => {
    await firstRunStore.load();
    let refuse: (reason: Error) => void = () => {};
    mocks.finishFirstRun.mockImplementation(
      () =>
        new Promise<BufferDocument | null>((_resolve, reject) => {
          refuse = reject;
        }),
    );
    const consoleSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const { container } = mount();
    await continueToApps(container);
    const button = continueButton(container);

    fireEvent.click(button);
    await waitFor(() => expect(button.getAttribute("aria-busy")).toBe("true"));
    expect(button.hasAttribute("disabled")).toBe(true);
    fireEvent.click(button);
    fireEvent.keyDown(container.querySelector(".first-run-apps")!, { key: "Enter" });

    refuse(new Error("no IPC"));
    await waitFor(() => expect(button.hasAttribute("disabled")).toBe(false));
    expect(mocks.finishFirstRun).toHaveBeenCalledTimes(1);
    expect(container.querySelector(".first-run-setup")).not.toBeNull();
    consoleSpy.mockRestore();
  });

  it("records the chosen format, opens the note it answers with, and leaves", async () => {
    await firstRunStore.load();
    mocks.finishFirstRun.mockResolvedValue(DOC);
    const { container } = mount();

    fireEvent.click(option(container, "md"));
    await continueToApps(container);
    fireEvent.click(continueButton(container));

    await waitFor(() => expect(container.querySelector(".first-run-setup")).toBeNull());
    expect(mocks.finishFirstRun).toHaveBeenCalledWith("md", []);
    expect(configStore.config().files.default_extension).toBe("md");
    expect(bufferRegistry.activeTabs().map((b) => b.id)).toContain(DOC.id);
    expect(win!.tabs.activeTabId()).toBe(DOC.id);
  });
});
