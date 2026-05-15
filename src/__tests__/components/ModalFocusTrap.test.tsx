import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";

const h = vi.hoisted(() => ({
  focusEditor: vi.fn(),
}));

vi.mock("../../stores/theme", () => ({
  themeStore: {
    toConfig: () => ({}),
    loadConfig: vi.fn(),
    resolvedTokens: () => ({}),
    setOverride: vi.fn(),
    setPreset: vi.fn(),
    resetOverrides: vi.fn(),
    presetId: () => "default",
    presets: () => [{ id: "default", name: "Default" }],
    activePreset: () => ({}),
  },
}));

vi.mock("../../stores/config", () => ({
  configStore: {
    config: () => ({ theme: {} }),
    save: vi.fn().mockResolvedValue(undefined),
  },
}));

vi.mock("../../stores/editor", () => ({
  editorStore: { focusEditor: h.focusEditor },
}));

vi.mock("../../types/theme", () => ({
  TOKEN_GROUPS: [],
}));

vi.mock("../Notifications/Toast", () => ({
  showToast: vi.fn(),
}));

import ThemeEditor, { openThemeEditor, closeThemeEditor } from "../../components/ThemeEditor/ThemeEditor";

describe("ThemeEditor focus trap", () => {
  afterEach(() => {
    closeThemeEditor();
    cleanup();
    h.focusEditor.mockClear();
    while (document.body.firstChild) document.body.removeChild(document.body.firstChild);
  });

  const tick = () => new Promise<void>((r) => queueMicrotask(() => r()));

  it("applies inert to peers while open and removes on close", async () => {
    const peer = document.createElement("div");
    peer.appendChild(document.createElement("button"));
    document.body.appendChild(peer);

    render(() => <ThemeEditor />);
    openThemeEditor();
    await tick();
    expect(peer.hasAttribute("inert")).toBe(true);

    closeThemeEditor();
    await tick();
    expect(peer.hasAttribute("inert")).toBe(false);
  });

  it("focuses an element inside the dialog on open", async () => {
    render(() => <ThemeEditor />);
    openThemeEditor();
    await tick();
    const dialog = document.querySelector<HTMLElement>(".theme-editor")!;
    expect(dialog).not.toBeNull();
    expect(dialog.contains(document.activeElement)).toBe(true);
  });

  it("Escape closes the dialog", async () => {
    render(() => <ThemeEditor />);
    openThemeEditor();
    await tick();
    const dialog = document.querySelector<HTMLElement>(".theme-editor")!;
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(document.querySelector(".theme-editor")).toBeNull();
  });

  it("falls back to editor focus when previouslyFocused is body", async () => {
    if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
    render(() => <ThemeEditor />);
    openThemeEditor();
    await tick();
    closeThemeEditor();
    await tick();
    expect(h.focusEditor).toHaveBeenCalled();
  });
});
