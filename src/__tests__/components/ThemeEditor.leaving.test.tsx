import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";
import { themeStore } from "../../stores/global/theme";
import type { WritConfig } from "../../types/config";

// The colour edits apply live, so the two ways out of this editor throw away
// work the user is watching.

const h = vi.hoisted(() => ({
  save: vi.fn().mockResolvedValue(undefined),
  config: vi.fn(),
  focusEditor: vi.fn(),
  showToast: vi.fn(),
}));

vi.mock("../../stores/global/config", () => ({
  configStore: { config: () => h.config(), save: h.save },
}));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({ editor: { focusEditor: h.focusEditor } }),
}));

vi.mock("../../components/Notifications/Toast", () => ({
  showToast: h.showToast,
  default: () => null,
}));

import ThemeEditor, {
  openThemeEditor,
  closeThemeEditor,
} from "../../components/ThemeEditor/ThemeEditor";
import ConfirmDialog from "../../components/ConfirmDialog/ConfirmDialog";

function baseConfig(): Partial<WritConfig> {
  return { theme: { preset: "writ-light", overrides: {} } };
}

function both() {
  return (
    <>
      <ThemeEditor />
      <ConfirmDialog />
    </>
  );
}

beforeEach(() => {
  h.save.mockReset().mockResolvedValue(undefined);
  h.config.mockReset().mockReturnValue(baseConfig());
  h.showToast.mockReset();
  themeStore.setAppearance({
    polarity: "light",
    accent: "pine",
    prose_face: "system",
    interface_text_size: null,
  });
  themeStore.loadConfig({ preset: "writ-light", overrides: {} });
});

afterEach(() => {
  closeThemeEditor();
  cleanup();
});

describe("leaving the theme editor", () => {
  it("asks before a click on the scrim throws the colours away", async () => {
    const screen = render(both);
    openThemeEditor();
    themeStore.setOverride("bg.canvas", "#123456");

    fireEvent.click(screen.container.querySelector(".theme-editor-overlay")!);

    await waitFor(() => expect(screen.container.querySelector(".confirm-dialog")).toBeTruthy());
    expect(screen.container.querySelector(".theme-editor")).toBeTruthy();
    expect(screen.getByText("Discard your changes?")).toBeTruthy();

    fireEvent.click(screen.container.querySelector(".confirm-cancel")!);
    await waitFor(() => expect(screen.container.querySelector(".confirm-dialog")).toBeNull());
    expect(screen.container.querySelector(".theme-editor")).toBeTruthy();
  });

  it("closes without asking when no colour was touched", async () => {
    const screen = render(both);
    openThemeEditor();

    fireEvent.click(screen.container.querySelector(".theme-editor-overlay")!);

    await waitFor(() => expect(screen.container.querySelector(".theme-editor")).toBeNull());
    expect(screen.container.querySelector(".confirm-dialog")).toBeNull();
  });
});

describe("resetting every colour", () => {
  it("asks first, and is named the way the shortcut editor names it", async () => {
    const screen = render(both);
    openThemeEditor();
    themeStore.setOverride("bg.canvas", "#123456");

    const reset = screen.container.querySelector<HTMLButtonElement>("[data-action='reset-theme']")!;
    expect(reset.textContent!.trim()).toBe("Reset all");
    fireEvent.click(reset);

    await waitFor(() => expect(screen.container.querySelector(".confirm-dialog")).toBeTruthy());
    expect(themeStore.overrides()["bg.canvas"]).toBe("#123456");

    fireEvent.click(screen.container.querySelector(".confirm-accept")!);
    await waitFor(() => expect(themeStore.overrides()["bg.canvas"]).toBeUndefined());
  });

  it("does not ask when there is nothing to reset", async () => {
    const screen = render(both);
    openThemeEditor();

    fireEvent.click(screen.container.querySelector("[data-action='reset-theme']")!);

    await Promise.resolve();
    expect(screen.container.querySelector(".confirm-dialog")).toBeNull();
  });
});

describe("the editor's own words", () => {
  it("takes its name from the title on screen", () => {
    const screen = render(both);
    openThemeEditor();

    const dialog = screen.container.querySelector<HTMLElement>(".theme-editor")!;
    expect(dialog.getAttribute("aria-label")).toBeNull();
    const titleId = dialog.getAttribute("aria-labelledby")!;
    expect(screen.container.querySelector(`#${titleId}`)!.textContent).toBe("Customize theme");
  });

  it("says it could not save the theme", async () => {
    h.save.mockRejectedValueOnce(new Error("disk"));
    const screen = render(both);
    openThemeEditor();

    fireEvent.click(screen.container.querySelector("[data-action='save-theme']")!);

    await waitFor(() => expect(h.showToast).toHaveBeenCalled());
    expect(h.showToast).toHaveBeenCalledWith("Could not save the theme", "error");
  });
});
